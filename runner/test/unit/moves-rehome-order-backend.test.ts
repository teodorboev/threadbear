// A move's rows going to its new runner a page at a time (moves:rehomeRows)
// can't reach past the move (Daybreak finding 132, 15 Nov 2026): the move
// ends only once every page has run, so the thread can't start its next
// move before then; each page is its move's and does nothing once that
// move has ended; and a runner key's quota never deletes a row of a thread
// that's now another runner's. Played against the in-memory database, with
// the scheduled pages run in whatever order the test says.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { call, claimAndFinish, load, memoryBackend, outcome, THREAD_BASICS } from '../helpers/convex-memory.ts';

const OWNER = 'owner-key';
const MINI_KEY = 'tbr_mac-mini';
const CLOUD_KEY = 'tbr_cloud';
const LAPTOP = 'laptop000001'; // the main Mac (the owner key's): U
const MINI = 'cb921b2447ea'; // T
const CLOUD = 'befe8df8975c'; // S
const SEAL = 'A'.repeat(43) + '=';
const SHA = 'a'.repeat(64);
const ROWS = 450; // three pages of 200

const { sha256 } = load('lib') as { sha256: (s: string) => Promise<string> };
const sync = load('sync');
const moves = load('moves');

async function setup() {
  process.env.RUNNER_KEY = OWNER;
  delete process.env.RUNNER_KEY_NEXT;
  process.env.CONVEX_SITE_URL = 'https://site.example';
  const b = memoryBackend();
  await b.ctx.db.insert('runnerKeys', { runnerId: MINI, keyHash: await sha256(MINI_KEY), name: 'mac-mini', createdAt: 1 });
  await b.ctx.db.insert('runnerKeys', { runnerId: CLOUD, keyHash: await sha256(CLOUD_KEY), name: 'cloud', createdAt: 1 });
  for (const [runnerId, name] of [
    [LAPTOP, 'Laptop'],
    [MINI, 'mac-mini'],
    [CLOUD, 'cloud'],
  ] as const) {
    await b.ctx.db.insert('runners', { runnerId, name, version: '1', lastSeenAt: Date.now(), startedAt: new Date().toISOString(), draining: false, threadCount: 0 });
  }
  const fn = (f: unknown, args: Record<string, unknown>) => call(b.ctx, f as never, args);
  const as = (key: string) => ({
    start: (threadId: string, to: string) => fn(moves.startMove, { key, threadId, to }),
    info: (moveId: string) => fn(moves.moveInfo, { key, moveId, info: { cwd: '/x' } }),
    ready: (moveId: string) => fn(moves.moveReady, { key, moveId, plan: { cwd: '/x' }, sealKey: SEAL }),
    frozen: (moveId: string) => fn(moves.moveFrozen, { key, moveId }),
    // A runner key's chunk goes up through /runner/upload, then the move is packed with it.
    pack: async (moveId: string) => {
      const ticket = new URL(await fn(moves.moveChunkUrl, { key, moveId })).searchParams.get('ticket');
      const id = b.file(`chunk-${moveId}`);
      assert.equal(await claimAndFinish(b.ctx, sync.claimUpload, sync.finishUpload, { ticket, storageId: id, bytes: 1000 }), true);
      return fn(moves.movePacked, { key, moveId, chunks: [id], sha256: SHA, bytes: 1000 });
    },
    claim: (moveId: string) => fn(moves.moveClaim, { key, moveId }),
    done: (moveId: string) => fn(moves.moveDone, { key, moveId }),
    upsert: (...threads: Record<string, unknown>[]) => fn(sync.upsertThreads, { key, threads }),
    append: (messages: Record<string, unknown>[]) => fn(sync.appendMessages, { key, messages }),
    bodies: (bodies: Record<string, unknown>[]) => fn(sync.appendBodies, { key, bodies }),
  });
  // The rehome pages scheduled and not yet run, taken out to run when the test says.
  const pages = () => {
    const out: Record<string, unknown>[] = [];
    for (let i = b.scheduled.length - 1; i >= 0; i--) {
      const job = b.scheduled[i] as { fn: string; args: Record<string, unknown> };
      if (job.fn.endsWith('rehomeRows')) out.unshift(...(b.scheduled.splice(i, 1) as typeof job[]).map((j) => j.args));
    }
    return out;
  };
  const page = (args: Record<string, unknown>) => fn(moves.rehomeRows, args);
  const runAll = async () => {
    for (let i = 0; i < 100; i++) {
      const next = pages();
      if (!next.length) return;
      for (const a of next) await page(a);
    }
    throw new Error('rehomeRows never finished');
  };
  return { ...b, pages, page, runAll, owner: as(OWNER), mini: as(MINI_KEY), cloud: as(CLOUD_KEY) };
}
type S = Awaited<ReturnType<typeof setup>>;

const thread = (threadId: string, runnerId: string) => ({ threadId, runnerId, ...THREAD_BASICS, title: 'T', view: 'inbox', needsYou: false });
const moveOf = (s: S, moveId: string) => s.rows('threadMoves').find((m) => m.moveId === moveId)!;
const owners = (s: S) => new Set(s.rows('messages').filter((m) => m.threadId === 'T').map((m) => m.runnerId ?? 'owner'));

// A thread on Cloud with ROWS messages, moved to Mac mini up to its switch.
async function claimedOnMini(s: S): Promise<string> {
  await s.cloud.upsert(thread('T', CLOUD));
  for (let i = 0; i < ROWS; i += 50) await s.cloud.append(Array.from({ length: 50 }, (_, k) => ({ threadId: 'T', seq: i + k + 1, role: 'assistant', text: `m${i + k + 1}`, at: 'now' })));
  const { moveId } = await s.owner.start('T', MINI);
  await s.cloud.info(moveId);
  await s.mini.ready(moveId);
  await s.cloud.frozen(moveId);
  await s.cloud.pack(moveId);
  await s.mini.claim(moveId);
  return moveId;
}

async function afterMinutes<T>(n: number, f: () => Promise<T>): Promise<T> {
  const realNow = Date.now;
  Date.now = () => realNow() + n * 60_000;
  try {
    return await f();
  } finally {
    Date.now = realNow;
  }
}

async function toMain(s: S): Promise<string> {
  const { moveId } = await s.owner.start('T', LAPTOP);
  await s.mini.info(moveId);
  await s.owner.ready(moveId);
  await s.mini.frozen(moveId);
  await s.mini.pack(moveId);
  await s.owner.claim(moveId);
  return moveId;
}

test("a move T says is done stays claimed until its last page has run, so the thread's next move waits", async () => {
  const s = await setup();
  const a = await claimedOnMini(s);
  // One page of three has run.
  const [first] = s.pages();
  await s.page(first!);
  await s.mini.done(a);
  assert.equal(moveOf(s, a).state, 'claimed', 'not done while pages are still to run');
  assert.equal(await outcome(s.owner.start('T', LAPTOP)), 'conflict', 'no next move meanwhile');
  await s.runAll();
  assert.equal(moveOf(s, a).state, 'done', 'the last page ends it');
  assert.deepEqual([...owners(s)], [MINI]);
  assert.equal(await outcome(s.owner.start('T', LAPTOP)), 'ok');
});

test('a page of an earlier move that runs after the next move changes nothing', async () => {
  const s = await setup();
  const a = await claimedOnMini(s);
  // A's first page runs; its second is held back, as Convex may run it late.
  const [first] = s.pages();
  await s.page(first!);
  const [late] = s.pages();
  assert.ok(late, 'a second page');
  await s.mini.done(a);
  assert.equal(moveOf(s, a).state, 'claimed', "the held page's rows are still Cloud's");
  assert.ok(owners(s).has(CLOUD));
  // Ten minutes on, the stuck check sends A's rows again, and the last page ends it.
  await afterMinutes(11, () => call(s.ctx, moves.expireMoves, {}));
  await s.runAll();
  assert.equal(moveOf(s, a).state, 'done');
  assert.deepEqual([...owners(s)], [MINI]);
  // Move B to the main Mac.
  const b = await toMain(s);
  await s.runAll();
  await s.owner.done(b);
  assert.equal(moveOf(s, b).state, 'done');
  assert.deepEqual([...owners(s)], ['owner']);
  // A's held page runs at last, and so does one from before pages named
  // their move: neither gives anything back to Mac mini.
  await s.page(late!);
  const { moveId: _m, ...legacy } = late as Record<string, unknown>;
  await s.page({ ...legacy, cursor: null, table: 0 });
  assert.deepEqual([...owners(s)], ['owner'], 'still the main Mac’s');
  assert.equal(s.pages().length, 0, 'and nothing more scheduled');
  assert.equal(s.rows('runnerStorage').find((r) => r.runnerId === MINI)?.messageBytes ?? 0, 0, "Mac mini isn't charged for rows it doesn't have");
});

test('a claimed move T said was done whose pages were lost is rehomed again by the stuck check, and ends', async () => {
  const s = await setup();
  const a = await claimedOnMini(s);
  s.pages(); // lost
  await s.mini.done(a);
  assert.equal(moveOf(s, a).state, 'claimed');
  await afterMinutes(11, () => call(s.ctx, moves.expireMoves, {}));
  await s.runAll();
  assert.equal(moveOf(s, a).state, 'done');
  assert.deepEqual([...owners(s)], [MINI]);
});

test("a runner key's quota never deletes a row of a thread that's another runner's now: it hands it over", async () => {
  const s = await setup();
  // Rows of Cloud's thread left under Mac mini's id (what a late page of an old move would have left).
  await s.cloud.upsert(thread('T', CLOUD));
  await s.cloud.append([{ threadId: 'T', seq: 1, role: 'assistant', text: 'cloud’s', at: 'now' }]);
  await s.cloud.bodies([{ threadId: 'T', seq: 1, text: 'cloud’s body' }]);
  for (const t of ['messages', 'eventBodies']) for (const r of s.rows(t).filter((x) => x.threadId === 'T')) await s.ctx.db.patch(r._id, { runnerId: MINI });
  // Mac mini at the top of its allowances, writing to a thread of its own.
  await s.mini.upsert(thread('M', MINI));
  const usage = s.rows('runnerStorage').find((r) => r.runnerId === MINI);
  if (usage) await s.ctx.db.patch(usage._id, { messageBytes: 48 * 1024 * 1024, bodyBytes: 32 * 1024 * 1024 });
  else await s.ctx.db.insert('runnerStorage', { runnerId: MINI, bytes: 0, messageBytes: 48 * 1024 * 1024, bodyBytes: 32 * 1024 * 1024 });
  await s.mini.append([{ threadId: 'M', seq: 1, role: 'assistant', text: 'mini’s', at: 'now' }]);
  await s.mini.bodies([{ threadId: 'M', seq: 1, text: 'mini’s body' }]);
  // Cloud's rows are still there, and Cloud's again.
  assert.deepEqual(s.rows('messages').filter((m) => m.threadId === 'T').map((m) => [m.text, m.runnerId]), [['cloud’s', CLOUD]]);
  assert.deepEqual(s.rows('eventBodies').filter((m) => m.threadId === 'T').map((m) => [m.text, m.runnerId]), [['cloud’s body', CLOUD]]);
});

test('a runner key sending a row of its own thread again takes it back from a runner it was left under', async () => {
  const s = await setup();
  await s.cloud.upsert(thread('T', CLOUD));
  await s.cloud.append([{ threadId: 'T', seq: 1, role: 'assistant', text: 'hi', at: 'now' }]);
  await s.cloud.bodies([{ threadId: 'T', seq: 1, text: 'body' }]);
  for (const t of ['messages', 'eventBodies']) for (const r of s.rows(t).filter((x) => x.threadId === 'T')) await s.ctx.db.patch(r._id, { runnerId: MINI });
  await s.cloud.append([{ threadId: 'T', seq: 1, role: 'assistant', text: 'hi', at: 'now' }]);
  await s.cloud.bodies([{ threadId: 'T', seq: 1, text: 'body', ok: true }]);
  assert.equal(s.rows('messages').find((m) => m.threadId === 'T')!.runnerId, CLOUD);
  assert.equal(s.rows('eventBodies').find((m) => m.threadId === 'T')!.runnerId, CLOUD);
});
