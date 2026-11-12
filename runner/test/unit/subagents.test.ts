import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { subagentEvents, subagentModel } from '../../src/adapters/claude/subagents.ts';
import { fileContext } from '../../src/adapters/claude/normalize.ts';
import { subagentTranscriptPath } from '../../src/transcripts.ts';
import { checkEvent } from '../helpers/protocol-check.ts';
import { makeHarness, tmpDir } from '../helpers/fakes.ts';
import { RpcError } from '../../src/protocol.ts';

const FIX = path.join(import.meta.dirname, '..', 'fixtures');
const TRANSCRIPT = path.join(FIX, 'subagent-transcript.jsonl');

function ctx() {
  const dir = tmpDir('tb-sub-');
  return fileContext('/work/proj', path.join(dir, 'media'), path.join(dir, 'blobs'));
}

test('a sub-agent transcript reads as its brief, then its tool calls and reply (recorded)', () => {
  const evs = subagentEvents(fs.readFileSync(TRANSCRIPT, 'utf8'), ctx()).map((e) => e.body as any);
  assert.equal(evs[0].type, 'user.message');
  assert.match(evs[0].text, /ls/);
  const types = evs.map((e) => e.type);
  assert.ok(types.includes('tool.call'));
  const call = evs.find((e) => e.type === 'tool.call');
  assert.equal(call.name, 'Bash');
  const result = evs.find((e) => e.type === 'tool.result' && e.itemId === call.itemId);
  assert.ok(result, 'the call has its result');
  assert.equal(evs.at(-1).type, 'assistant.text');
  for (const [i, body] of evs.entries()) assert.deepEqual(checkEvent({ seq: i + 1, ts: '2026-10-27T00:00:00.000Z', threadId: 't', ...body }), []);
});

test('a half-written last line and lines it does not know are skipped', () => {
  const text = [
    JSON.stringify({ type: 'user', message: { role: 'user', content: 'Do the thing' }, timestamp: '2026-10-27T01:00:00.000Z' }),
    JSON.stringify({ type: 'attachment', attachment: { type: 'instructions' } }),
    JSON.stringify({ type: 'user', isMeta: true, message: { role: 'user', content: 'meta' } }),
    JSON.stringify({ type: 'assistant', message: { id: 'm1', content: [{ type: 'text', text: 'On it' }] } }),
    '{"type":"assistant","message":{"id":"m2","con',
  ].join('\n');
  const evs = subagentEvents(text, ctx());
  assert.deepEqual(evs.map((e) => e.body.type), ['user.message', 'assistant.text']);
  assert.equal(evs[0].ts, '2026-10-27T01:00:00.000Z');
});

test('the transcript is found beside the session, under the folder\'s real path', () => {
  const config = tmpDir('tb-cfg-');
  const cwd = fs.realpathSync(tmpDir('tb-cwd-'));
  const dir = path.join(config, 'projects', cwd.replace(/[^a-zA-Z0-9]/g, '-'), 'sess-1', 'subagents');
  fs.mkdirSync(dir, { recursive: true });
  fs.writeFileSync(path.join(dir, 'agent-abc123.jsonl'), '');
  assert.equal(subagentTranscriptPath(config, cwd, 'sess-1', 'abc123'), path.join(dir, 'agent-abc123.jsonl'));
  assert.equal(subagentTranscriptPath(config, cwd, 'sess-1', 'nope'), null);
  assert.equal(subagentTranscriptPath(config, cwd, '../x', 'abc123'), null);
  assert.equal(subagentTranscriptPath(config, cwd, 'sess-1', '../abc123'), null);
});

function create(h: ReturnType<typeof makeHarness>) {
  return h.runner.create({ text: 'Fan out', accountId: 'claude-3' });
}

test('the summary lists sub-agents as they start, work and finish', () => {
  const h = makeHarness();
  const { thread } = create(h);
  const s = h.adapter.last;
  s.init();
  s.out({ kind: 'subagent', update: { id: 'a1', started: true, status: 'running', background: false, itemId: 'toolu_1', description: 'Look around', agentType: 'Explore' } });
  let sum = h.runner.get(thread.id);
  assert.equal(sum.subagents?.length, 1);
  const a = sum.subagents![0];
  assert.deepEqual({ ...a, startedAt: 'x' }, { id: 'a1', itemId: 'toolu_1', description: 'Look around', agentType: 'Explore', status: 'running', background: false, startedAt: 'x' });
  assert.ok(!('sessionId' in a) && !('cwd' in a) && !('accountId' in a), 'where the transcript is stays inside the runner');
  s.out({ kind: 'subagent', update: { id: 'a1', activity: 'Running grep', toolUses: 2 } });
  assert.equal(h.runner.get(thread.id).subagents![0].activity, 'Running grep');
  h.clock.advance(5000);
  s.out({ kind: 'subagent', update: { id: 'a1', status: 'completed', summary: 'Found it' } });
  sum = h.runner.get(thread.id);
  const done = sum.subagents![0];
  assert.equal(done.status, 'completed');
  assert.equal(done.summary, 'Found it');
  assert.equal(done.activity, undefined);
  assert.equal(done.toolUses, 2);
  assert.equal(Date.parse(done.endedAt!) - Date.parse(done.startedAt), 5000);
  // thread.updated carried each change.
  const updates = h.notes.filter((n: any) => n.method === 'thread.updated' && n.params.thread.subagents);
  assert.ok(updates.length >= 3);
});

test('only the newest 20 sub-agents are kept, finished ones going first', () => {
  const h = makeHarness();
  const { thread } = create(h);
  const s = h.adapter.last;
  s.init();
  s.out({ kind: 'subagent', update: { id: 'long', started: true, status: 'running', background: true, description: 'Long one' } });
  for (let i = 0; i < 25; i++) {
    s.out({ kind: 'subagent', update: { id: `q${i}`, started: true, status: 'running', background: false, description: `Quick ${i}` } });
    s.out({ kind: 'subagent', update: { id: `q${i}`, status: 'completed' } });
  }
  const ids = h.runner.get(thread.id).subagents!.map((a) => a.id);
  assert.equal(ids.length, 20);
  assert.equal(ids[0], 'long', 'still running, so kept');
  assert.equal(ids.at(-1), 'q24');
});

test('running sub-agents are stopped when their CLI goes, and after a runner restart', () => {
  const h = makeHarness();
  const { thread } = create(h);
  const s = h.adapter.last;
  s.init();
  s.out({ kind: 'subagent', update: { id: 'a1', started: true, status: 'running', background: true, description: 'Background' } });
  s.sink.exited(new Error('Claude Code process exited with code 1'));
  const a = h.runner.get(thread.id).subagents![0];
  assert.equal(a.status, 'stopped');
  assert.ok(a.endedAt);

  const h2 = makeHarness();
  const t2 = create(h2).thread;
  h2.adapter.last.init();
  h2.adapter.last.out({ kind: 'subagent', update: { id: 'b1', started: true, status: 'running', background: true, description: 'Mid-run' } });
  h2.runner.flush();
  // The runner goes down without stopping anything (a crash).
  const again = h2.restart();
  assert.equal(again.runner.get(t2.id).subagents![0].status, 'stopped');
});

test('threads.subagent reads the transcript where the sub-agent ran', () => {
  const seen: unknown[][] = [];
  const h = makeHarness(undefined, {}, {
    subagentTranscript: (...args) => {
      seen.push(args);
      return args[3] === 'a1' ? TRANSCRIPT : null;
    },
  });
  const { thread } = create(h);
  const s = h.adapter.last;
  s.init();
  s.out({ kind: 'subagent', update: { id: 'a1', started: true, status: 'running', background: false, description: 'List files' } });
  const r = h.runner.subagent(thread.id, 'a1');
  assert.equal(r.subagent?.id, 'a1');
  // Reading it fills in the model and effort, for the sub-agent's header.
  assert.deepEqual([r.subagent?.model, r.subagent?.effort], ['claude-haiku-5-5', 'medium']);
  h.runner.flush();
  assert.equal(h.runner.get(thread.id).subagents![0].effort, 'medium');
  assert.ok(r.events.length > 2);
  assert.deepEqual(r.events.map((e) => e.seq), r.events.map((_, i) => i + 1));
  assert.ok(r.events.every((e) => e.threadId === thread.id && e.turnId === undefined));
  const rec = h.store.loadAll().records.find((x) => x.id === thread.id)!;
  assert.deepEqual(seen[0].slice(1), [rec.cwd, rec.nativeSessionId, 'a1']);
  assert.throws(() => h.runner.subagent(thread.id, 'zz'), (e: unknown) => e instanceof RpcError && e.code === -32006);
});

test("a sub-agent transcript says the model and effort its latest reply ran at", () => {
  assert.deepEqual(subagentModel(fs.readFileSync(TRANSCRIPT, 'utf8')), { model: 'claude-haiku-5-5', effort: 'medium' });
  const line = (model: string, effort?: string) => JSON.stringify({ type: 'assistant', message: { model, content: [] }, ...(effort ? { effort } : {}) });
  assert.deepEqual(subagentModel([line('claude-haiku-5-5', 'low'), line('claude-opus-5-5', 'high'), line('<synthetic>'), '{"type":"assis'].join('\n')), { model: 'claude-opus-5-5', effort: 'high' });
  assert.deepEqual(subagentModel(line('claude-opus-5-5')), { model: 'claude-opus-5-5' });
  assert.deepEqual(subagentModel(''), {});
});
