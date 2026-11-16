// Threads a restart, a crash or the Mac restarting cut off mid-turn carry
// on by themselves once the next runner is up (Runner.carryOnStopped),
// once: a carry-on that's cut off too is left for Alex, as are threads put
// away, ones with a message of their own and ones stopped long ago.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { makeHarness, tick } from '../helpers/fakes.ts';
import { CARRY_ON_RESTART } from '../../src/runner.ts';

const runnerMessages = (h: { runner: { events(id: string): { events: any[] } } }, id: string) =>
  h.runner.events(id).events.filter((e) => e.type === 'user.message' && e.origin === 'runner');

test('a crash mid-turn: the next runner carries the thread on, once', async () => {
  const h = makeHarness();
  const { thread } = h.runner.create({ text: 'build the thing' });
  h.adapter.last.init();
  // The runner dies (no shutdown); a new one loads the same data dir.
  const h2 = h.restart();
  assert.equal(h2.runner.get(thread.id).status, 'stopped');
  h2.runner.carryOnStopped();
  await tick();
  assert.equal(h2.adapter.sessions.length, 1);
  assert.equal(h2.adapter.last.sent[0].text, CARRY_ON_RESTART);
  assert.equal(h2.adapter.last.opts.resume, true);
  assert.equal(runnerMessages(h2, thread.id).length, 1);
  assert.ok(h2.notes.some((n) => n.method === 'runner.notice' && /carrying on by itself/.test((n.params as any).message)));
  // The carry-on dies with the runner too: left for Alex this time.
  h2.adapter.last.init();
  const h3 = h2.restart();
  h3.runner.carryOnStopped();
  await tick();
  assert.equal(h3.adapter.sessions.length, 0);
  assert.equal(h3.runner.get(thread.id).status, 'stopped');
  // And it doesn't come up again at the restart after.
  const h4 = h3.restart();
  h4.runner.carryOnStopped();
  await tick();
  assert.equal(h4.adapter.sessions.length, 0);
});

test('a clean stop mid-turn (a restart, a reboot) carries on too', async () => {
  const h = makeHarness();
  const { thread } = h.runner.create({ text: 'x' });
  h.adapter.last.init();
  await h.runner.shutdown();
  const h2 = h.restart();
  h2.runner.carryOnStopped();
  await tick();
  assert.equal(h2.adapter.last.sent[0].text, CARRY_ON_RESTART);
  assert.notEqual(h2.runner.get(thread.id).status, 'stopped');
});

test('not carried on: idle threads, put away, stopped long ago, or already sent a message', async () => {
  const h = makeHarness();
  const idle = h.runner.create({ text: 'idle' }).thread;
  h.adapter.last.init();
  h.adapter.last.result('completed');
  const archived = h.runner.create({ text: 'archived' }).thread;
  h.adapter.last.init();
  const old = h.runner.create({ text: 'old' }).thread;
  h.adapter.last.init();
  const told = h.runner.create({ text: 'told' }).thread;
  h.adapter.last.init();
  await tick();
  const h2 = h.restart();
  h2.runner.update({ threadId: archived.id, archived: true });
  // The restart's own resume got there first.
  h2.runner.send(told.id, 'the restart is done, check it');
  // Stopped more than 12 hours ago.
  h2.clock.advance(13 * 3_600_000);
  h2.runner.carryOnStopped();
  await tick();
  for (const t of [idle, archived, old, told]) assert.equal(runnerMessages(h2, t.id).length, 0, t.title);
  assert.equal(h2.adapter.sessions.length, 1, 'only the message sent');
  assert.ok(h2.notes.some((n) => n.method === 'runner.notice' && /send a message to carry on/.test((n.params as any).message)));
});

test('a first turn cut off before the CLI said its session started resumes that session, not a new one with the same id', async () => {
  const h = makeHarness();
  const { thread } = h.runner.create({ text: 'count to 60' });
  // No init: the restart comes before the CLI's init reaches the runner,
  // though the CLI has already written the session (4 Nov 2026, a real run).
  await h.runner.shutdown();
  const h2 = h.restart();
  h2.runner.carryOnStopped();
  await tick();
  const first = h2.adapter.last;
  assert.equal(first.opts.resume, false);
  const turnId = runnerMessages(h2, thread.id)[0].turnId;
  first.sink.exited(new Error(`Claude Code process exited with code 1\nError: Session ID ${first.opts.sessionId} is already in use.`));
  await tick();
  const again = h2.adapter.last;
  assert.notEqual(again, first);
  assert.equal(again.opts.resume, true);
  assert.equal(again.opts.sessionId, first.opts.sessionId);
  assert.equal(again.sent[0].id, turnId);
  assert.equal(again.sent[0].text, CARRY_ON_RESTART);
  again.init();
  again.result('completed');
  await tick();
  assert.equal(h2.events(thread.id).at(-1).outcome, 'completed');
  assert.ok(!h2.events(thread.id).some((e) => e.type === 'system.note' && /new one/.test(e.text)));
});
