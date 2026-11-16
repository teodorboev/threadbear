// A message's, tool body's or picture's seq is a whole number from 0 to
// MAX_SEQ (convex/lib.ts isSeq; Daybreak finding 134, 15 Nov 2026):
// v.number() takes NaN, the infinities, fractions and numbers past 2^53,
// which a runner key could store and the main Mac's relay then sorted,
// keyed and multiplied into event ids. Against the in-memory database.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { call, load, memoryBackend, outcome, THREAD_BASICS } from '../helpers/convex-memory.ts';

const OWNER = 'owner-key';
const MINI_KEY = 'tbr_mac-mini';
const MINI = 'cb921b2447ea';

const { sha256, MAX_SEQ } = load('lib') as { sha256: (s: string) => Promise<string>; MAX_SEQ: number };
const sync = load('sync');
const relay = load('relay');

const BAD = [NaN, Infinity, -Infinity, 1.5, -1, 2 ** 53, Number.MAX_SAFE_INTEGER, MAX_SEQ + 1];

async function setup() {
  process.env.RUNNER_KEY = OWNER;
  delete process.env.RUNNER_KEY_NEXT;
  process.env.CONVEX_SITE_URL = 'https://site.example';
  const b = memoryBackend();
  await b.ctx.db.insert('runnerKeys', { runnerId: MINI, keyHash: await sha256(MINI_KEY), name: 'mac-mini', createdAt: 1 });
  await b.ctx.db.insert('runners', { runnerId: MINI, name: 'mac-mini', version: 'v', startedAt: 'x', draining: false, threadCount: 1, lastSeenAt: Date.now() });
  await call(b.ctx, sync.upsertThreads, { key: MINI_KEY, threads: [{ threadId: 'T', runnerId: MINI, ...THREAD_BASICS, title: 'T', view: 'inbox', needsYou: false }] });
  return b;
}

for (const [who, key] of [
  ['a runner key', MINI_KEY],
  ['the main key', OWNER],
] as const) {
  test(`${who}'s messages and tool bodies under a bad seq are dropped, and the rest of the batch goes in`, async () => {
    const s = await setup();
    const messages = [...BAD.map((seq) => ({ threadId: 'T', seq, role: 'assistant', text: `bad ${seq}`, at: 'now' })), { threadId: 'T', seq: 0, role: 'user', text: 'first', at: 'now' }, { threadId: 'T', seq: MAX_SEQ, role: 'assistant', text: 'last', at: 'now' }];
    await call(s.ctx, sync.appendMessages, { key, messages });
    assert.deepEqual(s.rows('messages').map((m) => [m.seq, m.text]), [[0, 'first'], [MAX_SEQ, 'last']]);
    await call(s.ctx, sync.appendBodies, { key, bodies: [...BAD.map((seq) => ({ threadId: 'T', seq, text: 'bad' })), { threadId: 'T', seq: 3, text: 'good' }] });
    assert.deepEqual(s.rows('eventBodies').map((b) => [b.seq, b.text]), [[3, 'good']]);
  });
}

test("a runner key's picture under a bad seq is refused", async () => {
  const s = await setup();
  for (const seq of BAD) {
    const storageId = s.file(`f${seq}`);
    await s.ctx.db.insert('storageOwners', { storageId, runnerId: MINI, at: Date.now(), bytes: 1 });
    for (const fn of [sync.addReplyImage, sync.addShareImage]) {
      assert.equal(await outcome(call(s.ctx, fn, { key: MINI_KEY, threadId: 'T', seq, path: 'a.png', storageId, mediaType: 'image/png' })), 'badArgs', `${seq}`);
    }
  }
});

test("rows stored under a bad seq before it was checked are left out of the relay's pages, and the rest still come", async () => {
  const s = await setup();
  for (const seq of [1, 2, 3]) await s.ctx.db.insert('messages', { threadId: 'T', seq, role: 'assistant', text: `m${seq}`, at: 'now', runnerId: MINI });
  for (const seq of BAD) await s.ctx.db.insert('messages', { threadId: 'T', seq, role: 'assistant', text: 'bad', at: 'now', runnerId: MINI });
  for (const notes of [undefined, true]) {
    const page = await call(s.ctx, relay.remoteMessages, { key: OWNER, threadId: 'T', ...(notes ? { notes } : {}) });
    assert.deepEqual(page.messages.map((m: { seq: number }) => m.seq), [1, 2, 3]);
    const after = await call(s.ctx, relay.remoteMessages, { key: OWNER, threadId: 'T', afterSeq: 1, ...(notes ? { notes } : {}) });
    assert.deepEqual(after.messages.map((m: { seq: number }) => m.seq), [2, 3]);
    const tail = await call(s.ctx, relay.remoteTail, { key: OWNER, threadId: 'T', ...(notes ? { notes } : {}) });
    assert.deepEqual(tail.messages.map((m: { seq: number }) => m.seq), [1, 2, 3]);
  }
});

test("a long run of rows stored before seqs were checked (Grok's review) doesn't leave the relay's pages empty", async () => {
  const s = await setup();
  for (const seq of [1, 2, 3]) await s.ctx.db.insert('messages', { threadId: 'T', seq, role: 'assistant', text: `m${seq}`, at: 'now', runnerId: MINI });
  for (const seq of [Infinity, -1, 2 ** 53]) for (let i = 0; i < 600; i++) await s.ctx.db.insert('messages', { threadId: 'T', seq: seq === -1 ? -1 - i : seq, role: 'assistant', text: 'x', at: 'now', runnerId: MINI });
  const tail = await call(s.ctx, relay.remoteTail, { key: OWNER, threadId: 'T' });
  assert.deepEqual(tail.messages.map((m: { seq: number }) => m.seq), [1, 2, 3]);
  assert.equal(tail.hasMore, false);
  const first = await call(s.ctx, relay.remoteMessages, { key: OWNER, threadId: 'T' });
  assert.deepEqual(first.messages.map((m: { seq: number }) => m.seq), [1, 2, 3]);
  const after = await call(s.ctx, relay.remoteMessages, { key: OWNER, threadId: 'T', afterSeq: 2 });
  assert.deepEqual(after.messages.map((m: { seq: number }) => m.seq), [3]);
  assert.equal(after.hasMore, false);
  const before = await call(s.ctx, relay.remoteMessages, { key: OWNER, threadId: 'T', beforeSeq: 3 });
  assert.deepEqual(before.messages.map((m: { seq: number }) => m.seq), [1, 2]);
});
