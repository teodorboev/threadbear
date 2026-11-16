// Nested threads, the runner's half (experiment nestedThreads,
// runner/nested.ts; docs/protocol.md, "Nested threads"): which threads
// nest, and a starter hearing from its threads once, at the end.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { makeHarness, tmpDir } from '../helpers/fakes.ts';
import { PATIENCE_MS } from '../helpers/patience.ts';
import { HELD_HEAD, senderPrefix } from '../../src/runner/nested.ts';

const END_TURN = 'mcp__threadbear__end_turn';

async function until(check: () => boolean, what: string): Promise<void> {
  const end = Date.now() + PATIENCE_MS;
  while (!check()) {
    if (Date.now() > end) throw new Error(`Timed out waiting for ${what}`);
    await new Promise((r) => setTimeout(r, 10));
  }
}

// Long enough for a release that was going to happen (setImmediate) to have.
const settle = () => new Promise((r) => setTimeout(r, 150));

function setup(experiments: Record<string, boolean> = { nestedThreads: true }) {
  const home = tmpDir();
  fs.writeFileSync(path.join(home, 'runner.json'), JSON.stringify({ experiments }));
  return makeHarness(home);
}

type H = ReturnType<typeof setup>;

const session = (h: H, id: string) => h.adapter.sessions.filter((x) => x.opts.threadId === id).at(-1)!;

function finish(h: H, id: string, outcome: 'completed' | 'failed' = 'completed') {
  const s = session(h, id);
  s.init();
  if (outcome === 'completed') s.out({ kind: 'event', event: { type: 'tool.call', itemId: `e${Math.random()}`, name: END_TURN, kind: 'mcp', title: 'end_turn', input: { needs: 'nothing' } } });
  s.text(`m${Math.random()}`, 'Done.');
  s.result(outcome);
}

const mine = (h: H) => h.runner.create({ text: 'Port the app', accountId: 'claude-main' }).thread.id;
const child = (h: H, parent: string, detached = false) =>
  h.runner.create({ text: 'Build the pane', accountId: 'claude-main', startedBy: { kind: 'thread', label: 'Agent', threadId: parent }, ...(detached ? { detached: true } : {}) }).thread.id;
// What send_message sends: the sender's line, then the report.
const report = (h: H, from: string, to: string, words: string) =>
  h.runner.send(to, `[Sent by the agent in the thread "${h.runner.get(from).title}" (${from.slice(0, 8)})]\n\n${words}`);
// The thread's record, which keeps what the summary doesn't show.
const record = (h: H, id: string) => (h.runner as unknown as { threads: Map<string, { rec: { heldReports?: unknown[] } }> }).threads.get(id)!.rec;
const said = (h: H, id: string) => h.events(id).filter((e) => e.type === 'user.message').map((e) => String(e.text));

// A starter that has finished its own turn, with two threads working.
async function family(h: H) {
  const parent = mine(h);
  finish(h, parent);
  await until(() => h.runner.get(parent).status === 'idle', 'the starter idle');
  const a = child(h, parent);
  const b = child(h, parent);
  session(h, a).init();
  session(h, b).init();
  return { parent, a, b };
}

test('nestedThreads: the sender is read from the line send_message starts with', () => {
  assert.equal(senderPrefix('[Sent by the agent in the thread "Fix (the) pane" (0a1b2c3d)]\n\nDone.'), '0a1b2c3d');
  assert.equal(senderPrefix('Done (0a1b2c3d)]'), undefined);
  assert.equal(senderPrefix('[Sent by the agent in the thread "x" (zzzz)]\n\n'), undefined);
});

test('nestedThreads: a thread can start detached, and be moved out from under its starter and back', () => {
  const h = setup();
  const parent = mine(h);
  const kept = child(h, parent);
  const apart = child(h, parent, true);
  assert.equal(h.runner.get(kept).detached, undefined);
  assert.equal(h.runner.get(apart).detached, true);
  assert.equal(h.runner.update({ threadId: kept, detached: true }).thread.detached, true);
  assert.equal(h.runner.update({ threadId: apart, detached: false }).thread.detached, undefined);
  // Mike's own threads have no starter to be under.
  assert.equal(h.runner.update({ threadId: parent, detached: true }).thread.detached, undefined);
  const own = h.runner.create({ text: 'x', accountId: 'claude-main', detached: true }).thread.id;
  assert.equal(h.runner.get(own).detached, undefined);
});

test('nestedThreads: reports wait until the threads it started have all finished, then come as one message', async () => {
  const h = setup();
  const { parent, a, b } = await family(h);
  const before = said(h, parent).length;
  assert.equal(report(h, a, parent, 'The pane lists pages.').held, true);
  finish(h, a);
  await settle();
  assert.equal(said(h, parent).length, before, 'b is still working, so nothing yet');
  assert.equal(h.runner.get(parent).status, 'idle');
  assert.equal(report(h, b, parent, 'Sign-in handoff works.').held, true);
  finish(h, b);
  await until(() => said(h, parent).length > before, 'the reports delivered');
  const got = said(h, parent).slice(before);
  assert.equal(got.length, 1, 'one message, one wake');
  assert.ok(got[0].startsWith(HELD_HEAD));
  assert.match(got[0], /The pane lists pages\./);
  assert.match(got[0], /Sign-in handoff works\./);
});

test('nestedThreads: one that fails sends what was held straight away', async () => {
  const h = setup();
  const { parent, a, b } = await family(h);
  const before = said(h, parent).length;
  assert.equal(report(h, a, parent, 'Halfway there.').held, true);
  await settle();
  assert.equal(said(h, parent).length, before, 'held while both work');
  finish(h, b, 'failed');
  await until(() => said(h, parent).length > before, 'delivered on a failure');
  assert.match(said(h, parent).at(-1)!, /Halfway there\./);
});

test("nestedThreads: Mike writing to the starter takes the held reports in first", async () => {
  const h = setup();
  const { parent, a } = await family(h);
  assert.equal(report(h, a, parent, 'Halfway there.').held, true);
  h.runner.send(parent, 'How is it going?');
  const texts = said(h, parent);
  const reportAt = texts.findIndex((t) => /Halfway there\./.test(t));
  const mikeAt = texts.findIndex((t) => t === 'How is it going?');
  assert.ok(reportAt >= 0 && mikeAt > reportAt, 'the reports, then his message');
  assert.equal(record(h, parent).heldReports, undefined, 'nothing left held');
});

test('nestedThreads: off, detached, or a starter in a turn: sent as before', async () => {
  const off = setup({});
  const f = await family(off);
  assert.equal(report(off, f.a, f.parent, 'x').held, undefined);

  const h = setup();
  const parent = mine(h);
  finish(h, parent);
  await until(() => h.runner.get(parent).status === 'idle', 'idle');
  const apart = child(h, parent, true);
  const kept = child(h, parent);
  session(h, apart).init();
  session(h, kept).init();
  assert.equal(report(h, apart, parent, 'Unrelated news.').held, undefined, 'a detached thread is its own');

  const busy = setup();
  const p2 = mine(busy);
  const c2 = child(busy, p2);
  session(busy, c2).init();
  // The starter's first turn is still going: it reads the report in it.
  assert.equal(report(busy, c2, p2, 'Early news.').held, undefined);
});

test('nestedThreads: a starter waiting on its threads wakes once, with their replies and the held reports', async () => {
  const h = setup();
  const parent = mine(h);
  const a = child(h, parent);
  session(h, a).init();
  const s = session(h, parent);
  s.init();
  s.out({ kind: 'event', event: { type: 'tool.call', itemId: 'w', name: END_TURN, kind: 'mcp', title: 'end_turn', input: { needs: 'nothing', waitFor: [a] } } });
  s.result('completed');
  await until(() => Boolean(h.runner.get(parent).snoozedUntil), 'the starter hibernating');
  const before = said(h, parent).length;
  report(h, a, parent, 'Found the bug.');
  finish(h, a);
  await until(() => said(h, parent).length > before, 'the wake');
  await settle();
  const got = said(h, parent).slice(before);
  assert.equal(got.length, 1, 'one wake, not two');
  assert.match(got[0], /you hibernated to wait on/);
  assert.match(got[0], /Found the bug\./);
});

test('nestedThreads: reports held across a restart go once the threads have finished', async () => {
  const h = setup();
  const { parent, a, b } = await family(h);
  finish(h, a);
  finish(h, b);
  await settle();
  // As a runner coming back finds it: a report on the record, held when it
  // went down, and the threads it waited on finished since.
  const rec = record(h, parent);
  rec.heldReports = [{ from: a, text: `[Sent by the agent in the thread "x" (${a.slice(0, 8)})]\n\nKept over a restart.`, at: new Date().toISOString() }];
  const before = said(h, parent).length;
  h.runner.settleAllHeld();
  await until(() => said(h, parent).length > before, 'delivered at the start');
  assert.match(said(h, parent).at(-1)!, /Kept over a restart\./);
  assert.equal(rec.heldReports, undefined);
});

test("nestedThreads: create_thread's nested: false starts it detached, and a held send_message says so", async () => {
  const { buildThreadbearTools } = await import('../../src/threadbear-mcp.ts');
  const h = setup();
  const parent = mine(h);
  finish(h, parent);
  await until(() => h.runner.get(parent).status === 'idle', 'the starter idle');
  const call = async (selfId: string, name: string, args: Record<string, unknown>) => {
    const t = buildThreadbearTools(h.runner, selfId, () => Date.now()).find((x) => x.name === name)!;
    const r = (await t.handler(args as never, {})) as { content: { text: string }[]; isError?: boolean };
    return r.content.map((c) => c.text).join('\n');
  };
  await call(parent, 'create_thread', { text: 'Fix the sleep timer in Poddle', folder: '.', nested: false });
  await call(parent, 'create_thread', { text: 'Build the pane', folder: '.' });
  const apart = h.runner.list().find((x) => x.title === 'Fix the sleep timer in Poddle')!;
  const kept = h.runner.list().find((x) => x.title === 'Build the pane')!;
  assert.equal(apart.detached, true);
  assert.equal(kept.detached, undefined);
  session(h, kept.id).init();
  const reply = await call(kept.id, 'send_message', { threadId: parent, text: 'The pane works.' });
  assert.match(reply, /^Held for /);
  assert.equal(h.runner.get(parent).status, 'idle');
});
