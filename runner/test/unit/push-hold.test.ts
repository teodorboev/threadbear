// A push held while Mike is at the Mac (convex/push.ts notify, holdPush) goes
// when he leaves (sync:setPresence). A Mac whose lid closed never says he
// left, as its runner sleeps with it, so the push waited for good and the
// phone never buzzed (13 Nov 2026, the implicit state machines scan). The
// cron sends the held pushes of a runner that said he's there but has
// stopped checking in.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { call, load, memoryBackend, THREAD_BASICS } from '../helpers/convex-memory.ts';

const sync = load('sync');
const { PRESENCE_FRESH_MS } = load('lib') as { PRESENCE_FRESH_MS: number };

async function world(lastSeenAgo: number) {
  const b = memoryBackend();
  const now = Date.now();
  await b.ctx.db.insert('runners', { runnerId: 'mac1', name: 'M5', version: '1', lastSeenAt: now - lastSeenAgo, startedAt: new Date().toISOString(), draining: false, threadCount: 2, atMac: true, atMacAt: now - lastSeenAgo });
  await b.ctx.db.insert('threads', { threadId: 'needs', runnerId: 'mac1', ...THREAD_BASICS, title: 'Needs him', view: 'inbox', needsYou: true, pushHeldAt: now - lastSeenAgo });
  await b.ctx.db.insert('threads', { threadId: 'seen', runnerId: 'mac1', ...THREAD_BASICS, title: 'Seen since', view: 'inbox', needsYou: false, pushHeldAt: now - lastSeenAgo });
  return b;
}

const notified = (b: ReturnType<typeof memoryBackend>) => b.scheduled.filter((s: any) => String(s.fn).endsWith('push.notify')).map((s: any) => s.args);

// The cron, then the drains it hands on, as the scheduler would run them.
async function cron(b: ReturnType<typeof memoryBackend>) {
  const from = b.scheduled.length;
  await call(b.ctx, sync.sendStaleHeldPushes, {});
  for (let i = from; i < b.scheduled.length; i++) {
    const s = b.scheduled[i] as any;
    if (String(s.fn).endsWith('drainHeldPushes')) await call(b.ctx, sync.drainHeldPushes, s.args);
  }
}

test("a held push goes once its Mac has stopped checking in, as one whose lid closed with Mike at it", async () => {
  const b = await world(PRESENCE_FRESH_MS + 60_000);
  await cron(b);
  assert.deepEqual(notified(b), [{ threadId: 'needs', held: true }]);
  assert.ok(b.rows('threads').every((t: any) => t.pushHeldAt === undefined), 'nothing still held');
  assert.equal(b.rows('runners')[0].atMac, true, 'he is there again when it wakes and checks in');
  await cron(b);
  assert.equal(notified(b).length, 1, 'once');
});

test('a Mac still checking in with Mike at it keeps its pushes held', async () => {
  const b = await world(30_000);
  await cron(b);
  assert.deepEqual(notified(b), []);
  assert.ok(b.rows('threads').every((t: any) => t.pushHeldAt !== undefined));
});


// Daybreak 119 (13 Nov 2026): a secondary runner's key may keep 3,000 rows
// of up to 8 KB and hold a push on each (needsYou flipped ten a minute while
// it says Mike is there). The cron read every held row of every stale runner
// in one mutation: past Convex's 16 MiB, so it failed every two minutes and
// another Mac's held push never went. Now the cron only hands each runner's
// drain to a mutation of its own, and each drain reads a small batch.
test("a runner's huge held backlog can't stop another Mac's held push, and drains a batch at a time", async () => {
  const b = memoryBackend();
  const now = Date.now();
  const stale = now - PRESENCE_FRESH_MS - 60_000;
  const runner = (runnerId: string) => b.ctx.db.insert('runners', { runnerId, name: runnerId, version: '1', lastSeenAt: stale, startedAt: new Date().toISOString(), draining: false, threadCount: 1, atMac: true, atMacAt: stale });
  await runner('bad');
  await runner('mac1');
  const big = 'q'.repeat(7_600);
  for (let i = 0; i < 3000; i++) await b.ctx.db.insert('threads', { threadId: `bad-${i}`, runnerId: 'bad', ...THREAD_BASICS, title: 'x', asked: big, view: 'den', needsYou: true, pushHeldAt: stale });
  await b.ctx.db.insert('threads', { threadId: 'needs', runnerId: 'mac1', ...THREAD_BASICS, title: 'Needs him', view: 'inbox', needsYou: true, pushHeldAt: stale });
  const LIMIT = 16 * 1024 * 1024;
  const run = async (fn: any, args: Record<string, unknown>) => {
    b.resetReads();
    await call(b.ctx, fn, args);
    assert.ok(b.bytesRead() < 1024 * 1024, `${b.bytesRead()} bytes read in one mutation`);
    assert.ok(b.bytesRead() < LIMIT);
  };
  await run(sync.sendStaleHeldPushes, {});
  const drains = () => b.scheduled.filter((s: any) => String(s.fn).endsWith('drainHeldPushes'));
  assert.deepEqual(drains().map((s: any) => s.args.runnerId).sort(), ['bad', 'mac1'], 'one drain each');
  assert.deepEqual(notified(b), [], 'the cron itself sends nothing');
  // Each runner's drain on its own: mac1's push goes whatever bad's does.
  await run(sync.drainHeldPushes, { runnerId: 'mac1' });
  assert.deepEqual(notified(b), [{ threadId: 'needs', held: true }]);
  // bad's goes a batch at a time, each scheduling the next, until none are held.
  let done = 0;
  while (done < drains().length) {
    const next = drains()[done++] as any;
    if (next.args.runnerId !== 'bad') continue;
    await run(sync.drainHeldPushes, next.args);
  }
  assert.equal(notified(b).length, 3001);
  assert.ok(b.rows('threads').every((t: any) => t.pushHeldAt === undefined));
  // A cron with nothing held hands nothing on.
  const before = drains().length;
  await run(sync.sendStaleHeldPushes, {});
  assert.equal(drains().length, before);
});
