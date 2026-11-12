// Reads what a sub-agent did from the transcript the CLI writes for it, as
// protocol events, for threads.subagent. The CLI appends to the file as the
// sub-agent works, so reading it again picks up where it has got to. The
// file's layout isn't a documented contract: lines this doesn't know are
// skipped.
import fs from 'node:fs';
import type { EventBody } from '../../protocol.ts';
import { ClaudeNormalizer, realModel, type NormalizerContext } from './normalize.ts';

export type TranscriptEvent = { ts?: string; body: EventBody };

type AnyMsg = Record<string, any>;

export type SubagentRead = { events: TranscriptEvent[]; model?: string; effort?: string };

export function readSubagentTranscript(file: string, ctx: NormalizerContext): SubagentRead {
  const text = fs.readFileSync(file, 'utf8');
  return { events: subagentEvents(text, ctx), ...subagentModel(text) };
}

// The model and effort level its latest reply ran at. Each assistant line
// has `message.model` and the CLI's own `effort`; the last one wins, as a
// fallback model can take over partway.
export function subagentModel(text: string): { model?: string; effort?: string } {
  const lines = text.split('\n');
  for (let i = lines.length - 1; i >= 0; i--) {
    if (!lines[i].includes('"assistant"')) continue;
    let d: AnyMsg;
    try {
      d = JSON.parse(lines[i]);
    } catch {
      continue;
    }
    const model = d.type === 'assistant' ? realModel(d.message?.model) : undefined;
    if (!model) continue;
    return typeof d.effort === 'string' && d.effort ? { model, effort: d.effort } : { model };
  }
  return {};
}

export function subagentEvents(text: string, ctx: NormalizerContext): TranscriptEvent[] {
  const norm = new ClaudeNormalizer(ctx);
  const out: TranscriptEvent[] = [];
  for (const line of text.split('\n')) {
    if (!line.trim()) continue;
    let d: AnyMsg;
    try {
      d = JSON.parse(line);
    } catch {
      continue; // a line the CLI is still writing
    }
    if ((d.type !== 'user' && d.type !== 'assistant') || !d.message || d.isMeta) continue;
    const ts = typeof d.timestamp === 'string' ? d.timestamp : undefined;
    if (d.type === 'user') {
      // Its brief, and anything else said to it, are plain user turns; tool
      // results go through the normaliser like a thread's.
      const text = promptText(d.message.content);
      if (text) {
        out.push({ ts, body: { type: 'user.message', text, images: [] } });
        continue;
      }
    }
    // The transcript calls the result's details toolUseResult; the SDK's
    // messages call them tool_use_result.
    const msg: AnyMsg = { type: d.type, uuid: d.uuid, message: d.message };
    if (d.toolUseResult !== undefined) msg.tool_use_result = d.toolUseResult;
    for (const o of norm.handle(msg)) if (o.kind === 'event') out.push({ ts, body: o.event });
  }
  return out;
}

// The text of a user message that isn't tool results.
function promptText(content: unknown): string {
  if (typeof content === 'string') return content;
  if (!Array.isArray(content) || content.some((c: AnyMsg) => c?.type === 'tool_result')) return '';
  return (content as AnyMsg[])
    .filter((c) => c?.type === 'text' && typeof c.text === 'string')
    .map((c) => c.text)
    .join('\n');
}
