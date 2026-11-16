// Threads on an agent that speaks ACP (acp/agents.ts): one process per live
// thread, and everything it wants to do comes through here first.
//
// The agent is set to ask before every tool (AcpAgent.env, or for Grok the
// hook its args load: grok-guard.ts), and each request is answered from the
// thread's permission mode at that moment. So Ask, Auto and Plan never run a
// tool unguarded, and leaving Full takes effect at once, with no new process.
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { EFFORT_LEVELS, isEffortLevel, type Account, type EffortLevel, type ImageRef, type ModelChoice, type PermissionMode } from '../../protocol.ts';
import type { Logger } from '../../log.ts';
import type { ProcessRegistry } from '../../proc.ts';
import { cardInput, displayPath, tooLongForCard } from '../../tools.ts';
import { codeWriteNote, runsAsCodeByName, type CodeWrite } from '../../code-writes.ts';
import { placePictures } from '../../pictures.ts';
import { providerInfo, providerName } from '../../providers.ts';
import { firstLineSafely } from '../../safe-cut.ts';
import type { SlashCommand } from '../../slash-commands.ts';
import { cliEnv } from '../claude/adapter.ts';
import { commandRule, ruleMayCover, shellWords } from '../codex/approvals.ts';
import { toolGuidance, type ToolDefinition } from '../codex/tools.ts';
import { fileContext, type NormalizerContext } from '../context.ts';
import type { AdapterOutput, AgentAdapter, AgentSession, SessionSink, StartSessionOptions, ToolApprovalAnswer, TurnResult, UserInput } from '../types.ts';
import { SessionNotFoundError } from '../types.ts';
import { agentArgs, type AcpAgent } from './agents.ts';
import { AcpExitedError, AcpProcess, AcpRpcError, AUTH_REQUIRED, type AcpTimings, type InitializeResult, type RequestId } from './connection.ts';
import { AcpMcpServer } from './mcp-http.ts';
import { AcpNormalizer, absolutePath, codeModeThreadbearCall, isPlainEdit, isPlainObject, requestPaths, writesFiles, type AcpToolCall } from './normalize.ts';

// Where an execute call can say it runs (OpenCode's bash tool: workdir).
const FOLDER_KEYS = ['workdir', 'cwd', 'workingDirectory', 'directory', 'dir'];
// What an execute call can carry that doesn't change what runs or where.
const BENIGN_KEYS = new Set(['description', 'timeout']);

function realFolder(p: string): string | undefined {
  try {
    return fs.realpathSync.native(p);
  } catch {
    return undefined;
  }
}

export type AcpAdapterDeps = {
  agent: AcpAgent;
  // Read for every process: runner.json's path, or wherever it's installed.
  binary: () => string | undefined;
  registry: ProcessRegistry;
  log: Logger;
  toolsFor?: (threadId: string) => ToolDefinition[];
  extraGuidance?: () => string;
  timings?: Partial<AcpTimings>;
  env?: NodeJS.ProcessEnv;
  now?: () => number;
  // The runner's data dir, where an agent keeps what Threadbear gives it
  // (Grok's hook: grok-guard.ts).
  dataDir?: string;
};

// "Always allow" for a file edit: the thread's edits, while they aren't to
// a file that runs as code. A command's rule is the exact command in that
// folder (codex/approvals.ts).
export const EDIT_RULE = 'acp:edit';

type ConfigOption = { id?: string; category?: string; type?: string; currentValue?: unknown; options?: unknown[] };
// Grok's `models` also says each model's effort levels and its default one.
type ModelState = { availableModels?: unknown[] } | null;
type SessionResult = { sessionId?: string; configOptions?: ConfigOption[] | null; modes?: { currentModeId?: string; availableModes?: { id?: string }[] } | null; models?: ModelState };

// The effort levels a model lists (Grok: _meta.reasoningEfforts), those that
// are Threadbear's own, and the one it marks as its default.
export function modelEfforts(models: ModelState | undefined): Map<string, { levels: EffortLevel[]; defaultEffort?: EffortLevel }> {
  const out = new Map<string, { levels: EffortLevel[]; defaultEffort?: EffortLevel }>();
  for (const m of Array.isArray(models?.availableModels) ? models.availableModels : []) {
    if (!isPlainObject(m) || typeof m.modelId !== 'string' || !isPlainObject(m._meta) || !Array.isArray(m._meta.reasoningEfforts)) continue;
    const levels: EffortLevel[] = [];
    let defaultEffort: EffortLevel | undefined;
    for (const e of m._meta.reasoningEfforts) {
      if (!isPlainObject(e) || !isEffortLevel(e.id)) continue;
      levels.push(e.id);
      if (e.default === true) defaultEffort = e.id;
    }
    // In Threadbear's order, lowest first, whatever order the agent lists them.
    levels.sort((a, b) => EFFORT_LEVELS.indexOf(a) - EFFORT_LEVELS.indexOf(b));
    if (levels.length) out.set(m.modelId, { levels, ...(defaultEffort ? { defaultEffort } : {}) });
  }
  return out;
}

export function modelChoices(options: ConfigOption[] | null | undefined, models?: ModelState): ModelChoice[] {
  const model = options?.find((o) => o.category === 'model');
  if (!model || !Array.isArray(model.options)) return [];
  const efforts = modelEfforts(models);
  const out: ModelChoice[] = [];
  const add = (o: unknown) => {
    if (!isPlainObject(o)) return;
    if (Array.isArray(o.options)) return o.options.forEach(add); // a named group
    if (typeof o.value !== 'string' || !o.value) return;
    const title = typeof o.name === 'string' && o.name ? o.name : o.value;
    const e = efforts.get(o.value);
    out.push({ id: o.value, model: o.value, title, ...(e ? { effortLevels: e.levels, ...(e.defaultEffort ? { defaultEffort: e.defaultEffort } : {}) } : {}) });
  };
  model.options.forEach(add);
  return out;
}

// Grok adds the hook that asked to the title of a request it asks about
// ("Write `a.txt` — hook 'plugin/threadbear-ask/...' asks: ..."). The card
// says what the call does, not which of Threadbear's own pieces sent it.
export function withoutHookNote(call: AcpToolCall, meta: unknown): AcpToolCall {
  const hook = isPlainObject(meta) && isPlainObject(meta.hookAsk) ? meta.hookAsk.hookName : undefined;
  if (typeof hook !== 'string' || !hook || typeof call.title !== 'string') return call;
  const at = call.title.indexOf(` — hook '${hook}'`);
  return at > 0 ? { ...call, title: call.title.slice(0, at) } : call;
}

function notFound(e: unknown): boolean {
  if (e instanceof AcpRpcError && e.code === -32002) return true;
  return /not found|no such session|does not exist/i.test(e instanceof Error ? e.message : String(e));
}

export class AcpAdapter implements AgentAdapter {
  readonly provider;
  readonly settingsApplyNextTurn = true;
  readonly wakesAfterTasks = false;
  private deps: AcpAdapterDeps;

  constructor(deps: AcpAdapterDeps) {
    this.deps = deps;
    this.provider = deps.agent.provider;
  }

  private env(account: Account, threadEnv?: Record<string, string>): Record<string, string | undefined> {
    const env = cliEnv(account, this.deps.env ?? process.env);
    delete env.CLAUDE_CONFIG_DIR;
    delete env.CLAUDE_AGENT_SDK_CLIENT_APP;
    // Last, so a thread's own variables can't take these off.
    return { ...env, ...threadEnv, ...this.deps.agent.env() };
  }

  private binary(): string {
    const name = providerName(this.provider);
    const bin = this.deps.binary();
    if (!bin) throw new Error(`Threadbear can't find ${name} on this Mac.`);
    return bin;
  }

  start(opts: StartSessionOptions, sink: SessionSink): AgentSession {
    // None of these run in the sandbox (sandbox.ts).
    if (opts.confined) throw new Error(`This thread runs in a sandbox, and ${providerName(this.provider)} can't, so it wasn't started. Move it to a Claude login.`);
    const refused = this.deps.agent.refuses?.({ cwd: opts.cwd, mode: opts.permissionMode });
    if (refused) throw new Error(refused);
    const s = new AcpSession(opts, sink, this.deps, this.binary(), this.env(opts.account, opts.env));
    s.start();
    return s;
  }

  // What session/new offers. It makes a session in the agent's own list; a
  // folder of temp means it's in nobody's way, and it's closed where the
  // agent can.
  async models(account: Account): Promise<ModelChoice[]> {
    const { deps } = this;
    const cwd = fs.mkdtempSync(path.join(os.tmpdir(), 'threadbear-acp-'));
    const p = new AcpProcess({ binary: this.binary(), args: agentArgs(deps.agent, this.binary(), { dataDir: deps.dataDir }), env: this.env(account), cwd, registry: deps.registry, log: deps.log, label: `models:${account.id}`, timings: deps.timings });
    try {
      const init = await p.start();
      if (!deps.agent.identifies(init)) throw new Error(`The program found for ${providerName(this.provider)} isn't it.`);
      const r = await p.request<SessionResult>('session/new', { cwd, mcpServers: [] });
      if (r?.sessionId && init.agentCapabilities?.sessionCapabilities?.close !== undefined) await p.request('session/close', { sessionId: r.sessionId }).catch(() => undefined);
      return modelChoices(r?.configOptions, r?.models);
    } finally {
      await p.close().catch(() => undefined);
      fs.rmSync(cwd, { recursive: true, force: true });
    }
  }
}

type State = 'starting' | 'ready' | 'turn' | 'closing' | 'closed';
type Pending = { abort: AbortController; options: PermissionOption[] };
type PermissionOption = { optionId?: string; kind?: string };

export class AcpSession implements AgentSession {
  private opts: StartSessionOptions;
  private sink: SessionSink;
  private deps: AcpAdapterDeps;
  private now: () => number;
  private proc: AcpProcess;
  private mcp?: AcpMcpServer;
  private normalizer: AcpNormalizer;
  private ctx: NormalizerContext;
  private state: State = 'starting';
  private sessionId?: string;
  private init: InitializeResult = {};
  private held: UserInput[] = [];
  private turn?: { id?: string; startedAt: number; interrupted: boolean; dropped: string[] };
  private pending = new Map<RequestId, Pending>();
  private rules: Set<string>;
  private mode: PermissionMode;
  private model?: string;
  private modelChanged = false;
  private effort?: EffortLevel;
  private effortChanged = false;
  // Each model's own effort level, for going back to it (Grok's list).
  private ownEfforts = new Map<string, { levels: EffortLevel[]; defaultEffort?: EffortLevel }>();
  // The threadbear tools this session's MCP server offers, as the agent names them.
  private ownTools = new Set<string>();
  private configOptions: ConfigOption[] = [];
  // The plugin whose hook must be reported running before the model does
  // anything in a turn (Grok's guard), and whether it has been this turn.
  private guard?: string;
  private guardSeen = false;
  // Whether the agent trusted the folder when this process started (Grok).
  private trusted = false;
  private currentMode?: string;
  private cliCommands: SlashCommand[] = [];
  // Said once, before the first message of a new session.
  private preamble?: string;
  private replaying = false;
  private exitedCalled = false;
  private exitPromise: Promise<void>;
  private resolveExit!: () => void;

  constructor(opts: StartSessionOptions, sink: SessionSink, deps: AcpAdapterDeps, binary: string, env: Record<string, string | undefined>) {
    this.opts = opts;
    this.sink = sink;
    this.deps = deps;
    this.now = deps.now ?? Date.now;
    this.mode = opts.permissionMode;
    this.model = opts.model;
    this.effort = opts.effort;
    this.rules = new Set(opts.allowRules ?? []);
    this.exitPromise = new Promise((r) => (this.resolveExit = r));
    this.ctx = fileContext(opts.cwd, opts.mediaDir, opts.blobsDir);
    this.normalizer = new AcpNormalizer(this.ctx);
    this.trusted = deps.agent.trusts?.(opts.cwd) ?? false;
    const args = agentArgs(deps.agent, binary, { dataDir: deps.dataDir, trusted: this.trusted });
    this.guard = deps.agent.guardName?.(args);
    this.proc = new AcpProcess({ binary, args, env, cwd: opts.cwd, registry: deps.registry, log: deps.log, label: opts.threadId, timings: deps.timings });
    this.proc.onNotification = (method, params) => this.notification(method, params);
    this.proc.onRequest = (id, method, params) => this.request(id, method, params);
    this.proc.onExit = (err) => this.exited(err);
  }

  private addRules(rules: string[]): void {
    for (const r of rules) this.rules.add(r);
    this.sink.output({ kind: 'allowRules', rules });
  }

  start(): void {
    void this.boot().catch((e: unknown) => {
      if (this.state === 'closing' || this.state === 'closed') return void this.finish();
      this.deps.log.warn('acp session did not start', { threadId: this.opts.threadId, error: String(e) });
      void this.finish(e instanceof Error ? e : new Error(String(e)));
    });
  }

  private async boot(): Promise<void> {
    const { opts, deps } = this;
    const name = providerName(deps.agent.provider);
    this.init = await this.proc.start();
    if (this.state !== 'starting') return void (await this.proc.close());
    if (!deps.agent.identifies(this.init)) {
      const said = this.init.agentInfo?.name ? ` (it says it's ${this.init.agentInfo.name})` : '';
      throw new Error(`The program found for ${name} isn't ${name}${said}, so this thread wasn't started.`);
    }
    if (this.proc.version) this.sink.output({ kind: 'cliVersion', version: this.proc.version });

    const defs = deps.toolsFor?.(opts.threadId) ?? [];
    let mcpServers: unknown[] = [];
    if (defs.length && this.init.agentCapabilities?.mcpCapabilities?.http) {
      this.mcp = new AcpMcpServer(defs, {
        log: deps.log,
        label: opts.threadId,
        requestApproval: (req) => this.sink.requestApproval(req),
        cwd: () => opts.cwd,
        mode: () => this.mode,
        rules: () => this.rules,
        addRules: (rules) => this.addRules(rules),
      });
      const url = await this.mcp.listen();
      mcpServers = [{ type: 'http', name: 'threadbear', url, headers: [{ name: 'Authorization', value: `Bearer ${this.mcp.token}` }] }];
      this.ownTools = new Set(defs.map((d) => `threadbear__${d.name}`));
    }

    let res: SessionResult;
    try {
      res = await this.openSession(mcpServers);
    } catch (e) {
      if (e instanceof AcpRpcError && e.code === AUTH_REQUIRED) {
        throw Object.assign(new Error(`${name} needs a sign-in. Run \`${providerInfo(deps.agent.provider).loginCommand}\` in a terminal, then send this again.`), { errorKind: 'auth' });
      }
      throw e;
    }
    if (this.state !== 'starting') return;
    this.sessionId = this.sessionId ?? res.sessionId;
    if (typeof this.sessionId !== 'string' || !this.sessionId) throw new Error(`${name} started a session without an id`);
    this.noteOptions(res);
    this.ownEfforts = modelEfforts(res.models);
    if (!opts.resume) {
      this.preamble = [defs.length ? toolGuidance(defs) : '', defs.length ? (deps.extraGuidance?.() ?? '') : '', opts.instructions ?? ''].filter(Boolean).join('\n\n') || undefined;
    }
    await this.applyMode();
    if (this.model) await this.applyModel(this.model);
    if (this.effort) await this.applyEffort(this.effort);
    const model = this.modelNow();
    this.sink.output({ kind: 'session', nativeSessionId: this.sessionId, ...(model ? { model } : {}) });
    this.state = 'ready';
    this.next();
  }

  private openSession(mcpServers: unknown[]): Promise<SessionResult> {
    const { opts } = this;
    const caps = this.init.agentCapabilities;
    if (!opts.resume) return this.proc.request<SessionResult>('session/new', { cwd: opts.cwd, mcpServers });
    this.sessionId = opts.sessionId;
    const common = { sessionId: opts.sessionId, cwd: opts.cwd, mcpServers };
    const opened = caps?.sessionCapabilities?.resume !== undefined
      ? this.proc.request<SessionResult>('session/resume', common)
      : caps?.loadSession
        ? this.load(common)
        : Promise.reject(new SessionNotFoundError('This agent can\'t carry on a session in a new process.'));
    return opened.catch((e: unknown) => {
      if (notFound(e)) throw new SessionNotFoundError(`The session to resume wasn't found (${(e as Error).message}).`);
      throw e;
    });
  }

  // session/load replays the old conversation as updates: already in the
  // transcript, so they're dropped.
  private async load(params: Record<string, unknown>): Promise<SessionResult> {
    this.replaying = true;
    try {
      return await this.proc.request<SessionResult>('session/load', params);
    } finally {
      this.replaying = false;
    }
  }

  private noteOptions(res: { configOptions?: ConfigOption[] | null; modes?: SessionResult['modes'] }): void {
    if (Array.isArray(res.configOptions)) this.configOptions = res.configOptions;
    const mode = this.configOptions.find((o) => o.category === 'mode');
    if (typeof mode?.currentValue === 'string') this.currentMode = mode.currentValue;
    else if (typeof res.modes?.currentModeId === 'string') this.currentMode = res.modes.currentModeId;
  }

  private modelNow(): string | undefined {
    const v = this.configOptions.find((o) => o.category === 'model')?.currentValue;
    return typeof v === 'string' ? v : undefined;
  }

  // Puts the agent in the session mode this thread's mode calls for. If it
  // can't be done, a thread that has to ask doesn't run: Cursor's read-only
  // mode is all that stands between it and its own allow-list.
  // One at a time, and until the agent is where the thread's mode is now:
  // a quick Full then Ask must not end on whichever answer came last.
  private applyMode(): Promise<void> {
    const run = this.modeChain.then(() => this.settleMode());
    this.modeChain = run.catch(() => undefined);
    return run;
  }

  private async settleMode(): Promise<void> {
    for (let i = 0; i < 8; i++) {
      const want = this.deps.agent.modeFor(this.mode);
      if (!want || want === this.currentMode) return;
      await this.setAgentMode(want);
    }
  }

  private async setAgentMode(want: string): Promise<void> {
    try {
      const option = this.configOptions.find((o) => o.category === 'mode');
      if (option?.id) {
        const r = await this.proc.request<SessionResult>('session/set_config_option', { sessionId: this.sessionId, configId: option.id, value: want });
        this.noteOptions(r ?? {});
      } else {
        await this.proc.request('session/set_mode', { sessionId: this.sessionId, modeId: want });
        this.currentMode = want;
      }
    } catch (e) {
      if (this.mode !== 'full') throw new Error(`Couldn't put ${providerName(this.deps.agent.provider)} in its ${want} mode, so this thread wasn't started (${(e as Error).message}).`);
      this.deps.log.info('could not set the agent mode', { threadId: this.opts.threadId, want, error: String(e) });
      return;
    }
    if (this.currentMode !== want && this.mode !== 'full') throw new Error(`Couldn't put ${providerName(this.deps.agent.provider)} in its ${want} mode, so this thread wasn't started.`);
  }

  private modeChain: Promise<unknown> = Promise.resolve();

  private async applyModel(model: string): Promise<void> {
    const option = this.configOptions.find((o) => o.category === 'model');
    if (!option?.id) return;
    try {
      const r = await this.proc.request<SessionResult>('session/set_config_option', { sessionId: this.sessionId, configId: option.id, value: model });
      this.noteOptions(r ?? {});
    } catch (e) {
      this.deps.log.warn('could not set the model', { threadId: this.opts.threadId, model, error: String(e) });
      this.sink.output({ kind: 'event', event: { type: 'system.note', text: `${model} couldn't be picked, so this turn uses ${this.modelNow() ?? 'the default model'}.` } });
    }
  }

  // An agent that has a thought_level option (Grok's reasoning_effort) takes
  // the thread's effort there; undefined is the model's own level. Others have
  // none, and providers.ts says so, so the runner never sends them one.
  private async applyEffort(effort: EffortLevel | undefined): Promise<void> {
    const option = this.configOptions.find((o) => o.category === 'thought_level');
    if (!option?.id) return;
    const model = this.modelNow();
    const value = effort ?? (model ? this.ownEfforts.get(model)?.defaultEffort : undefined);
    if (!value || value === option.currentValue) return;
    try {
      const r = await this.proc.request<SessionResult>('session/set_config_option', { sessionId: this.sessionId, configId: option.id, value });
      this.noteOptions(r ?? {});
    } catch (e) {
      this.deps.log.warn('could not set the effort', { threadId: this.opts.threadId, effort: value, error: String(e) });
      this.sink.output({ kind: 'event', event: { type: 'system.note', text: `${value} effort couldn't be set, so this turn uses ${String(this.effortNow() ?? "the model's own level")}.` } });
    }
  }

  private effortNow(): EffortLevel | undefined {
    const v = this.configOptions.find((o) => o.category === 'thought_level')?.currentValue;
    return isEffortLevel(v) ? v : undefined;
  }

  // ---- From the agent

  private notification(method: string, params: unknown): void {
    if (this.guard && !this.replaying && this.checkGuard(method, params)) return;
    if (this.replaying || method !== 'session/update' || !isPlainObject(params)) return;
    if (this.sessionId && params.sessionId !== undefined && params.sessionId !== this.sessionId) return;
    const update = params.update;
    if (isPlainObject(update)) {
      if (update.sessionUpdate === 'current_mode_update' && typeof update.currentModeId === 'string') this.currentMode = update.currentModeId;
      if (update.sessionUpdate === 'config_option_update' && Array.isArray(update.configOptions)) this.noteOptions({ configOptions: update.configOptions as ConfigOption[] });
      if (update.sessionUpdate === 'available_commands_update' && Array.isArray(update.availableCommands)) this.noteCommands(update.availableCommands);
    }
    this.emit(this.normalizer.handle(update));
  }

  // Grok reports each hook it ran (hook_execution) as a prompt goes in and
  // before each tool. Threadbear's must be among them: a folder's config can
  // turn a plugin off, and then nothing would ask. Anything from the model in
  // a turn before that report, or a report without it, stops the thread
  // there, before any tool runs. True when it stopped it.
  private checkGuard(method: string, params: unknown): boolean {
    if (!isPlainObject(params) || !isPlainObject(params.update)) return false;
    const u = params.update;
    if (method.endsWith('x.ai/session_notification') && u.sessionUpdate === 'hook_execution' && (u.event_name === 'user_prompt_submit' || u.event_name === 'pre_tool_use')) {
      const ran = Array.isArray(u.runs) && u.runs.some((r) => isPlainObject(r) && typeof r.name === 'string' && r.name.startsWith(`plugin/${this.guard}/`));
      if (ran) {
        if (u.event_name === 'user_prompt_submit') this.guardSeen = true;
        return false;
      }
      return this.guardMissing();
    }
    const fromModel = new Set(['agent_message_chunk', 'agent_thought_chunk', 'tool_call', 'tool_call_update', 'plan']);
    if (method === 'session/update' && this.turn && !this.guardSeen && fromModel.has(String(u.sessionUpdate))) return this.guardMissing();
    return false;
  }

  private guardMissing(): boolean {
    if (this.state === 'closed') return true;
    const name = providerName(this.deps.agent.provider);
    this.deps.log.error('acp guard hook did not run', { threadId: this.opts.threadId, guard: this.guard });
    this.state = 'closing';
    this.proc.kill();
    this.exited(new Error(`${name} didn't run Threadbear's hook that makes it ask before using tools (a folder's .grok/config.toml can turn plugins off), so this thread was stopped before it could use any.`));
    return true;
  }

  private noteCommands(raw: unknown[]): void {
    this.cliCommands = raw.flatMap((c) => {
      if (!isPlainObject(c) || typeof c.name !== 'string' || !c.name) return [];
      return [{ name: c.name, source: 'cli' as const, ...(typeof c.description === 'string' ? { description: c.description } : {}) }];
    });
  }

  private emit(outs: AdapterOutput[]): void {
    for (const o of outs) this.sink.output(o);
  }

  private request(id: RequestId, method: string, params: unknown): void {
    if (method === 'session/request_permission' && !this.replaying) {
      const p = (isPlainObject(params) ? params : {}) as { toolCall?: AcpToolCall; options?: PermissionOption[]; _meta?: unknown };
      const call = isPlainObject(p.toolCall) ? withoutHookNote(p.toolCall, p._meta) : p.toolCall;
      // Never left unanswered: the turn would hang with session/prompt open.
      this.permission(id, { ...p, toolCall: call }).catch((e: unknown) => {
        this.deps.log.error('acp permission handler threw', { threadId: this.opts.threadId, error: String(e) });
        this.pending.delete(id);
        this.proc.respond(id, { outcome: { outcome: 'cancelled' } });
      });
      return;
    }
    // We said we offer no fs or terminal, and the extensions aren't handled
    // yet: refused at once, so nothing hangs waiting for an answer.
    this.deps.log.info('refused an ACP request', { threadId: this.opts.threadId, method });
    this.proc.respondError(id, -32601, `Threadbear doesn't handle ${method}.`);
  }

  // An option by kind: only the once kinds, as "always" would be remembered
  // by the agent, past this thread and anything Mike can see. "Always
  // allow" is kept by the runner instead (EDIT_RULE and commandRule).
  private answer(id: RequestId, options: PermissionOption[], allow: boolean): void {
    const kind = allow ? 'allow_once' : 'reject_once';
    const optionId = options.find((o) => o?.kind === kind)?.optionId;
    // No plain allow or reject on offer: nothing ever goes ahead by default.
    this.proc.respond(id, optionId ? { outcome: { outcome: 'selected', optionId } } : { outcome: { outcome: 'cancelled' } });
  }

  private async permission(id: RequestId, p: { toolCall?: AcpToolCall; options?: PermissionOption[] }): Promise<void> {
    const options = Array.isArray(p.options) ? p.options : [];
    const seen = this.normalizer.noteCall(p.toolCall ?? {});
    this.emit(this.normalizer.flush());
    this.emit(seen.events);
    // Read at the moment it's asked, not when the turn began.
    if (this.mode === 'full') return this.answer(id, options, true);
    // One of this session's own threadbear tools: its MCP server asks where
    // the tool needs it (tool-gate.ts), so a card here would be a second one.
    // Grok names the tool exactly, and a name it can't find doesn't run.
    const mcpName = p.toolCall ? this.deps.agent.mcpToolName?.(p.toolCall) : undefined;
    if (mcpName && this.ownTools.has(mcpName)) return this.answer(id, options, true);
    // A code-mode script that only calls one threadbear tool: its own gate
    // (mcp-http.ts) asks as the mode says, so no card here as well.
    const raw = p.toolCall?.rawInput;
    if (codeModeThreadbearCall(p.toolCall?.name ?? p.toolCall?.title ?? seen.title, isPlainObject(raw) && Object.keys(raw).length ? raw : seen.input)) return this.answer(id, options, true);

    const { kind } = seen;
    // What this request names and what earlier updates to the call said:
    // more names only makes the check stricter.
    const paths = [...new Set([...seen.paths, ...requestPaths(p.toolCall ?? {})].map((f) => absolutePath(f, this.opts.cwd)))];
    const input = isPlainObject(p.toolCall?.rawInput) && Object.keys(p.toolCall.rawInput).length ? p.toolCall.rawInput : seen.input;
    const command = kind === 'execute' && typeof input.command === 'string' ? input.command : '';
    const argv = command ? shellWords(command) : null;
    // A tool can name its own folder (OpenCode's workdir): the rule and the
    // card go by where the command really runs.
    // Each as the disk has it (realpath(3): a link followed before any "..",
    // as chdir does), bound to the rule that way, so `link/..` or a link
    // pointed somewhere new isn't the folder a rule was saved for. One that
    // can't be resolved, or that reads differently taken as written (as a CLI
    // that tidies the path first would use it), is somewhere unknown (Codex's
    // review, 10 Nov).
    const named = FOLDER_KEYS.map((k) => input[k]).filter((d): d is string => typeof d === 'string' && d !== '');
    const resolve = () =>
      named.map((d) => {
        const disk = realFolder(path.isAbsolute(d) ? d : `${this.opts.cwd}/${d}`);
        return disk !== undefined && disk === realFolder(absolutePath(d, this.opts.cwd)) ? disk : undefined;
      });
    const places = resolve();
    const said = [...new Set(places.filter((d): d is string => d !== undefined))];
    const unknownPlace = places.includes(undefined);
    const where = said[0] ?? (named[0] !== undefined ? absolutePath(named[0], this.opts.cwd) : (realFolder(this.opts.cwd) ?? this.opts.cwd));
    // A saved rule is for this command in this folder and nothing else. Two
    // folders that disagree, or anything else the call carries that could
    // change what runs (an env, a shell, a folder under a name not listed
    // here), and it can't be known which applies: no rule covers it, Always
    // allow isn't offered, and the card shows the lot (Daybreak, 10 Nov).
    const exact = Boolean(command) && !unknownPlace && said.length <= 1 && Object.entries(input).every(([k, v]) => k === 'command' || BENIGN_KEYS.has(k) || (FOLDER_KEYS.includes(k) && (typeof v === 'string' || v === undefined)));
    const rule = exact && argv?.length && ruleMayCover(argv) ? commandRule(argv, where) : null;

    let code: CodeWrite = false;
    if (writesFiles(kind)) {
      // An edit with no named files can't be checked: it asks every time.
      code = paths.length ? this.runsAsCode(paths) : 'unsure';
      if (!code && !isPlainEdit(input)) code = 'unsure';
      if (!code && this.rules.has(EDIT_RULE)) return this.answer(id, options, true);
    } else if (paths.length && kind !== 'read' && kind !== 'search' && kind !== 'fetch' && kind !== 'think') {
      // A kind that doesn't say what it does, naming a file that runs.
      code = this.runsAsCode(paths);
    }
    // A saved rule doesn't stand in for a file that runs as code.
    if (!code && rule && this.rules.has(rule)) return this.answer(id, options, true);

    const title = command
      ? firstLineSafely(command, 200, '…')
      : writesFiles(kind) && paths.length === 1
        ? `Edit ${displayPath(paths[0], this.opts.cwd)}`
        : firstLineSafely(seen.title || kind || 'Use a tool', 200, '…');
    // The folder as the agent gave it goes on the card too when it reads
    // differently (a link): what the card says it resolved to is checked
    // again once it's answered, below.
    const given = Object.fromEntries(FOLDER_KEYS.filter((k) => typeof input[k] === 'string' && input[k] !== where).map((k) => [k, input[k]]));
    const shown: Record<string, unknown> = command ? (exact ? { command, cwd: where, ...given } : { ...input, command, ...(FOLDER_KEYS.some((k) => k in input) ? {} : { cwd: where }) }) : writesFiles(kind) ? { paths } : input;
    const card = cardInput(shown, command ? 'all' : writesFiles(kind) ? ['paths'] : 'all');
    if (card.tooLong) {
      this.sink.output({ kind: 'event', event: { type: 'system.note', text: tooLongForCard(card.tooLong) } });
      return this.answer(id, options, false);
    }
    const editRule = writesFiles(kind) && !code && paths.length > 0;
    const pending: Pending = { abort: new AbortController(), options };
    this.pending.set(id, pending);
    let a: ToolApprovalAnswer;
    try {
      a = await this.sink.requestApproval({
        toolUseId: typeof p.toolCall?.toolCallId === 'string' ? p.toolCall.toolCallId : undefined,
        // As an MCP tool's, for anything else: a card whose arguments say
        // what it does can only be allowed where all of them are shown.
        name: command ? 'shell' : writesFiles(kind) ? 'apply_patch' : `mcp__acp__${kind ?? 'tool'}`,
        title: code ? `${title} (${codeWriteNote(code)})` : title,
        input: card.input,
        wholeInput: input,
        canAlwaysAllow: !code && (editRule || Boolean(rule)),
        signal: pending.abort.signal,
      });
    } catch (e) {
      this.deps.log.warn('approval request failed', { threadId: this.opts.threadId, error: String(e) });
      a = { decision: 'expired' };
    }
    // Already answered as cancelled: an interrupt, or the session ending.
    if (!this.pending.delete(id)) return;
    if (a.decision === 'expired') return void this.proc.respond(id, { outcome: { outcome: 'cancelled' } });
    const allow = a.decision === 'allow' || a.decision === 'allowAlways';
    // A link pointed somewhere else while the card waited: what was allowed
    // isn't where it would run now (Codex's second look, 10 Nov).
    if (allow && named.length && JSON.stringify(resolve()) !== JSON.stringify(places)) {
      this.sink.output({ kind: 'event', event: { type: 'system.note', text: `The folder for "${firstLineSafely(command, 80, '…')}" changed while it waited for an answer, so it wasn't run. The agent can ask again.` } });
      return this.answer(id, options, false);
    }
    if (a.decision === 'allowAlways' && !code) {
      if (editRule) this.addRules([EDIT_RULE]);
      else if (rule) this.addRules([rule]);
    }
    this.answer(id, options, allow);
  }

  private runsAsCode(paths: string[]): CodeWrite {
    const kinds = paths.map((f) => runsAsCodeByName(f) || (this.opts.runsAsCode?.(f) ?? false));
    return kinds.includes(true) || (kinds.includes('unsure') ? 'unsure' : false);
  }

  // Every open request answered as cancelled, which ACP requires of a
  // client cancelling a turn. Cards close with it.
  private cancelPending(respond: boolean): void {
    for (const [id, p] of this.pending) {
      p.abort.abort();
      if (respond) this.proc.respond(id, { outcome: { outcome: 'cancelled' } });
    }
    this.pending.clear();
  }

  // ---- Turns

  send(input: UserInput): void {
    if (this.state === 'closing' || this.state === 'closed') {
      this.deps.log.warn('a message reached an acp session that has ended', { threadId: this.opts.threadId });
      return;
    }
    // ACP has no way to add to a running turn: it's the next one.
    this.held.push(input);
    this.next();
  }

  private next(): void {
    if (this.state !== 'ready' || this.turn || !this.held.length) return;
    void this.runTurn(this.held.shift()!);
  }

  private promptBlocks(input: UserInput): unknown[] {
    const blocks: unknown[] = [];
    if (this.preamble) {
      blocks.push({ type: 'text', text: this.preamble });
      this.preamble = undefined;
    }
    const canSee = this.init.agentCapabilities?.promptCapabilities?.image === true;
    const parts = placePictures(input.text, input.images);
    if (!parts.length) blocks.push({ type: 'text', text: input.text });
    for (const part of parts) {
      if ('text' in part) blocks.push({ type: 'text', text: part.text });
      else blocks.push(imageBlock(part.image, canSee));
    }
    return blocks;
  }

  private async runTurn(input: UserInput): Promise<void> {
    const turn = { id: input.id, startedAt: this.now(), interrupted: false, dropped: [] as string[] };
    this.turn = turn;
    this.state = 'turn';
    let result: TurnResult;
    let dispatched = false;
    try {
      // Between turns, so the next message takes any change made during this one.
      await this.applyMode();
      const modelChanged = this.modelChanged;
      if (this.modelChanged) {
        this.modelChanged = false;
        await this.applyModel(this.model ?? this.opts.account.defaultModel ?? '');
      }
      // Again after a new model, which may start at its own level.
      if (this.effortChanged || (modelChanged && this.effort)) {
        this.effortChanged = false;
        await this.applyEffort(this.effort);
      }
      // Stop pressed during the setup above: nothing has reached the model.
      if (turn.interrupted) throw new Error('Stopped');
      this.guardSeen = false;
      dispatched = true;
      const r = await this.proc.request<{ stopReason?: string }>('session/prompt', { sessionId: this.sessionId, prompt: this.promptBlocks(input) }, null);
      this.emit(this.normalizer.closeOpen('Stopped'));
      result = this.resultFor(turn, r?.stopReason);
      const effort = this.effortNow();
      if (effort) this.sink.output({ kind: 'effort', level: effort });
    } catch (e) {
      if (e instanceof AcpExitedError) return; // the exit handler reports it
      this.emit(this.normalizer.closeOpen('Failed'));
      const auth = e instanceof AcpRpcError && e.code === AUTH_REQUIRED;
      result = { outcome: turn.interrupted ? 'interrupted' : 'failed', durationMs: Math.max(0, this.now() - turn.startedAt), ...(turn.interrupted ? {} : { error: (e as Error).message }), ...(auth ? { errorKind: 'auth' as const } : {}) };
    }
    if (this.turn !== turn) return;
    this.turn = undefined;
    if (this.state === 'turn') this.state = 'ready';
    if (this.mode === 'plan' && result.outcome === 'completed') {
      const text = this.normalizer.takeLastAssistantText();
      if (text) this.sink.output({ kind: 'planProposed', itemId: `${turn.id ?? 'plan'}-plan`, text });
    }
    const answered = [...(turn.id ? [turn.id] : []), ...turn.dropped];
    this.sink.output({ kind: 'result', result: { ...result, ...(turn.id ? { answers: turn.id } : {}), answered, unreceived: [...(!dispatched && turn.id ? [turn.id] : []), ...turn.dropped], queuedTurns: this.held.length, ...(this.normalizer.costUsd !== undefined ? { totalCostUsd: this.normalizer.costUsd } : {}) } });
    this.next();
  }

  private resultFor(turn: { startedAt: number; interrupted: boolean }, stop: string | undefined): TurnResult {
    const durationMs = Math.max(0, this.now() - turn.startedAt);
    switch (stop) {
      case 'cancelled':
        return { outcome: 'interrupted', durationMs };
      case 'refusal':
        return { outcome: 'failed', durationMs, error: 'The model refused to continue.' };
      case 'max_tokens':
        return { outcome: 'failed', durationMs, error: 'The reply reached the model\'s length limit.' };
      case 'max_turn_requests':
        return { outcome: 'failed', durationMs, error: 'The turn reached the agent\'s limit on steps.' };
      default:
        return { outcome: turn.interrupted ? 'interrupted' : 'completed', durationMs };
    }
  }

  async interrupt(): Promise<void> {
    if (this.state === 'closed' || this.state === 'closing') {
      if (!this.proc.alive) throw new Error('The agent has already exited.');
      return;
    }
    // Stop means stop: what's waiting for a later turn goes too.
    const dropped = this.held.map((m) => m.id).filter((x): x is string => Boolean(x));
    this.held = [];
    const turn = this.turn;
    if (!turn) {
      if (dropped.length) this.sink.output({ kind: 'result', result: { outcome: 'interrupted', durationMs: 0, answers: dropped[0], answered: dropped, unreceived: dropped, queuedTurns: 0 } });
      return;
    }
    // They end with this turn, so the runner sees one result, when it's over.
    turn.dropped.push(...dropped);
    turn.interrupted = true;
    this.cancelPending(true);
    this.proc.notify('session/cancel', { sessionId: this.sessionId });
  }

  // ---- Settings: from the next turn (settingsApplyNextTurn), except the
  // permission mode, which every request reads as it's asked.

  async setModel(model: string | undefined): Promise<void> {
    if (model !== this.model) this.modelChanged = true;
    this.model = model;
  }

  // Only an agent with a thought_level option takes one (applyEffort).
  async setEffort(effort: EffortLevel | undefined): Promise<void> {
    if (effort !== this.effort) this.effortChanged = true;
    this.effort = effort;
  }

  // Asked for at once, as a running turn in Cursor's writable mode wouldn't
  // send a request to stop at. One that can't be made safe doesn't go on.
  async setPermissionMode(mode: PermissionMode): Promise<void> {
    this.mode = mode;
    // A mode this folder can't run in (Grok's trusted folders): the process
    // goes, and the next message starts one, which says why it can't.
    const refused = this.deps.agent.refuses?.({ cwd: this.opts.cwd, mode, trusted: this.trusted });
    if (refused && this.state !== 'closed' && this.state !== 'closing') {
      this.sink.output({ kind: 'event', event: { type: 'system.note', text: refused } });
      this.kill();
      return;
    }
    if (!this.sessionId || (this.state !== 'ready' && this.state !== 'turn')) return;
    try {
      await this.applyMode();
    } catch (e) {
      this.deps.log.warn('could not change the agent mode', { threadId: this.opts.threadId, error: String(e) });
      if (this.turn) {
        this.sink.output({ kind: 'event', event: { type: 'system.note', text: `Stopped this turn, as ${providerName(this.deps.agent.provider)} couldn't be switched to the mode you picked.` } });
        await this.interrupt();
      }
    }
  }

  async commands(): Promise<SlashCommand[]> {
    return this.cliCommands;
  }

  // ---- Ending

  async end(opts: { hard?: boolean } = {}): Promise<void> {
    if (this.state === 'closed') return this.exitPromise;
    this.state = 'closing';
    if (opts.hard) this.proc.kill();
    await this.finish();
  }

  kill(): void {
    if (this.state === 'closed') return;
    this.state = 'closing';
    this.proc.kill();
    this.exited();
  }

  private async finish(error?: Error): Promise<void> {
    if (this.state !== 'closed') this.state = 'closing';
    this.proc.onExit = null;
    await this.proc.close().catch(() => undefined);
    this.exited(error);
  }

  private exited(error?: Error): void {
    if (this.exitedCalled) return;
    this.exitedCalled = true;
    this.state = 'closed';
    this.cancelPending(false);
    this.proc.onRequest = null;
    this.mcp?.close();
    this.emit(this.normalizer.closeOpen('Interrupted'));
    this.turn = undefined;
    this.resolveExit();
    this.sink.exited(error);
  }
}

// A picture for the prompt: the image itself where the agent says it takes
// them, else a link to the saved file, which every ACP agent must accept.
function imageBlock(image: ImageRef, canSee: boolean): unknown {
  if (canSee) {
    try {
      return { type: 'image', data: fs.readFileSync(image.path).toString('base64'), mimeType: image.mediaType };
    } catch {
      // Gone from disk: the link says so when the agent tries it.
    }
  }
  return { type: 'resource_link', uri: `file://${image.path}`, name: path.basename(image.path), mimeType: image.mediaType };
}
