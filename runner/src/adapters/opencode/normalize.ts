// Turns `opencode acp` session/update notifications into protocol events.
// Pure apart from the injected file helpers (NormalizerContext), so it can be
// tested by feeding recorded ACP lines through it, like the other two.
//
// ACP streams text as chunks tagged with a messageId and has no "item
// finished" for them, so a text is closed (its whole text sent as one
// event after the deltas) when something else starts: another message,
// a tool call, or the end of the turn (finishTurn).
//
// Tools: OpenCode's names map onto Claude's (shell is Bash, read is Read,
// ...), with their arguments renamed to match, so the app shows them the
// way it shows Claude's. The threadbear MCP server's tools come through as
// threadbear_<tool> and go out as mcp__threadbear__<tool>.
import type { EventBody, PlanEntry, ToolKind } from '../../protocol.ts';
import type { AdapterOutput } from '../types.ts';
import { truncateResultText, type NormalizerContext } from '../context.ts';
import { diffLines } from '../../diff.ts';
import { displayPath, toolKind, toolTitle, truncateInput } from '../../tools.ts';
import { SERVER_NAME } from '../../threadbear-mcp.ts';

type AnyMsg = Record<string, any>;

// OpenCode tool name -> Claude's, and how its arguments are renamed.
const TOOL_NAMES: Record<string, string> = {
  shell: 'Bash',
  bash: 'Bash',
  read: 'Read',
  write: 'Write',
  edit: 'Edit',
  multiedit: 'MultiEdit',
  patch: 'Edit',
  apply_patch: 'Edit',
  grep: 'Grep',
  glob: 'Glob',
  list: 'LS',
  webfetch: 'WebFetch',
  websearch: 'WebSearch',
  todowrite: 'TodoWrite',
  todoread: 'TodoWrite',
  task: 'Task',
};
const ARG_NAMES: Record<string, string> = {
  path: 'file_path',
  filePath: 'file_path',
  oldString: 'old_string',
  newString: 'new_string',
  replaceAll: 'replace_all',
};
const MCP_PREFIX = `${SERVER_NAME}_`;

// ACP's kind when OpenCode's name isn't one this knows.
const ACP_KINDS: Record<string, ToolKind> = {
  read: 'read',
  edit: 'edit',
  delete: 'edit',
  move: 'edit',
  search: 'search',
  execute: 'command',
  fetch: 'web',
  think: 'other',
  switch_mode: 'other',
  other: 'other',
};

export type ToolName = { name: string; kind: ToolKind };

// The name and kind a tool call goes out under.
export function toolName(raw: string, acpKind?: string): ToolName {
  if (raw.startsWith(MCP_PREFIX)) return { name: `mcp__${SERVER_NAME}__${raw.slice(MCP_PREFIX.length)}`, kind: 'mcp' };
  const mapped = TOOL_NAMES[raw];
  if (mapped) return { name: mapped, kind: toolKind(mapped) === 'other' && acpKind ? (ACP_KINDS[acpKind] ?? 'other') : toolKind(mapped) };
  // Another MCP server from the user's own opencode.json (server_tool).
  if (/^[\w-]+_[\w-]+/.test(raw) && (!acpKind || acpKind === 'other')) return { name: raw, kind: 'mcp' };
  return { name: raw || 'tool', kind: (acpKind && ACP_KINDS[acpKind]) || 'other' };
}

export function toolInput(raw: unknown): Record<string, unknown> {
  if (!raw || typeof raw !== 'object' || Array.isArray(raw)) return {};
  const out: Record<string, unknown> = {};
  for (const [k, v] of Object.entries(raw as Record<string, unknown>)) out[ARG_NAMES[k] ?? k] = v;
  return out;
}

type OpenText = { type: 'assistant.text' | 'reasoning'; itemId: string; text: string };
type Call = { id: string; raw: string; acpKind?: string; title?: string; input: Record<string, unknown>; sent: boolean; done: boolean };

const ev = (event: EventBody): AdapterOutput => ({ kind: 'event', event });

export class OpencodeNormalizer {
  private ctx: NormalizerContext;
  private open?: OpenText;
  private calls = new Map<string, Call>();
  // The last assistant message of the turn, for "Plan first".
  lastText?: { itemId: string; text: string };

  constructor(ctx: NormalizerContext) {
    this.ctx = ctx;
  }

  // One session/update's `update`.
  update(u: AnyMsg): AdapterOutput[] {
    const out: AdapterOutput[] = [];
    switch (u?.sessionUpdate) {
      case 'agent_message_chunk':
        this.chunk(out, 'assistant.text', u);
        break;
      case 'agent_thought_chunk':
        this.chunk(out, 'reasoning', u);
        break;
      case 'tool_call':
        this.closeText(out);
        this.toolCall(out, u);
        break;
      case 'tool_call_update':
        this.closeText(out);
        this.toolUpdate(out, u);
        break;
      case 'plan': {
        const entries = (Array.isArray(u.entries) ? u.entries : [])
          .map((e: AnyMsg): PlanEntry | null => {
            const text = typeof e?.content === 'string' ? e.content : '';
            if (!text) return null;
            const status = e.status === 'completed' || e.status === 'in_progress' ? e.status : 'pending';
            return { text, status };
          })
          .filter((e: PlanEntry | null): e is PlanEntry => e !== null);
        out.push(ev({ type: 'plan.updated', entries }));
        break;
      }
      case 'usage_update': {
        const used = Number(u.used);
        const size = Number(u.size);
        if (Number.isFinite(used) && used > 0) out.push({ kind: 'context', contextTokens: used, ...(Number.isFinite(size) && size > 0 ? { contextWindow: size } : {}) });
        break;
      }
      // Replayed history (session/load), the user's own text echoed, and
      // what the runner reads elsewhere (commands, modes, config).
      default:
        break;
    }
    return out;
  }

  // A permission card for a call: makes sure the call's row is out first,
  // so the card has something to sit under, and says what to show on it.
  forApproval(tc: AnyMsg): { out: AdapterOutput[]; itemId?: string; name: string; title: string; input: Record<string, unknown>; paths: string[] } {
    const out: AdapterOutput[] = [];
    this.closeText(out);
    const id = typeof tc?.toolCallId === 'string' ? tc.toolCallId : undefined;
    const call = id ? this.merge(id, tc) : undefined;
    if (call) this.send(out, call);
    const raw = call?.raw ?? String(tc?.name ?? tc?.title ?? 'tool');
    const { name } = toolName(raw, call?.acpKind ?? tc?.kind);
    const input = call?.input ?? toolInput(tc?.rawInput);
    const paths = [str(input.file_path), ...(Array.isArray(tc?.locations) ? tc.locations.map((l: AnyMsg) => str(l?.path)) : [])].filter(Boolean);
    return { out, itemId: call?.id, name, title: this.title(name, input, call?.title), input: truncateInput(input), paths };
  }

  // The turn ended: close any open text, and finish calls left running.
  finishTurn(): AdapterOutput[] {
    const out: AdapterOutput[] = [];
    this.closeText(out);
    for (const c of this.calls.values()) {
      if (c.sent && !c.done) out.push(ev({ type: 'tool.result', itemId: c.id, ok: false, text: 'Stopped before it finished.', truncated: false, images: [] }));
    }
    this.calls.clear();
    return out;
  }

  // Starts a new turn's record of its last message.
  startTurn(): void {
    this.lastText = undefined;
  }

  private chunk(out: AdapterOutput[], type: OpenText['type'], u: AnyMsg): void {
    const text = u?.content?.type === 'text' && typeof u.content.text === 'string' ? u.content.text : '';
    if (!text) return;
    const itemId = typeof u.messageId === 'string' && u.messageId ? u.messageId : `${type}:${Date.now()}`;
    if (this.open && (this.open.itemId !== itemId || this.open.type !== type)) this.closeText(out);
    if (!this.open) this.open = { type, itemId, text: '' };
    this.open.text += text;
    out.push({ kind: 'delta', delta: { type: type === 'assistant.text' ? 'assistant.text.delta' : 'reasoning.delta', itemId, text } });
  }

  private closeText(out: AdapterOutput[]): void {
    const t = this.open;
    this.open = undefined;
    if (!t || !t.text.trim()) return;
    if (t.type === 'assistant.text') {
      out.push(ev({ type: 'assistant.text', itemId: t.itemId, text: t.text }));
      this.lastText = { itemId: t.itemId, text: t.text };
    } else out.push(ev({ type: 'reasoning', itemId: t.itemId, text: t.text }));
  }

  private merge(id: string, u: AnyMsg): Call {
    let c = this.calls.get(id);
    if (!c) this.calls.set(id, (c = { id, raw: '', input: {}, sent: false, done: false }));
    if (typeof u.name === 'string' && u.name && !c.raw) c.raw = u.name;
    if (typeof u.kind === 'string') c.acpKind = u.kind;
    if (typeof u.title === 'string' && u.title && u.title !== c.raw) c.title = u.title;
    const input = toolInput(u.rawInput);
    if (Object.keys(input).length) c.input = { ...c.input, ...input };
    if (!c.raw) c.raw = typeof u.title === 'string' ? u.title : 'tool';
    return c;
  }

  // OpenCode first says a call is pending with next to no arguments, then
  // fills them in: its row goes out once the call runs (or asks first).
  private toolCall(out: AdapterOutput[], u: AnyMsg): void {
    const id = typeof u.toolCallId === 'string' ? u.toolCallId : undefined;
    if (!id) return;
    const c = this.merge(id, u);
    if (u.status && u.status !== 'pending') this.send(out, c);
    if (u.status === 'completed' || u.status === 'failed') this.result(out, c, u);
  }

  private toolUpdate(out: AdapterOutput[], u: AnyMsg): void {
    const id = typeof u.toolCallId === 'string' ? u.toolCallId : undefined;
    if (!id) return;
    const c = this.merge(id, u);
    this.send(out, c);
    if (u.status === 'completed' || u.status === 'failed') this.result(out, c, u);
  }

  private send(out: AdapterOutput[], c: Call): void {
    if (c.sent) return;
    c.sent = true;
    const own = codeModeCall(c.raw, c.input);
    if (own) {
      out.push(ev({ type: 'tool.call', itemId: c.id, name: own.name, kind: 'mcp', title: this.title(own.name, own.input), input: truncateInput(own.input) }));
      return;
    }
    const { name, kind } = toolName(c.raw, c.acpKind);
    out.push(ev({ type: 'tool.call', itemId: c.id, name, kind, title: this.title(name, c.input, c.title), input: truncateInput(c.input) }));
  }

  private title(name: string, input: Record<string, unknown>, acpTitle?: string): string {
    const own = toolTitle(name, input, this.ctx.cwd);
    // toolTitle falls back to the bare name for tools it doesn't know;
    // OpenCode's own title says more then.
    if (acpTitle && (own === name || !own)) return displayPath(acpTitle, this.ctx.cwd);
    return own || acpTitle || name;
  }

  private result(out: AdapterOutput[], c: Call, u: AnyMsg): void {
    if (c.done) return;
    c.done = true;
    const texts: string[] = [];
    for (const part of Array.isArray(u.content) ? u.content : []) {
      if (part?.type === 'content' && part.content?.type === 'text' && typeof part.content.text === 'string') texts.push(part.content.text);
      else if (part?.type === 'diff' && typeof part.path === 'string') {
        const before = typeof part.oldText === 'string' ? part.oldText : '';
        const after = typeof part.newText === 'string' ? part.newText : '';
        out.push(ev({ type: 'edit.diff', itemId: c.id, path: part.path, hunks: diffLines(before, after) }));
      }
    }
    const meta = u.rawOutput && typeof u.rawOutput === 'object' ? (u.rawOutput as AnyMsg) : {};
    if (!texts.length && typeof meta.output === 'string') texts.push(meta.output);
    if (!texts.length && typeof meta.error === 'string') texts.push(meta.error);
    const exit = meta.metadata?.exit;
    const ok = u.status === 'completed' && (exit === undefined || exit === null || exit === 0);
    const text = texts.join('\n');
    out.push(ev({ type: 'tool.result', itemId: c.id, ok, ...truncateResultText(text === '(no output)' ? '' : text, c.id, this.ctx), images: [] }));
  }
}

// OpenCode 2 hands MCP tools to the model behind its code-mode `execute`
// tool, so a threadbear tool call arrives as a little script. One that only
// calls one threadbear tool goes out as that tool, so it shows (and
// end_turn is found) by its own name; its arguments come along when
// they're written as JSON, else the script does.
const CODE_CALL = new RegExp(`^\\s*(?:(?:const|let|var)\\s+\\w+\\s*=\\s*)?(?:return\\s+)?(?:await\\s+)?tools\\.${SERVER_NAME}\\.(\\w+)\\(([\\s\\S]*)\\)\\s*;?\\s*(?:return\\s+\\w+\\s*;?\\s*)?$`);

export function codeModeCall(raw: string, input: Record<string, unknown>): { name: string; input: Record<string, unknown> } | undefined {
  if (raw !== 'execute' || typeof input.code !== 'string') return undefined;
  const m = CODE_CALL.exec(input.code);
  if (!m || input.code.split(`tools.${SERVER_NAME}.`).length !== 2) return undefined;
  let args: Record<string, unknown> = { code: input.code };
  try {
    const parsed = JSON.parse(m[2].trim() || '{}');
    if (parsed && typeof parsed === 'object' && !Array.isArray(parsed)) args = parsed as Record<string, unknown>;
  } catch {
    // written as JavaScript, not JSON: keep the script
  }
  return { name: `mcp__${SERVER_NAME}__${m[1]}`, input: args };
}

function str(v: unknown): string {
  return typeof v === 'string' ? v : '';
}
