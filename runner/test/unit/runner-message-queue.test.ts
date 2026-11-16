import { test } from 'node:test';
import assert from 'node:assert/strict';
import { makeHarness, tick } from '../helpers/fakes.ts';
import { Scheduler } from '../../src/schedules.ts';
import { nullLogger } from '../../src/log.ts';
import { scheduledMessageId } from '../../src/runner.ts';
import { checkOutcome } from '../../src/schedule-check.ts';

function running() {
  const h = makeHarness();
  const { thread, turnId } = h.runner.create({ text: 'Measure', accountId: 'claude-3' });
  const s = h.adapter.last;
  s.init();
  return { h, thread, turnId, s };
}

test('rebooked reminders keep only the current queued text while a turn runs', async () => {
  const { h, thread, s } = running();
  const sched = new Scheduler({ runner: h.runner, home: h.home, clock: h.clock, log: nullLogger });
  sched.start();
  try {
    const sc = await sched.create({ threadId: thread.id, text: 'Old check', at: new Date(h.clock.now() + 60_000).toISOString() });
    for (let i = 0; i < 4; i++) {
      h.clock.advance(60_000);
      await tick(8);
      assert.equal(h.runner.mustGet(thread.id).rec.queue.length, 1);
      assert.equal(sched.find(sc.id).runs, i + 1);
      await sched.update({ scheduleId: sc.id, text: `Current check ${i}`, at: new Date(h.clock.now() + 60_000).toISOString() });
    }
    h.clock.advance(60_000);
    await tick(8);
    assert.equal(h.runner.mustGet(thread.id).rec.queue.length, 1);
    assert.match(h.runner.mustGet(thread.id).rec.queue[0].text, /Current check 3/);
    s.result();
    await tick(8);
    assert.equal(s.sent.length, 2);
    assert.match(s.sent[1].text, /Current check 3/);
    assert.equal(sched.find(sc.id).runs, 5);
  } finally { sched.stop(); }
});

test('a delivery retry is acknowledged during its running turn and after compaction', () => {
  const { h, thread, s } = running();
  s.result();
  const sent = h.runner.sendScheduled(thread.id, 'Check', 's1', 'slot-1');
  assert.deepEqual(h.runner.sendScheduled(thread.id, 'Retry', 's1', 'slot-1'), sent);
  const queued = h.runner.sendScheduled(thread.id, 'New slot', 's1', 'slot-2');
  h.runner.sendScheduled(thread.id, 'Latest slot', 's1', 'slot-3');
  assert.deepEqual(h.runner.sendScheduled(thread.id, 'Retry superseded slot', 's1', 'slot-2'), queued);
  assert.deepEqual(h.runner.mustGet(thread.id).rec.queue.map(q => q.text), ['Latest slot']);
  assert.equal(s.sent.length, 2);
});

for (const peer of [false, true]) test(`${peer ? 'another thread' : 'Mike'} can steer past an old queued reminder without interrupting work`, () => {
  const { h, thread, turnId, s } = running();
  h.runner.send(thread.id, 'Old reminder', [], { origin: 'schedule', scheduleId: 's1', queue: true });
  const text = peer ? '[Sent by the agent in the thread "Coordinator" (12345678)]\n\nStop measuring and park the work.' : 'Stop measuring and park the work.';
  assert.equal(h.runner.send(thread.id, text).turnId, turnId);
  assert.equal(s.sent.at(-1)!.text, text);
  assert.equal(s.interrupts, 0);
  assert.deepEqual(h.runner.mustGet(thread.id).rec.queue.map(q => q.text), ['Old reminder']);
});

for (const queued of [false, true]) test(`a peer stop ${queued ? 'queues' : 'steers'} ahead of continuations with distinct schedule ids`, () => {
  const { h, thread, turnId, s } = running();
  for (let i = 0; i < 4; i++) h.runner.sendScheduled(thread.id, `Continue phase ${i}`, `schedule-${i}`, 'slot');
  const st = h.runner.mustGet(thread.id);
  assert.equal(st.rec.queue.length, 4, 'separate schedules are not coalesced');
  const text = '[Sent by the agent in the thread "Coordinator" (12345678)]\n\nStop the benchmark.';
  const sent = h.runner.send(thread.id, text, [], { queue: queued });
  if (queued) {
    assert.deepEqual(st.rec.queue.map(q => q.text), [text, ...Array.from({ length: 4 }, (_, i) => `Continue phase ${i}`)]);
    s.result();
  } else {
    assert.equal(sent.turnId, turnId);
    assert.equal(s.interrupts, 0);
  }
  assert.equal(s.sent.at(-1)!.text, text);
  assert.ok(h.events(thread.id).some(e => e.type === 'user.message' && e.text === text));
  assert.equal(st.rec.queue.length, 4);
});

for (const mode of ['snooze', 'archive'] as const) test(`external ${mode} holds already queued continuations with distinct schedule ids`, async () => {
  const { h, thread, s } = running();
  for (let i = 0; i < 4; i++) h.runner.sendScheduled(thread.id, `Continue phase ${i}`, `schedule-${i}`, 'slot');
  const st = h.runner.mustGet(thread.id);
  h.runner.update({ threadId: thread.id, ...(mode === 'snooze' ? { snoozedUntil: new Date(h.clock.now() + 86_400_000).toISOString() } : { archived: true }) });
  assert.equal(s.interrupts, 1);
  s.result('interrupted');
  await tick(8);
  h.clock.advance(60_000);
  await tick(8);
  assert.equal(st.rt.turn, undefined);
  assert.equal(s.sent.length, 1, 'no queued continuation starts');
  assert.equal(s.ended, true);
  assert.ok(mode === 'snooze' ? st.rec.snoozedUntil : st.rec.archivedAt);
  assert.deepEqual(st.rec.queue.map(q => q.text), Array.from({ length: 4 }, (_, i) => `Continue phase ${i}`));
});

test('legacy reminder piles keep only the newest per schedule and put other messages first', () => {
  const { h, thread, s } = running();
  const st = h.runner.mustGet(thread.id);
  // Old releases saved every rearmed run. Include a lost reply's key so
  // compaction cannot make its already accepted slot run again later.
  const old = h.runner.send(thread.id, 'Old', [], { origin: 'schedule', scheduleId: 's1', queue: true, messageId: scheduledMessageId(thread.id, 's1', 'old-run') });
  st.rec.queue.push(
    { turnId: 'peer', text: 'Peer report', images: [] },
    { turnId: 'new', text: 'New', images: [], origin: 'schedule', scheduleId: 's1' },
    { turnId: 'other', text: 'Other schedule', images: [], origin: 'schedule', scheduleId: 's2' },
    { turnId: 'person', text: 'Park the work', images: [] },
  );
  s.result();
  assert.equal(s.sent.at(-1)!.text, 'Peer report');
  assert.deepEqual(st.rec.queue.map(q => q.text), ['Park the work', 'New', 'Other schedule']);
  assert.ok(h.events(thread.id).some(e => e.type === 'system.note' && e.supersededReminder === old.turnId && /newer.*reminder/i.test(e.text)));
  assert.deepEqual(h.runner.sendScheduled(thread.id, 'Lost reply retry', 's1', 'old-run'), old);
  const adopted = makeHarness(h.home);
  assert.deepEqual(adopted.runner.findScheduledSend(thread.id, 's1', 'old-run'), old, 'the receipt survives recovery');
});

test('messages waiting for a fresh CLI retain their order ahead of reminders', () => {
  const { h, thread } = running();
  h.runner.send(thread.id, 'Reminder', [], { origin: 'schedule', scheduleId: 's1', queue: true });
  h.runner.send(thread.id, 'First message', [], { queue: true });
  h.runner.send(thread.id, 'Second message', [], { queue: true });
  assert.deepEqual(h.runner.mustGet(thread.id).rec.queue.map(q => q.text), ['First message', 'Second message', 'Reminder']);
});

test('repeated Run now requests also replace the pending reminder', async () => {
  const { h, thread, s } = running();
  const sched = new Scheduler({ runner: h.runner, home: h.home, clock: h.clock, log: nullLogger });
  sched.start();
  try {
    const sc = await sched.create({ threadId: thread.id, text: 'Explicit check', cron: '@hourly' });
    h.runner.send(thread.id, 'Old automatic check', [], { origin: 'schedule', scheduleId: sc.id, queue: true });
    await sched.runNow(sc.id);
    await sched.runNow(sc.id);
    const q = h.runner.mustGet(thread.id).rec.queue;
    assert.equal(q.length, 1);
    assert.match(q[0].text, /Explicit check/);
    s.result();
    assert.match(s.sent.at(-1)!.text, /Explicit check/);
  } finally { sched.stop(); }
});

test('compaction cannot discard a reminder before its receipt is durable', () => {
  const { h, thread } = running();
  const st = h.runner.mustGet(thread.id);
  st.rec.queue.push(
    { turnId: 'old', text: 'Old', images: [], origin: 'schedule', scheduleId: 's1' },
    { turnId: 'new', text: 'New', images: [], origin: 'schedule', scheduleId: 's1' },
  );
  const log = h.store.log(thread.id);
  const append = log.append.bind(log);
  log.append = () => { throw new Error('disk full'); };
  try {
    assert.throws(() => h.runner.prepareQueue(st), /disk full/);
    assert.equal(h.runner.kickQueue(st), false, 'a failed receipt does not throw out of the wake loop');
    assert.deepEqual(st.rec.queue.map(q => q.text), ['Old', 'New']);
  } finally { log.append = append; }
  h.runner.prepareQueue(st);
  assert.deepEqual(st.rec.queue.map(q => q.text), ['New']);
});

for (const mode of ['snooze', 'archive'] as const) test(`a new scheduled delivery wakes a thread in ${mode} even when it retained a queue`, async () => {
  const { h, thread, s } = running();
  s.result();
  h.runner.update({ threadId: thread.id, ...(mode === 'snooze' ? { snoozedUntil: new Date(h.clock.now() + 86_400_000).toISOString() } : { archived: true }) });
  h.runner.mustGet(thread.id).rec.queue.push({ turnId: 'peer', text: 'Peer message', images: [] });
  h.runner.sendScheduled(thread.id, 'Wake and check', 's1', 'slot');
  await tick(8);
  assert.equal(h.runner.get(thread.id).archivedAt, undefined);
  assert.equal(h.runner.get(thread.id).snoozedUntil, undefined);
  assert.equal(h.adapter.last.sent[0].text, 'Peer message');
  assert.deepEqual(h.runner.mustGet(thread.id).rec.queue.map(q => q.text), ['Wake and check']);
});

test('a queued reminder prevents an active turn declaring the whole thread finished or archiving it', () => {
  const { h, thread, s } = running();
  h.runner.sendScheduled(thread.id, 'One more check', 's1', 'slot');
  s.out({ kind: 'event', event: { type: 'tool.call', itemId: 'done', name: 'mcp__threadbear__end_turn', kind: 'mcp', title: 'end_turn', input: { needs: 'nothing', archive: true } } });
  s.result();
  assert.equal(h.runner.get(thread.id).archivedAt, undefined);
  assert.equal(h.runner.mustGet(thread.id).rec.finished, undefined);
  assert.equal(s.sent.at(-1)!.text, 'One more check');
});

test('queued check output is preserved even when a newer reminder has the same schedule', () => {
  const { h, thread } = running();
  for (const text of ['First new item', 'Second new item']) {
    const out = checkOutcome({ ok: true, output: text });
    assert.ok('extra' in out);
    h.runner.sendScheduled(thread.id, `Check${out.extra}`, 's1', text);
  }
  h.runner.sendScheduled(thread.id, 'Old ordinary reminder', 's1', 'old');
  h.runner.sendScheduled(thread.id, 'Latest reminder', 's1', 'latest');
  const q = h.runner.mustGet(thread.id).rec.queue;
  assert.equal(q.length, 3);
  assert.match(q[0].text, /First new item/);
  assert.match(q[1].text, /Second new item/);
  assert.equal(q[2].text, 'Latest reminder');
});

test('queue compaction waits while a message edit owns the log', () => {
  const { h, thread } = running();
  const st = h.runner.mustGet(thread.id);
  st.rec.queue.push(
    { turnId: 'old', text: 'Old', images: [], origin: 'schedule', scheduleId: 's1' },
    { turnId: 'new', text: 'New', images: [], origin: 'schedule', scheduleId: 's1' },
  );
  st.rt.editing = true;
  const seq = h.store.log(thread.id).lastSeq;
  assert.equal(h.runner.kickQueue(st), false);
  assert.equal(h.store.log(thread.id).lastSeq, seq);
  assert.equal(st.rec.queue.length, 2);
  st.rt.editing = false;
  h.runner.kickQueue(st);
  assert.deepEqual(st.rec.queue.map(q => q.text), ['New']);
});

test('replacement receipts belong to the real active turn, never an unstarted reminder', () => {
  const { h, thread, turnId, s } = running();
  h.runner.sendScheduled(thread.id, 'Old', 's1', 'old');
  h.runner.sendScheduled(thread.id, 'Latest', 's1', 'latest');
  const receipts = h.events(thread.id).filter(e => e.type === 'system.note' && e.supersededReminder);
  assert.equal(receipts.length, 1);
  assert.equal(receipts[0].turnId, turnId);
  s.result();
  assert.ok(h.events(thread.id).some(e => e.type === 'turn.completed' && e.turnId === receipts[0].turnId));
});

test('receipt metadata acknowledges an old delivery independently of the note wording', () => {
  const { h, thread, turnId } = running();
  const id = scheduledMessageId(thread.id, 's1', 'old-slot');
  h.runner.emit(h.runner.mustGet(thread.id), { type: 'system.note', text: 'A previous release used these words.', supersededReminder: id }, turnId);
  assert.deepEqual(h.runner.findScheduledSend(thread.id, 's1', 'old-slot'), { turnId: id });
});

test('a durable receipt removes a stale saved queue entry without a newer reminder', () => {
  const { h, thread, turnId, s } = running();
  const st = h.runner.mustGet(thread.id);
  const id = scheduledMessageId(thread.id, 's1', 'old');
  st.rec.queue.push({ turnId: id, text: 'Already superseded', images: [], origin: 'schedule', scheduleId: 's1', scheduledSince: 0 });
  h.runner.emit(st, { type: 'system.note', text: 'Superseded before the queue snapshot was saved.', supersededReminder: id }, turnId);
  s.result();
  assert.equal(st.rec.queue.length, 0);
  assert.equal(s.sent.length, 1);
  assert.equal(h.events(thread.id).filter(e => e.type === 'turn.started').length, 1);
});

test('repeated failed checks without output coalesce, but consumed failure output survives', () => {
  const { h, thread } = running();
  for (const [run, output] of [['data', 'A new item'], ['first', ''], ['latest', '']]) {
    const out = checkOutcome({ ok: false, output, why: 'timeout' });
    assert.ok('extra' in out);
    h.runner.sendScheduled(thread.id, `Check${out.extra}`, 's1', run);
  }
  const q = h.runner.mustGet(thread.id).rec.queue;
  assert.equal(q.length, 2);
  assert.match(q[0].text, /A new item/);
});

test('queued Run now requests cannot keep a stop message out of an active turn', () => {
  const { h, thread, turnId, s } = running();
  h.runner.sendScheduled(thread.id, 'Explicit run', 's1', 'manual', 0);
  assert.equal(h.runner.send(thread.id, 'Stop now').turnId, turnId);
  assert.equal(s.sent.at(-1)!.text, 'Stop now');
});

test('a malformed legacy reminder does not block valid messages', () => {
  const { h, thread, s } = running();
  const st = h.runner.mustGet(thread.id);
  st.rec.queue.push({ turnId: 'broken', images: [], origin: 'schedule', scheduleId: 's1' } as any);
  h.runner.send(thread.id, 'Valid message', [], { queue: true });
  s.result();
  assert.equal(s.sent.at(-1)!.text, 'Valid message');
});

for (const mode of ['archive', 'snooze', 'waitFor'] as const) test(`an older reminder cannot undo ${mode} after a later stop message`, () => {
  const { h, thread, s } = running();
  h.runner.sendScheduled(thread.id, 'Check the benchmark', 's1', 'old');
  h.runner.send(thread.id, 'Stop the benchmark and park this thread');
  const st = h.runner.mustGet(thread.id);
  const input = mode === 'archive' ? { archive: true } : mode === 'snooze' ? { snooze: '1d' } : { waitFor: [h.runner.create({ text: 'Unfinished helper', accountId: 'claude-3' }).thread.id] };
  s.out({ kind: 'event', event: { type: 'tool.call', itemId: 'park', name: 'mcp__threadbear__end_turn', kind: 'mcp', title: 'end_turn', input: { needs: 'nothing', ...input } } });
  s.result('completed', undefined, undefined, { answered: [...st.rt.turn!.messages] });
  assert.equal(st.rt.turn, undefined, 'the old reminder must not start another turn');
  assert.ok(st.rec.archivedAt || st.rec.snoozedUntil, 'the requested park takes effect');
  assert.equal(st.rec.queue.length, 1, 'accepted input is held, not discarded');
});

test('runner notifications cannot keep a stop out of an active turn', () => {
  const { h, thread, turnId, s } = running();
  h.runner.send(thread.id, 'A background task stopped during handover', [], { origin: 'runner', queue: true });
  assert.equal(h.runner.send(thread.id, 'Stop now').turnId, turnId);
  assert.equal(s.sent.at(-1)!.text, 'Stop now');
});

test('a fenced command in the reminder body does not turn an empty check failure into consumed output', () => {
  const { h, thread } = running();
  const out = checkOutcome({ ok: false, output: '', why: 'timeout' });
  assert.ok('extra' in out);
  for (const slot of ['old', 'latest']) h.runner.sendScheduled(thread.id, `Run this:\n\n\`\`\`\ninspect\n\`\`\`${out.extra}`, 's1', slot);
  assert.equal(h.runner.mustGet(thread.id).rec.queue.length, 1);
});

for (const peer of [false, true]) test(`an interrupt cannot let a reminder overtake an unread ${peer ? 'peer' : 'user'} stop`, () => {
  const { h, thread, s } = running();
  h.runner.sendScheduled(thread.id, 'Check the benchmark', 's1', 'old');
  const text = peer ? '[Sent by the agent in the thread "Coordinator" (12345678)]\n\nStop now.' : 'Stop now.';
  h.runner.send(thread.id, text);
  const st = h.runner.mustGet(thread.id);
  const unread = st.rt.turn!.messages.at(-1)!;
  h.runner.interrupt(thread.id);
  s.result('interrupted', undefined, undefined, { unreceived: [unread], queuedTurns: 0 });
  if (peer) assert.equal(s.sent.at(-1)!.text, text, 'the unread peer stop is queued again before reminders');
  else {
    assert.equal(st.rt.turn, undefined, 'automatic work waits for the saved user message');
    assert.equal(st.rec.interruptedMessages?.length, 1);
  }
  assert.ok(!s.sent.at(-1)!.text.includes('Check the benchmark'));
});

test('a copied check report in the schedule body cannot disable coalescing for new sends', async () => {
  const { h, thread } = running();
  const sched = new Scheduler({ runner: h.runner, home: h.home, clock: h.clock, log: nullLogger });
  sched.start();
  try {
    const sc = await sched.create({ threadId: thread.id, text: 'Quoted report\n\nIts check found this:\n\n```\nold data\n```', cron: '@hourly' });
    await sched.runNow(sc.id);
    await sched.runNow(sc.id);
    assert.equal(h.runner.mustGet(thread.id).rec.queue.length, 1);
  } finally { sched.stop(); }
});

test('a one-time held-check notice survives a later plain reminder', async () => {
  const { HELD_EXTRA } = await import('../../src/schedule-check.ts');
  const { h, thread } = running();
  h.runner.sendScheduled(thread.id, `Check${HELD_EXTRA}`, 's1', 'held');
  h.runner.sendScheduled(thread.id, 'Plain reminder', 's1', 'plain');
  assert.equal(h.runner.mustGet(thread.id).rec.queue.length, 2);
});

test('check-data protection survives a handover moved fallback', async () => {
  const { sendScheduled } = await import('../../src/handover.ts');
  const { RpcError, ErrorCode } = await import('../../src/protocol.ts');
  const { h, thread } = running();
  await sendScheduled(h.runner, { call: async (_id, _method, p) => {
    assert.equal(p.preserve, true);
    throw new RpcError(ErrorCode.moved, 'adopted');
  } }, thread.id, 'One-time check data', 's1', 'data', h.clock.now(), true);
  h.runner.sendScheduled(thread.id, 'Plain reminder', 's1', 'plain');
  assert.equal(h.runner.mustGet(thread.id).rec.queue.length, 2);
});

test('a new reminder after the later message still needs handling before archive', () => {
  const { h, thread, s } = running();
  h.runner.sendScheduled(thread.id, 'Old', 's1', 'old');
  h.runner.send(thread.id, 'Park this thread');
  h.runner.sendScheduled(thread.id, 'New after the message', 's1', 'new');
  const st = h.runner.mustGet(thread.id);
  s.out({ kind: 'event', event: { type: 'tool.call', itemId: 'park', name: 'mcp__threadbear__end_turn', kind: 'mcp', title: 'end_turn', input: { needs: 'nothing', archive: true } } });
  s.result('completed', undefined, undefined, { answered: [...st.rt.turn!.messages] });
  assert.equal(st.rec.archivedAt, undefined);
  assert.equal(s.sent.at(-1)!.text, 'New after the message');
});

test('an older check report is retained through parking and warns about later instructions', () => {
  const { h, thread, s } = running();
  h.runner.sendScheduled(thread.id, 'New data from the check', 's1', 'data', 0, true);
  h.runner.send(thread.id, 'Stop and park');
  const st = h.runner.mustGet(thread.id);
  s.out({ kind: 'event', event: { type: 'tool.call', itemId: 'park', name: 'mcp__threadbear__end_turn', kind: 'mcp', title: 'end_turn', input: { needs: 'nothing', archive: true } } });
  s.result('completed', undefined, undefined, { answered: [...st.rt.turn!.messages] });
  assert.equal(st.rec.queue.length, 1);
  h.store.save(st.rec);
  const adopted = makeHarness(h.home);
  const pending = adopted.runner.mustGet(thread.id).rec.queue[0];
  assert.equal(pending.preserveSchedule, true);
  assert.equal(pending.beforeMessage, true);
  assert.match(adopted.runner.forAgent(pending), /New data from the check\n\nThis reminder was queued before a later message/);
});

test('multiple unread peer inputs retain their order when an interrupt queues them again', () => {
  const { h, thread, s } = running();
  h.runner.sendScheduled(thread.id, 'Check again', 's1', 'old');
  const peer = (text: string) => `[Sent by the agent in the thread "Coordinator" (12345678)]\n\n${text}`;
  h.runner.send(thread.id, peer('First stop'));
  h.runner.send(thread.id, peer('Second instruction'));
  const st = h.runner.mustGet(thread.id);
  const unreceived = st.rt.turn!.messages.slice(1);
  h.runner.interrupt(thread.id);
  s.result('interrupted', undefined, undefined, { unreceived, queuedTurns: 0 });
  assert.equal(s.sent.at(-1)!.text, peer('First stop'));
  assert.equal(st.rec.queue[0].text, peer('Second instruction'));
});

test('interrupting the peer input that started a turn never requeues that first request', () => {
  const h = makeHarness();
  const { thread, turnId } = h.runner.create({ text: '[Sent by the agent in the thread "Coordinator" (12345678)]\n\nStart work', accountId: 'claude-3' });
  const s = h.adapter.last;
  h.runner.interrupt(thread.id);
  s.result('interrupted', undefined, undefined, { unreceived: [turnId], queuedTurns: 0 });
  assert.equal(h.runner.mustGet(thread.id).rec.queue.length, 0);
  assert.equal(h.runner.mustGet(thread.id).rt.turn, undefined);
});

test('a saved unread user stop holds a later automatic send, releases its idle CLI and survives recovery', () => {
  const { h, thread, s } = running();
  h.runner.send(thread.id, 'Stop now');
  const st = h.runner.mustGet(thread.id);
  const unread = st.rt.turn!.messages.at(-1)!;
  h.runner.interrupt(thread.id);
  s.result('interrupted', undefined, undefined, { unreceived: [unread], queuedTurns: 0 });
  h.runner.sendScheduled(thread.id, 'One-time check data', 's1', 'later', 0, true);
  assert.equal(st.rt.turn, undefined);
  assert.ok(st.rt.closing || !st.rt.session, 'held input must not pin an idle CLI');
  h.store.save(st.rec);
  const adopted = makeHarness(h.home);
  assert.equal(adopted.runner.mustGet(thread.id).rec.queue.length, 1);
  assert.equal(adopted.runner.mustGet(thread.id).rec.queue[0].preserveSchedule, true);
});

test('a saved user cancellation holds an unread peer retry until the user writes again', async () => {
  const { h, thread, s } = running();
  h.runner.sendScheduled(thread.id, 'Check again', 's1', 'old');
  const peer = '[Sent by the agent in the thread "Coordinator" (12345678)]\n\nContinue work';
  h.runner.send(thread.id, peer);
  h.runner.send(thread.id, "Don't do that");
  const st = h.runner.mustGet(thread.id);
  const unreceived = st.rt.turn!.messages.slice(1);
  h.runner.interrupt(thread.id);
  s.result('interrupted', undefined, undefined, { unreceived, queuedTurns: 0 });
  assert.equal(st.rt.turn, undefined);
  assert.equal(st.rec.queue.length, 2);
  h.runner.send(thread.id, 'Keep it parked');
  await tick(8);
  assert.ok(h.adapter.last.sent.at(-1)!.text.includes("Don't do that"));
  assert.ok(h.adapter.last.sent.at(-1)!.text.includes('Keep it parked'));
});

test('a retried peer stop parks a reminder that arrived after its original steer', () => {
  const { h, thread, s } = running();
  const peer = '[Sent by the agent in the thread "Coordinator" (12345678)]\n\nStop now';
  h.runner.send(thread.id, peer);
  const st = h.runner.mustGet(thread.id);
  const unread = st.rt.turn!.messages.at(-1)!;
  h.runner.sendScheduled(thread.id, 'Reminder after the first stop attempt', 's1', 'later');
  h.runner.interrupt(thread.id);
  s.result('interrupted', undefined, undefined, { unreceived: [unread], queuedTurns: 0 });
  assert.equal(s.sent.at(-1)!.text, peer);
  s.out({ kind: 'event', event: { type: 'tool.call', itemId: 'park', name: 'mcp__threadbear__end_turn', kind: 'mcp', title: 'end_turn', input: { needs: 'nothing', snooze: '1d' } } });
  s.result('completed', undefined, undefined, { answered: [...st.rt.turn!.messages] });
  assert.equal(st.rt.turn, undefined);
  assert.ok(st.rec.snoozedUntil);
  assert.equal(st.rec.queue[0].beforeMessage, true);
});

test('a fresh peer message is told to keep stopped work paused without reviving the saved user request', () => {
  const { h, thread, s } = running();
  h.runner.send(thread.id, "Don't run the benchmark");
  const st = h.runner.mustGet(thread.id);
  const unread = st.rt.turn!.messages.at(-1)!;
  h.runner.interrupt(thread.id);
  s.result('interrupted', undefined, undefined, { unreceived: [unread], queuedTurns: 0 });
  h.runner.send(thread.id, '[Sent by the agent in the thread "Coordinator" (12345678)]\n\nRun the benchmark');
  assert.ok(!s.sent.at(-1)!.text.includes("Don't run the benchmark"));
  assert.match(s.sent.at(-1)!.text, /Keep the stopped work paused until the user writes again/);
  s.result('completed', undefined, undefined, { answered: [...st.rt.turn!.messages] });
  assert.equal(st.rec.interruptedMessages?.length, 1, 'a peer cannot settle the saved user request');
});

test('a peer joining the user recovery turn cannot tell it to pause the request just resent', () => {
  const { h, thread, s } = running();
  h.runner.send(thread.id, 'Stop now');
  const st = h.runner.mustGet(thread.id);
  const unread = st.rt.turn!.messages.at(-1)!;
  h.runner.interrupt(thread.id);
  s.result('interrupted', undefined, undefined, { unreceived: [unread], queuedTurns: 0 });
  h.runner.send(thread.id, 'Resume with these corrections');
  assert.ok(st.rt.turn?.recoveryInput);
  const peer = '[Sent by the agent in the thread "Coordinator" (12345678)]\n\nStatus report';
  h.runner.send(thread.id, peer);
  assert.equal(s.sent.at(-1)!.text, peer);
});

for (const mode of ['archive', 'snooze', 'waitFor'] as const) test(`an ordinary stop joining a background-opened turn can ${mode} ahead of older reminders`, () => {
  const { h, thread, s } = running();
  s.result();
  s.out({ kind: 'cliTurn' });
  h.runner.sendScheduled(thread.id, 'Old reminder', 's1', 'old');
  h.runner.send(thread.id, 'Stop and park this thread');
  const st = h.runner.mustGet(thread.id);
  const input = mode === 'archive' ? { archive: true } : mode === 'snooze' ? { snooze: '1d' } : { waitFor: [h.runner.create({ text: 'Unfinished helper', accountId: 'claude-3' }).thread.id] };
  s.out({ kind: 'event', event: { type: 'tool.call', itemId: 'park', name: 'mcp__threadbear__end_turn', kind: 'mcp', title: 'end_turn', input: { needs: 'nothing', ...input } } });
  s.result('completed', undefined, undefined, { answered: [...st.rt.turn!.messages] });
  assert.equal(st.rt.turn, undefined);
  assert.ok(st.rec.archivedAt || st.rec.snoozedUntil);
  assert.equal(st.rec.queue.length, 1);
});

test('a runner notification alone cannot enable archive from a background-opened turn', () => {
  const { h, thread, s } = running();
  s.result();
  s.out({ kind: 'cliTurn' });
  h.runner.send(thread.id, 'Automatic notification', [], { origin: 'runner' });
  const st = h.runner.mustGet(thread.id);
  s.out({ kind: 'event', event: { type: 'tool.call', itemId: 'park', name: 'mcp__threadbear__end_turn', kind: 'mcp', title: 'end_turn', input: { needs: 'nothing', archive: true } } });
  s.result('completed', undefined, undefined, { answered: [...st.rt.turn!.messages] });
  assert.equal(st.rec.archivedAt, undefined);
});
