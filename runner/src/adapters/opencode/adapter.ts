// OpenCode threads: one `opencode acp` process per live thread, driven over
// the Agent Client Protocol (acp.ts), with session/update turned into
// events by normalize.ts.
//
// What ACP doesn't cover goes through `opencode api`, a request to the same
// background service: the session's permission rules (so Ask mode really
// asks, whatever the user's opencode.json says), Threadbear's standing
// instructions, and the model list.
//
// Permission rules: in Full every action is allowed. Otherwise every action
// asks except reading and searching, and the threadbear tools, which ask
// for themselves (codex/tools.ts, as on Codex). "Always allow" is kept per
// thread, as an "opencode:<tool>" rule, and answered here: OpenCode's own
// "always" would save it for the whole project.
import fs from 'node:fs';
import os from 'node:os';
import { execFile } from 'node:child_process';
import type { Account, EffortLevel, ModelChoice, PermissionMode } from '../../protocol.ts';
import type { Logger } from '../../log.ts';
import type { ProcessRegistry } from '../../proc.ts';
import type { AdapterOutput, AgentAdapter, AgentSession, SessionSink, StartSessionOptions, ToolApprovalAnswer, TurnResult, UserInput } from '../types.ts';
import { SessionNotFoundError } from '../types.ts';
import { fileContext } from '../context.ts';
import { runsAsCodeByName } from '../../code-writes.ts';
import type { SlashCommand } from '../../slash-commands.ts';
import { CodexTools, toolGuidance, type ToolDefinition } from '../codex/tools.ts';
import { AcpError, AcpProcess } from './acp.ts';
import { McpBridge } from './mcp-bridge.ts';
import { OpencodeNormalizer } from './normalize.ts';

export type OpencodeAdapterDeps = {
  opencodeBinary: () => string | undefined;
  registry: ProcessRegistry;
  log: Logger;
  toolsFor?: (threadId: string) => ToolDefinition[];
  extraGuidance?: () => string;
  // `opencode api` (tests pass their own).
  api?: ApiCall;
};

export type ApiCall = (binary: string, method: string, path: string, body?: unknown, cwd?: string) => Promise<unknown>;

export type PermissionRule = { action: string; resource: string; effect: 'allow' | 'ask' | 'deny' };

// Actions that only look: allowed outside Full too, as Claude's Ask mode
// lets reads through.
const READ_ONLY = ['read', 'grep', 'glob', 'list', 'lsp', 'todowrite', 'todoread', 'skill', 'question'];

// The session's rules for a Threadbear permission mode. OpenCode takes the
// last rule that matches, so the catch-all goes first.
export function permissionRules(mode: PermissionMode): PermissionRule[] {
  if (mode === 'full') return [{ action: '*', resource: '*', effect: 'allow' }];
  return [
    { action: '*', resource: '*', effect: 'ask' },
    ...READ_ONLY.map((action): PermissionRule => ({ action, resource: '*', effect: 'allow' })),
    { action: 'threadbear_*', resource: '*', effect: 'allow' },
  ];
}

const RULE_PREFIX = 'opencode:';

export const defaultApi: ApiCall = (binary, method, apiPath, body, cwd) =>
  new Promise((resolve, reject) => {
    const args = ['api', method, apiPath, ...(body === undefined ? [] : ['-d', JSON.stringify(body)])];
    execFile(binary, args, { cwd: cwd ?? os.tmpdir(), timeout: 30_000, maxBuffer: 32 * 1024 * 1024 }, (err, stdout, stderr) => {
      if (err) return reject(new Error(`opencode api ${method} ${apiPath}: ${(stderr || err.message).trim().slice(0, 300)}`));
      const text = stdout.trim();
      if (/^HTTP \d{3}/.test(text)) return reject(new Error(`opencode api ${method} ${apiPath}: ${text.slice(0, 300)}`));
      try {
        resolve(text ? JSON.parse(text) : undefined);
      } catch {
        resolve(text);
      }
    });
  });

// /api/model's rows as the picker's models, enabled ones only.
export function opencodeModelChoices(rows: unknown): ModelChoice[] {
  const list = Array.isArray(rows) ? rows : [];
  const out: ModelChoice[] = [];
  for (const r of list as Record<string, unknown>[]) {
    if (!r || r.enabled === false || r.status === 'deprecated') continue;
    const provider = typeof r.providerID === 'string' ? r.providerID : '';
    const model = typeof r.modelID === 'string' ? r.modelID : typeof r.id === 'string' ? r.id : '';
    if (!provider || !model) continue;
    const id = `${provider}/${model}`;
    out.push({ id, model: id, title: typeof r.name === 'string' && r.name ? r.name : model, description: provider });
  }
  return out;
}

export class OpencodeAdapter implements AgentAdapter {
  readonly provider = 'opencode';
  readonly wakesAfterTasks = false;
  private deps: OpencodeAdapterDeps;
  private bridge: McpBridge;

  constructor(deps: OpencodeAdapterDeps) {
    this.deps = deps;
    this.bridge = new McpBridge(deps.log);
  }

  private binary(): string {
    const b = this.deps.opencodeBinary();
    if (!b) throw new Error("OpenCode isn't installed on this Mac (no opencode binary found), so this thread can't start.");
    return b;
  }

  start(opts: StartSessionOptions, sink: SessionSink): AgentSession {
    if (opts.confined) throw new Error("This thread runs in a sandbox, and OpenCode can't, so it wasn't started. Move it to a Claude login.");
    const s = new OpencodeSession(opts, sink, this.deps, this.binary(), this.bridge);
    s.start();
    return s;
  }

  async models(_account: Account): Promise<ModelChoice[]> {
    const r = (await (this.deps.api ?? defaultApi)(this.binary(), 'GET', '/api/model')) as { data?: unknown } | undefined;
    return opencodeModelChoices(r?.data);
  }
}

type Turn = { id?: string; startedAt: number };

export class OpencodeSession implements AgentSession {
  private opts: StartSessionOptions;
  private sink: SessionSink;
  private deps: OpencodeAdapterDeps;
  private binary: string;
  private bridge: McpBridge;
  private proc: AcpProcess;
  private norm: OpencodeNormalizer;
  private api: ApiCall;
  private sessionId?: string;
  private ready: Promise<void>;
  private token?: string;
  private mode: PermissionMode;
  private model?: string;
  private turn?: Turn;
  private queue: UserInput[] = [];
  private rules = new Set<string>();
  private cmds: SlashCommand[] = [];
  private cost?: number;
  private ended = false;
  private exitedOnce = false;
  private approvals = new Set<AbortController>();

  constructor(opts: StartSessionOptions, sink: SessionSink, deps: OpencodeAdapterDeps, binary: string, bridge: McpBridge) {
    this.opts = opts;
    this.sink = sink;
    this.deps = deps;
    this.binary = binary;
    this.bridge = bridge;
    this.api = deps.api ?? defaultApi;
    this.mode = opts.permissionMode;
    this.model = opts.model;
    for (const r of opts.allowRules ?? []) this.rules.add(r);
    this.norm = new OpencodeNormalizer(fileContext(opts.cwd, opts.mediaDir, opts.blobsDir));
    this.proc = new AcpProcess({ binary, cwd: opts.cwd, env: opts.env, registry: deps.registry, log: deps.log, label: opts.threadId });
    this.proc.onNotification = (method, params) => this.onNotification(method, params);
    this.proc.onRequest = (method, params) => this.onRequest(method, params);
    this.proc.onExit = (e) => this.exited(e);
    this.ready = Promise.resolve();
  }

  start(): void {
    this.ready = this.open().catch((e: unknown) => {
      this.deps.log.warn('opencode session did not start', { threadId: this.opts.threadId, error: String(e) });
      this.proc.kill();
      this.exited(e instanceof SessionNotFoundError ? e : new Error(`OpenCode didn't start: ${(e as Error)?.message ?? String(e)}`));
      throw e;
    });
    this.ready.catch(() => undefined);
  }

  private async open(): Promise<void> {
    const { opts } = this;
    await this.proc.start();
    if (this.proc.version) this.sink.output({ kind: 'cliVersion', version: this.proc.version });
    const mcpServers: unknown[] = [];
    const tools = this.deps.toolsFor?.(opts.threadId) ?? [];
    if (tools.length) {
      const runner = new CodexTools(tools, {
        requestApproval: (req) => this.sink.requestApproval(req),
        cwd: () => this.opts.cwd,
        mode: () => this.mode,
        rules: () => this.rules,
        addRules: (rules) => {
          for (const r of rules) this.rules.add(r);
          this.sink.output({ kind: 'allowRules', rules });
        },
      });
      const { token, server } = await this.bridge.add({ tools, runner });
      this.token = token;
      mcpServers.push(server);
    }
    let result: Record<string, unknown>;
    if (opts.resume) {
      try {
        result = await this.proc.request('session/resume', { sessionId: opts.sessionId, cwd: opts.cwd, mcpServers });
      } catch (e) {
        if (e instanceof AcpError && /not found/i.test(e.message)) throw new SessionNotFoundError();
        throw e;
      }
      this.sessionId = opts.sessionId;
    } else {
      result = await this.proc.request('session/new', { cwd: opts.cwd, mcpServers });
      this.sessionId = String(result.sessionId);
    }
    let current = currentModel(result);
    if (this.model && this.model !== current) {
      const r = await this.proc.request<Record<string, unknown>>('session/set_config_option', { sessionId: this.sessionId, configId: 'model', value: this.model }).catch((e: unknown) => {
        this.sink.output({ kind: 'event', event: { type: 'system.note', text: `OpenCode couldn't switch to ${this.model}: ${(e as Error).message}` } });
        return undefined;
      });
      current = currentModel(r) ?? current;
    }
    await this.applyMode(true);
    await this.putInstructions(tools);
    this.sink.output({ kind: 'session', nativeSessionId: this.sessionId, ...(current ? { model: current } : {}) });
  }

  // Threadbear's standing instructions (the thread's projects, and how to
  // use its tools), as one of the session's instruction entries.
  private async putInstructions(tools: ToolDefinition[]): Promise<void> {
    const extra = this.deps.extraGuidance?.() ?? '';
    const text = [this.opts.instructions, tools.length ? toolGuidance(tools) : '', extra].filter(Boolean).join('\n\n');
    if (!text || !this.sessionId) return;
    await this.api(this.binary, 'PUT', `/api/experimental/session/${this.sessionId}/instructions/entries/threadbear`, { value: text }, this.opts.cwd).catch((e: unknown) => {
      this.deps.log.warn('could not give opencode its instructions', { threadId: this.opts.threadId, error: String(e) });
    });
  }

  // The session's permission rules and agent for the thread's mode.
  private async applyMode(first = false): Promise<void> {
    if (!this.sessionId) return;
    await this.api(this.binary, 'PATCH', `/api/session/${this.sessionId}`, { permissions: permissionRules(this.mode) }, this.opts.cwd);
    const agent = this.mode === 'plan' ? 'plan' : 'build';
    if (first && agent === 'build') return;
    await this.proc.request('session/set_config_option', { sessionId: this.sessionId, configId: 'mode', value: agent }).catch((e: unknown) => {
      this.deps.log.warn('could not set the opencode agent', { threadId: this.opts.threadId, agent, error: String(e) });
    });
  }

  private onNotification(method: string, params: Record<string, unknown>): void {
    if (method !== 'session/update') return;
    if (this.sessionId && params.sessionId && params.sessionId !== this.sessionId) return;
    const u = (params.update ?? {}) as Record<string, unknown>;
    if (u.sessionUpdate === 'available_commands_update' && Array.isArray(u.availableCommands)) {
      this.cmds = (u.availableCommands as Record<string, unknown>[])
        .filter((c) => typeof c?.name === 'string')
        .map((c) => ({ name: String(c.name), ...(typeof c.description === 'string' ? { description: c.description } : {}), source: 'cli' as const }));
      return;
    }
    if (u.sessionUpdate === 'usage_update') {
      const amount = Number((u.cost as { amount?: unknown } | undefined)?.amount);
      if (Number.isFinite(amount)) this.cost = amount;
    }
    // Updates outside a turn are history being replayed, or late ones.
    if (!this.turn) return;
    this.emit(this.norm.update(u));
  }

  private emit(outs: AdapterOutput[]): void {
    for (const o of outs) this.sink.output(o);
  }

  private async onRequest(method: string, params: Record<string, unknown>): Promise<unknown> {
    if (method === 'session/request_permission') return this.permission(params);
    throw new AcpError(-32601, `Threadbear doesn't handle ${method}`);
  }

  private async permission(params: Record<string, unknown>): Promise<unknown> {
    const options = (Array.isArray(params.options) ? params.options : []) as { optionId: string; kind: string }[];
    const pick = (kind: string) => options.find((o) => o.kind === kind)?.optionId;
    const answer = (kind: 'allow_once' | 'reject_once') => {
      const optionId = pick(kind);
      return optionId ? { outcome: { outcome: 'selected', optionId } } : { outcome: { outcome: 'cancelled' } };
    };
    const tc = (params.toolCall ?? {}) as Record<string, unknown>;
    const card = this.norm.forApproval(tc);
    this.emit(card.out);
    const raw = String(tc.name ?? '');
    const code = card.paths.some((p) => runsAsCodeByName(p) || this.opts.runsAsCode?.(p));
    const rule = `${RULE_PREFIX}${raw}`;
    if (raw && !code && this.rules.has(rule)) return answer('allow_once');
    const abort = new AbortController();
    this.approvals.add(abort);
    let ans: ToolApprovalAnswer;
    try {
      ans = await this.sink.requestApproval({
        toolUseId: card.itemId,
        name: card.name,
        title: card.title,
        input: card.input,
        canAlwaysAllow: Boolean(raw) && !code,
        signal: abort.signal,
      });
    } finally {
      this.approvals.delete(abort);
    }
    if (ans.decision === 'allowAlways' && raw && !code) {
      this.rules.add(rule);
      this.sink.output({ kind: 'allowRules', rules: [rule] });
    }
    if (ans.mode && ans.mode !== this.mode) void this.setPermissionMode(ans.mode);
    return answer(ans.decision === 'allow' || ans.decision === 'allowAlways' ? 'allow_once' : 'reject_once');
  }

  send(input: UserInput): void {
    // ACP has no way into a running turn: it runs next.
    if (this.turn) {
      this.queue.push(input);
      return;
    }
    this.runTurn(input);
  }

  private runTurn(input: UserInput): void {
    const turn: Turn = { id: input.id, startedAt: Date.now() };
    this.turn = turn;
    this.norm.startTurn();
    void this.prompt(turn, input);
  }

  private async prompt(turn: Turn, input: UserInput): Promise<void> {
    let result: TurnResult;
    try {
      await this.ready;
      const r = await this.proc.request<{ stopReason?: string; usage?: Record<string, unknown> }>('session/prompt', { sessionId: this.sessionId, prompt: promptBlocks(input) });
      result = this.resultFor(turn, r);
    } catch (e) {
      const message = (e as Error)?.message ?? String(e);
      result = { outcome: 'failed', durationMs: Date.now() - turn.startedAt, error: message, ...(turn.id ? { answers: turn.id } : {}), ...errorKind(message) };
    }
    if (this.turn !== turn) return;
    this.emit(this.norm.finishTurn());
    if (this.mode === 'plan' && result.outcome === 'completed' && this.norm.lastText) {
      this.sink.output({ kind: 'planProposed', itemId: `plan:${this.norm.lastText.itemId}`, text: this.norm.lastText.text });
    }
    this.turn = undefined;
    if (this.queue.length) result.queuedTurns = this.queue.length;
    this.sink.output({ kind: 'result', result });
    const next = this.queue.shift();
    if (next && !this.ended) this.runTurn(next);
  }

  private resultFor(turn: Turn, r: { stopReason?: string; usage?: Record<string, unknown> } | undefined): TurnResult {
    const stop = r?.stopReason ?? 'end_turn';
    const base: TurnResult = {
      outcome: stop === 'cancelled' ? 'interrupted' : stop === 'refusal' ? 'failed' : 'completed',
      durationMs: Date.now() - turn.startedAt,
      ...(turn.id ? { answers: turn.id } : {}),
      ...(r?.usage ? { usage: r.usage } : {}),
      ...(this.cost !== undefined ? { totalCostUsd: this.cost } : {}),
    };
    if (stop === 'refusal') base.error = 'The model refused to carry on.';
    if (stop === 'max_tokens') base.error = 'The model stopped at its output limit.';
    return base;
  }

  async interrupt(): Promise<void> {
    for (const a of this.approvals) a.abort();
    if (this.turn && this.sessionId) this.proc.notify('session/cancel', { sessionId: this.sessionId });
  }

  async setModel(model: string | undefined): Promise<void> {
    this.model = model;
    if (!model || !this.sessionId) return;
    await this.ready.catch(() => undefined);
    const r = await this.proc.request<Record<string, unknown>>('session/set_config_option', { sessionId: this.sessionId, configId: 'model', value: model });
    const current = currentModel(r);
    if (current) this.sink.output({ kind: 'session', nativeSessionId: this.sessionId, model: current });
  }

  async setEffort(_effort: EffortLevel | undefined): Promise<void> {
    // OpenCode takes no effort level (protocol, "Models and effort across providers").
  }

  async setPermissionMode(mode: PermissionMode): Promise<void> {
    const was = this.mode;
    this.mode = mode;
    await this.ready.catch(() => undefined);
    if (mode === was) return;
    await this.applyMode();
  }

  async commands(): Promise<SlashCommand[]> {
    return this.cmds;
  }

  async end(opts: { hard?: boolean } = {}): Promise<void> {
    this.ended = true;
    this.queue = [];
    if (this.turn) await this.interrupt();
    if (opts.hard) this.proc.kill();
    else await this.proc.stop();
  }

  kill(): void {
    this.ended = true;
    this.queue = [];
    if (this.turn && this.sessionId) this.proc.notify('session/cancel', { sessionId: this.sessionId });
    this.proc.kill();
  }

  private exited(error?: Error): void {
    if (this.exitedOnce) return;
    this.exitedOnce = true;
    if (this.token) this.bridge.remove(this.token);
    for (const a of this.approvals) a.abort();
    this.sink.exited(error);
  }
}

function currentModel(r: Record<string, unknown> | undefined): string | undefined {
  const opts = Array.isArray(r?.configOptions) ? (r.configOptions as Record<string, unknown>[]) : [];
  const m = opts.find((o) => o?.id === 'model');
  return typeof m?.currentValue === 'string' ? m.currentValue : undefined;
}

// The message as ACP content blocks: its text, then its pictures.
export function promptBlocks(input: UserInput): Record<string, unknown>[] {
  const blocks: Record<string, unknown>[] = [];
  if (input.text) blocks.push({ type: 'text', text: input.text });
  for (const img of input.images ?? []) {
    try {
      blocks.push({ type: 'image', mimeType: img.mediaType, data: fs.readFileSync(img.path).toString('base64') });
    } catch {
      blocks.push({ type: 'text', text: `[A picture that couldn't be read: ${img.path}]` });
    }
  }
  if (!blocks.length) blocks.push({ type: 'text', text: '' });
  return blocks;
}

function errorKind(message: string): Pick<TurnResult, 'errorKind'> {
  if (/rate.?limit|quota|usage limit|insufficient.?credit/i.test(message)) return { errorKind: 'usageLimit' };
  if (/context (window|length)|too many tokens|prompt is too long/i.test(message)) return { errorKind: 'contextWindow' };
  if (/unauthori[sz]ed|api key|auth/i.test(message)) return { errorKind: 'auth' };
  if (/ECONNREFUSED|ENOTFOUND|ETIMEDOUT|network/i.test(message)) return { errorKind: 'network' };
  return {};
}
