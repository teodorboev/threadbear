// How much of a free Convex plan this runner uses, and when full sync is asked about (src/convex-use.ts).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { askBrief, ConvexUseTally, estimate, FREE_IO_BYTES, FREE_STORAGE_BYTES, FullSyncOffer, fullSyncQuestion, fullSyncReason, shouldAskFullSync, WARN_AT, type FullSyncReason } from '../../src/convex-use.ts';

const DAY = 24 * 60 * 60_000;
const tmp = () => fs.mkdtempSync(path.join(os.tmpdir(), 'convex-use-'));

test('only a main runner with no answer yet may be asked, once', () => {
  const key = 'k';
  assert.equal(shouldAskFullSync({ syncOn: true, key, full: undefined, asked: undefined }), true);
  // Setup wrote it (a new install), or it was answered: never asked.
  assert.equal(shouldAskFullSync({ syncOn: true, key, full: true, asked: undefined }), false);
  assert.equal(shouldAskFullSync({ syncOn: true, key, full: false, asked: undefined }), false);
  // Asked before and never answered: it stays off, and isn't asked again.
  assert.equal(shouldAskFullSync({ syncOn: true, key, full: undefined, asked: '2026-11-13T00:00:00.000Z' }), false);
  // A second machine's runner key, or sync off: its owner answers on the main Mac.
  assert.equal(shouldAskFullSync({ syncOn: true, key: `tbr_${'ab'.repeat(16)}`, full: undefined, asked: undefined }), false);
  assert.equal(shouldAskFullSync({ syncOn: false, key, full: undefined, asked: undefined }), false);
  assert.equal(shouldAskFullSync({ syncOn: true, key: undefined, full: undefined, asked: undefined }), false);
});

test('the question names both answers, and the agent is told the pick sets it, not it', () => {
  const b = askBrief("'/x/tb'");
  assert.match(b.text, /Turn it on/);
  assert.match(b.text, /Leave it off/);
  assert.doesNotMatch(b.text, /settings full on/);
  assert.match(b.text, /the runner does that itself/);
  assert.match(b.text, /isn't a guarantee/);
});

test('light use warns nothing; a month heading past 70% of Free warns, as an estimate', () => {
  const quiet = estimate({ sent: 50e6, bodies: 20e6, since: 'x', days: 30 });
  assert.equal(quiet.io, 100e6);
  assert.equal(quiet.warning, undefined);
  const busy = estimate({ sent: (FREE_IO_BYTES * WARN_AT) / 2, bodies: 0, since: 'x', days: 30 });
  assert.match(busy.warning ?? '', /I\/O/);
  assert.match(busy.warning ?? '', /estimate/);
  assert.match(busy.warning ?? '', /"full": false/);
  assert.match(busy.warning ?? '', /usageWarning/);
  const stored = estimate({ sent: 0, bodies: (FREE_STORAGE_BYTES * WARN_AT) / 1.3 + 1, since: 'x', days: 30 });
  assert.match(stored.warning ?? '', /stored tool output/);
});

test("a busy first week is scaled to a month, so it warns before the month's out", () => {
  const week = estimate({ sent: 100e6, bodies: 0, since: 'x', days: 7 });
  assert.equal(week.io, Math.round(100e6 * 2 * (30 / 7)));
  assert.ok(week.warning);
  // A first day counts as a whole one, not a fraction that would blow it up.
  assert.equal(estimate({ sent: 1e6, bodies: 0, since: 'x', days: 0.01 }).io, 60e6);
});

test('the counts survive a restart and only the last 30 days count', () => {
  const home = tmp();
  let now = Date.parse('2026-11-01T12:00:00Z');
  const a = new ConvexUseTally({ home, now: () => now });
  a.record(1000, true);
  a.record(500, false);
  a.save();
  now += 10 * DAY;
  const b = new ConvexUseTally({ home, now: () => now });
  b.record(200, false);
  assert.deepEqual([b.use().sent, b.use().bodies], [1700, 1000]);
  assert.equal(b.use().since, '2026-11-01T00:00:00.000Z');
  now += 25 * DAY;
  assert.deepEqual([b.use().sent, b.use().bodies], [200, 0], 'the first day has dropped out');
  b.save();
  const saved = JSON.parse(fs.readFileSync(path.join(home, 'convex-use.json'), 'utf8')) as { days: Record<string, unknown> };
  assert.deepEqual(Object.keys(saved.days), ['2026-11-11']);
});

test('nothing sent leaves no file, and a broken file starts the count again', () => {
  const home = tmp();
  new ConvexUseTally({ home }).save();
  assert.equal(fs.existsSync(path.join(home, 'convex-use.json')), false);
  for (const broken of ['{not json', '{"days":null}', '{"days":[1]}', '{"days":{"x":{"sent":"a"},"2026-11-01":null}}', '{"days":{"2026-99-99":{"sent":1,"bodies":0}}}']) {
    fs.writeFileSync(path.join(home, 'convex-use.json'), broken);
    assert.equal(new ConvexUseTally({ home }).use().sent, 0, broken);
  }
});

test('what makes full sync useful: a live phone, then a browser, then another machine', () => {
  assert.equal(fullSyncReason({ devices: [], otherRunners: 0 }), undefined, 'one Mac on its own: nothing to gain');
  assert.equal(fullSyncReason({ devices: [{ kind: 'phone' }], otherRunners: 0 }), 'phone');
  assert.equal(fullSyncReason({ devices: [{ kind: 'browser' }], otherRunners: 0 }), 'browser');
  assert.equal(fullSyncReason({ devices: [], otherRunners: 1 }), 'machine');
  assert.equal(fullSyncReason({ devices: [{ kind: 'phone', expired: true }], otherRunners: 0 }), undefined, 'an expired token pairs nothing');
  assert.equal(fullSyncReason({ devices: [{ kind: 'browser' }, { kind: 'phone' }], otherRunners: 2 }), 'phone');
});

test('the question names what made it useful, with the benefit first', () => {
  assert.equal(fullSyncQuestion('phone', 'Mac'), "You've paired your phone. Want it to show this Mac's tool output and diffs too, even when the Mac's asleep?");
  assert.match(fullSyncQuestion('browser', 'Mac'), /^You've signed in on the website\./);
  assert.match(fullSyncQuestion('machine', 'machine'), /^You've added another machine\. .*this machine's tool output .*even when the machine's off\?$/);
  const b = askBrief("'/x/tb'", 'phone');
  assert.match(b.text, /You've paired your phone/);
  assert.match(b.text, /kept 30 days/);
});

function offer(start: { full?: boolean; asked?: string; key?: string } = {}) {
  const state = { full: start.full, asked: start.asked as string | undefined, allowed: true, noted: [] as (string | undefined)[], startedWithNote: [] as boolean[] };
  const started: FullSyncReason[] = [];
  const queued: (() => void)[] = [];
  let failNext = false;
  const o = new FullSyncOffer({
    read: () => ({ syncOn: true, key: start.key ?? 'k', full: state.full, asked: state.asked }),
    start: (reason) => {
      state.startedWithNote.push(state.asked !== undefined);
      if (failNext) {
        failNext = false;
        throw new Error('handing over');
      }
      started.push(reason);
      return `t${started.length}`;
    },
    note: (id, clear) => {
      state.asked = clear ? undefined : '2026-11-13T00:00:00.000Z';
      state.noted.push(clear ? 'cleared' : id);
    },
    later: (fn) => void queued.push(fn),
    delayMs: 1,
    allowed: () => state.allowed,
  });
  return { o, state, started, run: () => queued.splice(0).forEach((f) => f()), failOnce: () => (failNext = true) };
}

test('each trigger asks, and only the first: a second pairing or machine asks nothing', () => {
  for (const reason of ['phone', 'browser', 'machine'] as const) {
    const t = offer();
    t.o.trigger(reason);
    t.o.trigger('machine');
    t.run();
    t.o.trigger('phone');
    t.run();
    assert.deepEqual(t.started, [reason], `${reason} asks once`);
  }
});

test('nothing to gain asks nothing, and an install with the setting either way is never asked', () => {
  const none = offer();
  none.o.trigger(undefined);
  none.run();
  assert.deepEqual(none.started, []);
  for (const full of [true, false]) {
    const t = offer({ full });
    t.o.trigger('phone');
    t.run();
    assert.deepEqual(t.started, [], `sync.full ${full} is left as it is`);
    assert.equal(t.state.full, full);
  }
  const keyed = offer({ key: `tbr_${'ab'.repeat(16)}` });
  keyed.o.trigger('machine');
  keyed.run();
  assert.deepEqual(keyed.started, [], "a second machine's runner key never asks");
});

test("answered while it waited, it doesn't ask; a refused thread lets a later trigger ask", () => {
  const t = offer();
  t.o.trigger('phone');
  t.state.full = false;
  t.run();
  assert.deepEqual(t.started, []);
  const r = offer();
  r.failOnce();
  r.o.trigger('phone');
  r.run();
  assert.deepEqual(r.started, [], 'a runner handing over refuses the thread');
  r.o.trigger('browser');
  r.run();
  assert.deepEqual(r.started, ['browser']);
});

test('it notes before opening the thread, so a crash in between asks never, not twice', () => {
  const t = offer();
  t.o.trigger('phone');
  t.run();
  assert.deepEqual(t.state.startedWithNote, [true], 'already noted when the thread opens');
  assert.deepEqual(t.state.noted, [undefined, 't1'], 'then the thread that may answer is recorded');
  // A thread that won't open (handing over) takes the note back, so a later trigger asks.
  const r = offer();
  r.failOnce();
  r.o.trigger('phone');
  r.run();
  assert.deepEqual(r.state.noted, [undefined, 'cleared']);
  assert.equal(r.state.asked, undefined);
});

test('while the runner is stopping or handing over it waits, and a later trigger asks', () => {
  const t = offer();
  t.state.allowed = false;
  t.o.trigger('phone');
  t.run();
  assert.deepEqual(t.started, []);
  assert.equal(t.state.asked, undefined, 'not noted');
  t.state.allowed = true;
  t.o.trigger('machine');
  t.run();
  assert.deepEqual(t.started, ['machine']);
});

test("an offer waiting to open isn't the end: only asking or an answer is", () => {
  const t = offer();
  t.o.trigger('phone');
  assert.equal(t.o.open, false, 'busy while it waits');
  assert.equal(t.o.eligible, true, 'but still eligible, so the hourly look keeps going');
  t.failOnce();
  t.run();
  assert.equal(t.o.eligible, true, 'a thread that would not open leaves it eligible');
  t.o.trigger('phone');
  t.run();
  assert.equal(t.o.eligible, false, 'asked: done for good');
});
