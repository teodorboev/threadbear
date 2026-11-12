// Threads with nothing for Mike keep out of his inbox (experiment
// quietInbox, runner/quiet-inbox.ts; docs/protocol.md, "A quiet inbox").
import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { makeHarness, tmpDir } from '../helpers/fakes.ts';
import { PATIENCE_MS } from '../helpers/patience.ts';

const END_TURN = 'mcp__threadbear__end_turn';

async function until(check: () => boolean, what: string): Promise<void> {
  const end = Date.now() + PATIENCE_MS;
  while (!check()) {
    if (Date.now() > end) throw new Error(`Timed out waiting for ${what}`);
    await new Promise((r) => setTimeout(r, 10));
  }
}

// Long enough for a check that was going to act to have acted.
const settle = () => new Promise((r) => setTimeout(r, 150));

function setup(experiments: Record<string, boolean> = { quietInbox: true }) {
  const home = tmpDir();
  fs.writeFileSync(path.join(home, 'runner.json'), JSON.stringify({ experiments }));
  return makeHarness(home);
}

type H = ReturnType<typeof setup>;

function finish(h: H, id: string, input: Record<string, unknown> = { needs: 'nothing' }) {
  const s = h.adapter.sessions.filter((x) => x.opts.threadId === id).at(-1)!;
  s.init();
  s.out({ kind: 'event', event: { type: 'tool.call', itemId: `e${Math.random()}`, name: END_TURN, kind: 'mcp', title: 'end_turn', input } });
  s.text(`m${Math.random()}`, 'Done.');
  s.result('completed');
}

const mine = (h: H, text = 'Plan the port') => h.runner.create({ text, accountId: 'claude-main' }).thread.id;
const helper = (h: H, parent: string, text = 'Fix the panel reload') =>
  h.runner.create({ text, accountId: 'claude-main', startedBy: { kind: 'thread', label: 'Agent', threadId: parent } }).thread.id;
const notes = (h: H, id: string) => h.events(id).filter((e) => e.type === 'system.note').map((e) => String(e.text));

test('quietInbox: a thread another thread started archives itself when it finishes with nothing left', async () => {
  const h = setup();
  const parent = mine(h);
  const child = helper(h, parent);
  finish(h, child, { needs: 'nothing', byline: 'landed and pushed, nothing to do', open: [] });
  await until(() => Boolean(h.runner.get(child).archivedAt), 'the helper archived');
  const t = h.runner.get(child);
  assert.equal(t.archivedBy, 'threadbear');
  assert.ok(notes(h, child).some((n) => /^Archived: another thread started this one/.test(n)), 'says why it went');
  // How it came out is still on its row, for the list in its starter's Highlights.
  assert.equal(t.startedBy?.why, 'landed and pushed, nothing to do');
  // The thread that started it is Mike's own: it stays where it was.
  finish(h, parent);
  await settle();
  assert.equal(h.runner.get(parent).archivedAt, undefined);
});

test('quietInbox: off by default, and only for threads another thread started', async () => {
  const off = setup({});
  const a = helper(off, mine(off));
  finish(off, a);
  const h = setup();
  // Mike's own, a tool's and a schedule's stay: they're often what he wants to read.
  const own = mine(h);
  finish(h, own);
  const tool = h.runner.create({ text: 'Triage THREADBEAR-12', accountId: 'claude-main', startedBy: { kind: 'tool', label: 'Sentry' } }).thread.id;
  finish(h, tool);
  // The control: one that does go, to know the others have had time to.
  const b = helper(h, own);
  finish(h, b);
  await until(() => Boolean(h.runner.get(b).archivedAt), 'the control');
  await settle();
  assert.equal(off.runner.get(a).archivedAt, undefined, 'off: stays');
  assert.equal(h.runner.get(own).archivedAt, undefined, "Mike's own stays");
  assert.equal(h.runner.get(tool).archivedAt, undefined, "a tool's stays");
});

test('quietInbox: a helper that asks, suggests a follow-up, leaves something open or failed stays in the inbox', async () => {
  const h = setup();
  const parent = mine(h);
  const asks = helper(h, parent);
  finish(h, asks, { needs: 'you', asks: [{ question: 'Deploy it?' }] });
  const next = helper(h, parent);
  finish(h, next, { needs: 'nothing', next: ['Stop the Threadbear transcript jumping when a long reply arrives'] });
  const open = helper(h, parent);
  finish(h, open, { needs: 'nothing', open: ['A reply to Keith is worth sending'] });
  const booked = helper(h, parent);
  h.runner.schedulesFor = (t) => (t === booked ? [{ id: 's', title: 'Check the Android build', text: '', whenText: '', paused: false, createdAt: '', updatedAt: '', runs: 0, threadId: t, nextRunAt: '2026-11-13T01:30:00Z' }] : []);
  finish(h, booked);
  const failed = helper(h, parent);
  const fs1 = h.adapter.sessions.filter((x) => x.opts.threadId === failed).at(-1)!;
  fs1.init();
  fs1.result('failed');
  const stopped = helper(h, parent);
  const ss = h.adapter.sessions.filter((x) => x.opts.threadId === stopped).at(-1)!;
  ss.init();
  ss.out({ kind: 'event', event: { type: 'tool.call', itemId: 'e1', name: END_TURN, kind: 'mcp', title: 'end_turn', input: { needs: 'nothing' } } });
  ss.result('interrupted');
  const control = helper(h, parent);
  finish(h, control);
  await until(() => Boolean(h.runner.get(control).archivedAt), 'the control');
  await settle();
  for (const [id, why] of [[asks, 'asks Mike'], [next, 'suggests a follow-up'], [open, 'left something open'], [booked, 'booked a check-in'], [failed, 'failed'], [stopped, 'was stopped']] as const) {
    assert.equal(h.runner.get(id).archivedAt, undefined, `one that ${why} stays`);
  }
  // The check-in's run then finds nothing more to do: now it goes.
  h.runner.schedulesFor = () => [];
  h.runner.send(booked, 'Check the Android build', [], { origin: 'schedule', scheduleId: 's' });
  finish(h, booked);
  await until(() => Boolean(h.runner.get(booked).archivedAt), 'archived after its check-in');
});

test("quietInbox: a helper Mike has written in, starred or brought back out is his, and stays", async () => {
  const h = setup();
  const parent = mine(h);
  const wrote = helper(h, parent);
  finish(h, wrote, { needs: 'you', asks: [{ question: 'Which one?' }] });
  h.runner.send(wrote, 'the second one');
  finish(h, wrote);
  const starred = helper(h, parent);
  h.runner.update({ threadId: starred, starred: true });
  finish(h, starred);
  // Archived by itself, then Mike unarchives it to keep an eye on it.
  const back = helper(h, parent);
  finish(h, back);
  await until(() => Boolean(h.runner.get(back).archivedAt), 'archived the first time');
  h.runner.update({ threadId: back, archived: false });
  h.runner.send(back, '[Sent by the agent in the thread "Plan the port" (abcd1234)]\n\nOne more fix please', [], {});
  finish(h, back);
  // One its starter writes to again, with nobody having touched it, goes again.
  const again = helper(h, parent);
  finish(h, again);
  await until(() => Boolean(h.runner.get(again).archivedAt), 'archived the first time');
  h.runner.send(again, '[Sent by the agent in the thread "Plan the port" (abcd1234)]\n\nOne more fix please', [], {});
  assert.equal(h.runner.get(again).archivedAt, undefined, 'out while it works');
  finish(h, again);
  await until(() => Boolean(h.runner.get(again).archivedAt), 'archived again');
  await settle();
  assert.equal(h.runner.get(wrote).archivedAt, undefined, 'he wrote in it');
  assert.equal(h.runner.get(starred).archivedAt, undefined, 'he starred it');
  assert.equal(h.runner.get(back).archivedAt, undefined, 'he brought it back out');
});

test('quietInbox: a message that arrives while the check runs keeps the helper out', async () => {
  const h = setup();
  const parent = mine(h);
  const child = helper(h, parent);
  finish(h, child);
  // Before the check (git, off the main thread) has come back.
  h.runner.send(child, 'actually, hold on');
  await settle();
  assert.equal(h.runner.get(child).archivedAt, undefined);
});

test('quietInbox: a finished thread whose only loose ends are threads it started hibernates until they finish, then wakes with their replies', async () => {
  const h = setup();
  const parent = mine(h);
  const a = helper(h, parent, 'Text selection');
  const b = helper(h, parent, 'Open Anything');
  finish(h, parent);
  await until(() => Boolean(h.runner.get(parent).waitingOn?.length), 'the wait');
  let t = h.runner.get(parent);
  assert.deepEqual(t.waitingOn!.map((w) => w.threadId).sort(), [a, b].sort());
  assert.ok(t.snoozedUntil, 'hibernating, so out of the inbox');
  assert.equal(t.archivedAt, undefined);
  assert.ok(notes(h, parent).some((n) => /^Hibernating until /.test(n)), 'says why it went');
  const sessions = () => h.adapter.sessions.filter((x) => x.opts.threadId === parent).length;
  const before = sessions();
  // One finishing isn't all of them.
  finish(h, a);
  await until(() => Boolean(h.runner.get(a).archivedAt), 'the first helper archived');
  assert.ok(h.runner.get(parent).snoozedUntil, 'still waiting on the other');
  finish(h, b);
  await until(() => !h.runner.get(parent).snoozedUntil, 'the wake');
  const woke = h.events(parent).filter((e) => e.type === 'user.message').at(-1)!;
  assert.match(String(woke.text), /^Threadbear hibernated this thread after your last turn/, "says the runner put it away, not the agent");
  assert.match(String(woke.text), /They have finished now/);
  assert.match(String(woke.text), /Text selection/);
  assert.ok(sessions() > before || h.runner.get(parent).status !== 'idle', 'a turn runs with what they said');
  // Its reply then comes to the inbox as any done thread of Mike's does.
  finish(h, parent);
  await settle();
  t = h.runner.get(parent);
  assert.equal(t.snoozedUntil, undefined);
  assert.equal(t.archivedAt, undefined);
});

test("quietInbox: with something else left open as well, or with it off, a thread with helpers still going stays in the inbox", async () => {
  const h = setup();
  const mixed = mine(h);
  helper(h, mixed);
  finish(h, mixed, { needs: 'nothing', open: ['The export is being redone'] });
  const asks = mine(h);
  helper(h, asks);
  finish(h, asks, { needs: 'you', asks: [{ question: 'Which first?' }] });
  const off = setup({ looseEnds: true });
  const plain = mine(off);
  helper(off, plain);
  finish(off, plain);
  const control = mine(h);
  helper(h, control);
  finish(h, control);
  await until(() => Boolean(h.runner.get(control).waitingOn?.length), 'the control');
  await settle();
  assert.equal(h.runner.get(mixed).snoozedUntil, undefined, "something of the agent's own is open too");
  assert.equal(h.runner.get(asks).snoozedUntil, undefined, 'it asked Mike something');
  assert.equal(off.runner.get(plain).snoozedUntil, undefined, 'off: stays');
});

test('quietInbox: a helper with helpers of its own waits on them, then archives itself', async () => {
  const h = setup();
  const top = mine(h);
  const mid = helper(h, top, 'GPUI polish');
  const leaf = helper(h, mid, 'Fix the hover fade');
  finish(h, mid);
  await until(() => Boolean(h.runner.get(mid).waitingOn?.length), 'the wait');
  assert.equal(h.runner.get(mid).archivedAt, undefined, 'not archived with a helper still going');
  finish(h, leaf);
  await until(() => !h.runner.get(mid).snoozedUntil, 'the wake');
  finish(h, mid);
  await until(() => Boolean(h.runner.get(mid).archivedAt), 'archived once they were all done');
});

test('quietInbox: works beside the done card\'s own check, which still fills the card', async () => {
  const h = setup({ quietInbox: true, looseEnds: true });
  const own = mine(h);
  finish(h, own);
  await until(() => Boolean(h.runner.get(own).looseEnds), 'the card check');
  assert.deepEqual(h.runner.get(own).looseEnds?.ends, []);
  const child = helper(h, own);
  finish(h, child);
  await until(() => Boolean(h.runner.get(child).archivedAt), 'the helper archived');
});
