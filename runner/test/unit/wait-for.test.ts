// end_turn's waitFor (runner/hibernation.ts, waitOn): a thread hibernates
// until the threads it names finish, then wakes with their replies.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { makeHarness } from '../helpers/fakes.ts';
import { checkSummary } from '../helpers/protocol-check.ts';
import { WAIT_FOR_DEFAULT_MS } from '../../src/runner/hibernation.ts';

const END_TURN = 'mcp__threadbear__end_turn';

function setup() {
  const h = makeHarness();
  const make = (text: string) => {
    const { thread } = h.runner.create({ text, accountId: 'claude-3' });
    const s = h.adapter.last;
    s.init();
    s.result();
    return thread.id;
  };
  const waiter = make('Write the plan once the I/O numbers are in');
  const target = make('Measure the Convex I/O saving');
  const other = make('Something else');
  // The thread's live session, or the newest one it started.
  const session = (id: string) => h.adapter.sessions.filter((s) => s.opts.threadId === id).at(-1)!;
  // A turn in a thread that ends with this end_turn, after a reply.
  const turn = (id: string, input: Record<string, unknown>, reply = 'Done.') => {
    h.runner.send(id, 'go on');
    const s = session(id);
    s.out({ kind: 'event', event: { type: 'tool.call', itemId: 'e1', name: END_TURN, kind: 'mcp', title: 'end_turn', input } });
    s.text('m1', reply);
    s.result();
  };
  return { h, waiter, target, other, session, turn };
}

test('waitFor: hibernates until the thread finishes, then wakes and carries on with its reply', () => {
  const { h, waiter, target, session, turn } = setup();
  turn(waiter, { needs: 'nothing', waitFor: [target.slice(0, 8)] });
  let t = h.runner.get(waiter);
  assert.deepEqual(checkSummary(t), []);
  assert.equal(Date.parse(t.snoozedUntil!), h.clock.now() + WAIT_FOR_DEFAULT_MS);
  assert.deepEqual(t.waitingOn, [{ threadId: target, title: h.runner.get(target).title }]);
  assert.match(h.events(waiter).at(-1).text, /^Hibernating until ".+" finishes, or .+ at the latest\.$/);

  turn(target, { needs: 'nothing' }, 'Saved 4.1 MB on the restart.');
  t = h.runner.get(waiter);
  assert.equal(t.snoozedUntil, undefined);
  assert.equal(t.waitingOn, undefined);
  assert.ok(['starting', 'running'].includes(t.status), 'a new turn started in the waiting thread');
  const msg = h.events(waiter).filter((e) => e.type === 'user.message').at(-1);
  assert.equal(msg.origin, 'runner');
  const sent = msg.text as string;
  assert.match(sent, /Saved 4\.1 MB on the restart\./);
  assert.match(sent, /Carry on from here/);
  assert.ok(h.events(waiter).some((e) => e.type === 'system.note' && /^Woke up.*: ".+" finished\.$/.test(e.text)));
});

test('waitFor: a target that ends needing Alex, or hibernates itself, has not finished', () => {
  const { h, waiter, target, turn } = setup();
  turn(waiter, { needs: 'nothing', waitFor: [target] });
  turn(target, { needs: 'you', asks: [{ question: 'Restart the runner?' }] });
  assert.ok(h.runner.get(waiter).snoozedUntil);
  turn(target, { needs: 'nothing', snooze: 'restart' });
  assert.ok(h.runner.get(waiter).snoozedUntil);
  assert.deepEqual(h.runner.get(waiter).waitingOn?.map((w) => w.done), [undefined]);
});

test('waitFor: with two threads it wakes once both are done; archiving one counts', () => {
  const { h, waiter, target, other, turn } = setup();
  turn(waiter, { needs: 'nothing', waitFor: [target, other], snooze: '2h' });
  assert.equal(Date.parse(h.runner.get(waiter).snoozedUntil!), h.clock.now() + 2 * 3_600_000);
  turn(target, { needs: 'nothing' }, 'First one done.');
  let t = h.runner.get(waiter);
  assert.ok(t.snoozedUntil);
  assert.deepEqual(t.waitingOn?.map((w) => w.done), [true, undefined]);
  assert.deepEqual(checkSummary(t), []);
  h.runner.update({ threadId: other, archived: true });
  t = h.runner.get(waiter);
  assert.equal(t.snoozedUntil, undefined);
});

test('waitFor: a message from the thread it waits on wakes it as any message does', () => {
  const { h, waiter, target, turn } = setup();
  turn(waiter, { needs: 'nothing', waitFor: [target] });
  h.runner.send(waiter, 'Here is the report early');
  assert.equal(h.runner.get(waiter).waitingOn, undefined);
  assert.equal(h.runner.get(waiter).snoozedUntil, undefined);
});

test('waitFor: only with needs "nothing", and an unknown id leaves it in the inbox with a note', () => {
  const { h, waiter, target, turn } = setup();
  turn(waiter, { needs: 'you', asks: [{ question: 'Which?' }], waitFor: [target] });
  assert.equal(h.runner.get(waiter).snoozedUntil, undefined);
  turn(waiter, { needs: 'nothing', waitFor: ['ffffffff'] });
  assert.equal(h.runner.get(waiter).snoozedUntil, undefined);
  assert.ok(h.events(waiter).some((e) => e.type === 'system.note' && /^Not hibernated: none of the threads/.test(e.text)));
});

test('waitFor: a thread that finishes while the waiting turn is still ending counts', () => {
  const { h, waiter, target, session, turn } = setup();
  h.runner.send(waiter, 'go on');
  const s = session(waiter);
  s.out({ kind: 'event', event: { type: 'tool.call', itemId: 'e1', name: END_TURN, kind: 'mcp', title: 'end_turn', input: { needs: 'nothing', waitFor: [target] } } });
  turn(target, { needs: 'nothing' }, 'Finished first.');
  s.text('m1', 'Waiting on it.');
  s.result();
  const t = h.runner.get(waiter);
  assert.equal(t.snoozedUntil, undefined, 'woken straight away');
  assert.match(h.events(waiter).filter((e) => e.type === 'user.message').at(-1).text, /Finished first\./);
});

test('waitFor: a background job still running means not finished; the turn after it ends counts', () => {
  const { h, waiter, target, session, turn } = setup();
  turn(waiter, { needs: 'nothing', waitFor: [target] });
  h.runner.send(target, 'measure it');
  const s = session(target);
  s.out({ kind: 'tasks', tasks: [{ id: 'b1', type: 'local_bash', kind: 'shell', description: 'measure' }] });
  s.out({ kind: 'event', event: { type: 'tool.call', itemId: 'e1', name: END_TURN, kind: 'mcp', title: 'end_turn', input: { needs: 'nothing' } } });
  s.text('m1', 'Started the measurement.');
  s.result();
  assert.ok(h.runner.get(waiter).snoozedUntil, 'still waiting while the job runs');
  // The job ends and the CLI opens a turn by itself with the report.
  s.out({ kind: 'tasks', tasks: [] });
  s.out({ kind: 'cliTurn' });
  s.out({ kind: 'event', event: { type: 'tool.call', itemId: 'e2', name: END_TURN, kind: 'mcp', title: 'end_turn', input: { needs: 'nothing' } } });
  s.text('m2', 'Saved 4.1 MB.');
  s.result();
  assert.equal(h.runner.get(waiter).snoozedUntil, undefined);
  assert.match(h.events(waiter).filter((e) => e.type === 'user.message').at(-1).text, /Saved 4\.1 MB\./);
});

test('waitFor: the reply handed over is the one it finished with, not a later message', () => {
  const { h, waiter, target, other, turn } = setup();
  turn(waiter, { needs: 'nothing', waitFor: [target, other] });
  turn(target, { needs: 'nothing' }, 'The real report.');
  turn(target, { needs: 'you', asks: [{ question: 'Next?' }] }, 'Something later.');
  turn(other, { needs: 'nothing' }, 'Other done.');
  const text = h.events(waiter).filter((e) => e.type === 'user.message').at(-1).text as string;
  assert.match(text, /The real report\./);
  assert.doesNotMatch(text, /Something later/);
});

test('end_turn: a refused repeat keeps the card the first one set', () => {
  const { h, waiter, session } = setup();
  h.runner.send(waiter, 'go on');
  const s = session(waiter);
  s.out({ kind: 'event', event: { type: 'tool.call', itemId: 'e1', name: END_TURN, kind: 'mcp', title: 'end_turn', input: { needs: 'you', asks: [{ question: 'Deploy it?' }] } } });
  s.out({ kind: 'event', event: { type: 'tool.result', itemId: 'e1', ok: true, text: 'Noted: this thread will wait in Needs you.', truncated: false, images: [] } });
  s.out({ kind: 'event', event: { type: 'tool.call', itemId: 'e2', name: END_TURN, kind: 'mcp', title: 'end_turn', input: { needs: 'nothing', waitFor: [waiter] } } });
  s.out({ kind: 'event', event: { type: 'tool.result', itemId: 'e2', ok: false, text: "Not ended: waitFor can't name this thread.", truncated: false, images: [] } });
  s.text('m1', 'Want me to deploy?');
  s.result();
  assert.equal(h.runner.get(waiter).needs, 'you');
});

test('waitFor: a finish with a message queued behind it is not done', () => {
  const { h, waiter, target, session, turn } = setup();
  turn(waiter, { needs: 'nothing', waitFor: [target] });
  h.runner.send(target, 'first job');
  const s = session(target);
  h.runner.send(target, 'second job', [], { queue: true });
  s.out({ kind: 'event', event: { type: 'tool.call', itemId: 'e1', name: END_TURN, kind: 'mcp', title: 'end_turn', input: { needs: 'nothing' } } });
  s.text('m1', 'First done.');
  s.result();
  assert.ok(h.runner.get(waiter).snoozedUntil, 'still waiting: the queued job started');
});

test('waitFor: an earlier finish does not count once the thread has new work', () => {
  const { h, waiter, target, session, turn } = setup();
  turn(target, { needs: 'nothing' }, 'Unrelated earlier report.');
  // The waiting turn sets the target going, then waits on it.
  h.runner.send(waiter, 'go on');
  h.runner.send(target, 'measure it');
  const s = session(waiter);
  s.out({ kind: 'event', event: { type: 'tool.call', itemId: 'e1', name: END_TURN, kind: 'mcp', title: 'end_turn', input: { needs: 'nothing', waitFor: [target] } } });
  s.text('m1', 'Waiting.');
  s.result();
  assert.ok(h.runner.get(waiter).snoozedUntil, 'the old finish is not the answer');
  const b = session(target);
  b.out({ kind: 'event', event: { type: 'tool.call', itemId: 'e9', name: END_TURN, kind: 'mcp', title: 'end_turn', input: { needs: 'nothing' } } });
  b.text('m9', 'The measurement.');
  b.result();
  assert.equal(h.runner.get(waiter).snoozedUntil, undefined);
  assert.match(h.events(waiter).filter((e) => e.type === 'user.message').at(-1).text, /The measurement\./);
});

test('waitFor: waiting on an idle thread that already finished wakes at once with its reply', () => {
  const { h, waiter, target, turn } = setup();
  turn(target, { needs: 'nothing' }, 'Already done.');
  turn(waiter, { needs: 'nothing', waitFor: [target] });
  assert.equal(h.runner.get(waiter).snoozedUntil, undefined);
  assert.match(h.events(waiter).filter((e) => e.type === 'user.message').at(-1).text, /Already done\./);
});

test("end_turn: a refused repeat keeps the card on Codex's form too", () => {
  const { h, waiter, session } = setup();
  h.runner.send(waiter, 'go on');
  const s = session(waiter);
  s.out({ kind: 'event', event: { type: 'tool.call', itemId: 'e1', name: END_TURN, kind: 'mcp', title: 'end_turn', input: { needs: 'you', asks: [{ question: 'Deploy it?' }] } } });
  s.out({ kind: 'event', event: { type: 'tool.result', itemId: 'e1', ok: true, text: 'Noted. Now write your final answer.', truncated: false, images: [] } });
  s.out({ kind: 'event', event: { type: 'tool.call', itemId: 'e2', name: END_TURN, kind: 'mcp', title: 'end_turn', input: { needs: 'nothing', waitFor: [waiter] } } });
  s.out({ kind: 'event', event: { type: 'tool.result', itemId: 'e2', ok: false, text: "Not ended: waitFor can't name this thread.", truncated: false, images: [] } });
  s.text('m1', 'Want me to deploy?');
  s.result();
  assert.equal(h.runner.get(waiter).needs, 'you');
});

test('waitFor: a pending follow-up after a job ended mid-turn means not finished', () => {
  const { h, waiter, target, session, turn } = setup();
  turn(waiter, { needs: 'nothing', waitFor: [target] });
  h.runner.send(target, 'measure it');
  const s = session(target);
  s.out({ kind: 'tasks', tasks: [{ id: 'b1', type: 'local_bash', kind: 'shell', description: 'measure' }] });
  s.out({ kind: 'tasks', tasks: [] });
  s.out({ kind: 'event', event: { type: 'tool.call', itemId: 'e1', name: END_TURN, kind: 'mcp', title: 'end_turn', input: { needs: 'nothing' } } });
  s.text('m1', 'Started it.');
  s.result();
  assert.ok(h.runner.get(waiter).snoozedUntil, 'the turn reading the result is still to come');
});

test('waitFor: held while the runner is starting, then checked', () => {
  const { h, waiter, target, turn } = setup();
  turn(waiter, { needs: 'nothing', waitFor: [target] });
  h.runner.holdWaits = true;
  turn(target, { needs: 'nothing' }, 'Done while starting.');
  assert.ok(h.runner.get(waiter).snoozedUntil, 'held');
  h.runner.holdWaits = false;
  h.runner.reconcileWaits();
  assert.equal(h.runner.get(waiter).snoozedUntil, undefined);
});

test('waitFor until: an ordinary finish is not enough; the librarian saying it has happened wakes it', () => {
  const { h, waiter, target, turn } = setup();
  turn(waiter, { needs: 'nothing', waitFor: [{ thread: target, until: 'it reports the I/O saving' }] });
  let t = h.runner.get(waiter);
  assert.deepEqual(t.waitingOn, [{ threadId: target, title: h.runner.get(target).title, until: 'it reports the I/O saving' }]);
  assert.match(h.events(waiter).at(-1).text, /^Hibernating until ".+" does this: it reports the I\/O saving, or .+ at the latest\.$/);
  assert.deepEqual(h.runner.waitedOnFor(target).map((w) => w.threadId), [waiter]);
  turn(target, { needs: 'nothing' }, 'Restarted; waiting for the numbers.');
  assert.ok(h.runner.get(waiter).snoozedUntil, 'a finish alone does not count');
  turn(target, { needs: 'nothing' }, 'Saved 4.1 MB.');
  // An answer for another condition (one set since the look) doesn't count.
  assert.equal(h.runner.waitMet(waiter, target, 'it posted the saving', 'it reports 8 MB'), false);
  assert.equal(h.runner.waitMet(waiter, target, 'it posted the saving', 'it reports the I/O saving'), true);
  t = h.runner.get(waiter);
  assert.equal(t.snoozedUntil, undefined);
  const text = h.events(waiter).filter((e) => e.type === 'user.message').at(-1).text as string;
  assert.match(text, /has done what you waited for \(it reports the I\/O saving\): it posted the saving/);
  assert.match(text, /Saved 4\.1 MB\./);
  assert.equal(h.runner.waitMet(waiter, target, 'again', 'it reports the I/O saving'), false, 'only once');
});
