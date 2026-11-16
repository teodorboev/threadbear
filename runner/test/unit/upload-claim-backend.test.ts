// The upload routes claim their one-use ticket before reading a byte
// (convex/uploads.ts takeUpload; Daybreak finding 128, 15 Nov 2026): a
// read-only check first let many requests with one ticket each read and
// store a body before the one redemption that counted, the rest thrown
// away uncounted. Each test plays a route as convex/http.ts does, with the
// backend's real claim and finish mutations over the in-memory database.
// Convex runs mutations one at a time as far as any one can tell, so the
// action's mutations here go through one at a time too.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { call, load, memoryBackend } from '../helpers/convex-memory.ts';

const OWNER = 'owner-key';
const MINI_KEY = 'tbr_mac-mini';
const MINI = 'cb921b2447ea';
const TOKEN = 'phone-token';
const DAY = 24 * 60 * 60_000;
const MB = 1024 * 1024;

const { sha256 } = load('lib') as { sha256: (s: string) => Promise<string> };
const sync = load('sync');
const phone = load('phone');
const relay = load('relay');
const { takeUpload } = load('uploads');

async function setup() {
  process.env.RUNNER_KEY = OWNER;
  delete process.env.RUNNER_KEY_NEXT;
  process.env.CONVEX_SITE_URL = 'https://site.example';
  const b = memoryBackend();
  await b.ctx.db.insert('runnerKeys', { runnerId: MINI, keyHash: await sha256(MINI_KEY), name: 'mac-mini', createdAt: 1 });
  const now = Date.now();
  const tokenId = await b.ctx.db.insert('tokens', { hash: await sha256(TOKEN), name: 'iphone', kind: 'phone', expiresAt: now + 100 * DAY, createdAt: now });
  await b.ctx.db.insert('runners', { runnerId: MINI, name: 'mac-mini', version: 'v', startedAt: 'x', draining: false, threadCount: 1, lastSeenAt: now });
  return { ...b, tokenId };
}
type S = Awaited<ReturnType<typeof setup>>;

// An HTTP action's ctx: the real mutations, one at a time, with a hook to fail one.
function actionCtx(s: S, fail?: (ref: string) => boolean) {
  let chain: Promise<unknown> = Promise.resolve();
  const fns: Record<string, Record<string, any>> = { sync, phone, relay };
  const stores: string[] = [];
  return {
    stores,
    ctx: {
      runMutation(ref: unknown, args: Record<string, unknown>) {
        const [, mod, name] = String(ref).split('.');
        const run = async () => {
          if (fail?.(String(ref))) throw new Error('the mutation failed');
          return call(s.ctx, fns[mod!]![name!], args);
        };
        const p = chain.then(run, run);
        chain = p.catch(() => undefined);
        return p;
      },
      storage: {
        async store(blob: Blob) {
          const id = await s.ctx.storage.store(blob);
          stores.push(id);
          return id;
        },
        delete: (id: string) => s.ctx.storage.delete(id),
      },
    },
  };
}

// A body of `n` bytes that counts how much of it the route read (nothing is
// pulled ahead of a read), and can break off partway.
function body(n: number, opts: { breakAt?: number } = {}) {
  const seen = { bytes: 0 };
  let left = n;
  const stream = new ReadableStream<Uint8Array>(
    {
      pull(c) {
        if (opts.breakAt !== undefined && seen.bytes >= opts.breakAt) return c.error(new Error('connection reset'));
        if (left <= 0) return c.close();
        const k = Math.min(left, 256 * 1024);
        left -= k;
        seen.bytes += k;
        c.enqueue(new Uint8Array(k));
      },
    },
    { highWaterMark: 0 },
  );
  return { stream, seen };
}

// What takeUpload reads of a Request (a real one buffers its body ahead of the reader).
function post(url: string, stream: ReadableStream<Uint8Array>, headers: Record<string, string> = {}): Request {
  return { url, method: 'POST', body: stream, headers: new Headers({ 'Content-Type': 'image/png', ...headers }) } as unknown as Request;
}

type Route = { name: string; url: (s: S) => Promise<string>; claim: unknown; finish: unknown; maxBytes: number };
const ROUTES: Route[] = [
  { name: '/runner/upload', url: (s) => call(s.ctx, sync.iconUploadUrl, { key: MINI_KEY }), claim: 'internal.sync.claimUpload', finish: 'internal.sync.finishUpload', maxBytes: 10 * MB },
  { name: '/phone/upload', url: (s) => call(s.ctx, phone.imageUploadUrl, { token: TOKEN }), claim: 'internal.phone.claimPhoneUpload', finish: 'internal.phone.finishPhoneUpload', maxBytes: 10 * MB },
  { name: '/relay/upload', url: (s) => call(s.ctx, relay.pictureUploadUrl, { key: OWNER }), claim: 'internal.relay.claimRelayUpload', finish: 'internal.relay.finishRelayUpload', maxBytes: 10 * MB },
];
const take = (a: ReturnType<typeof actionCtx>, r: Route, req: Request) => takeUpload(a.ctx, req, { maxBytes: r.maxBytes, claim: r.claim, finish: r.finish, refused: 'refused' }) as Promise<Response>;

for (const r of ROUTES) {
  test(`${r.name}: of many requests with one ticket at once, only one reads its body or stores anything`, async () => {
    const s = await setup();
    const url = await r.url(s);
    const a = actionCtx(s);
    const bodies = Array.from({ length: 8 }, () => body(2 * MB));
    const answers = await Promise.all(bodies.map((b) => take(a, r, post(url, b.stream))));
    assert.deepEqual(answers.map((x) => x.status).sort(), [200, 403, 403, 403, 403, 403, 403, 403]);
    assert.equal(bodies.filter((b) => b.seen.bytes > 0).length, 1, 'one body read');
    assert.equal(a.stores.length, 1, 'one file stored');
    assert.equal(s.hasFile(a.stores[0]!), true);
    const won = (await answers.find((x) => x.status === 200)!.json()) as { storageId: string };
    assert.equal(won.storageId, a.stores[0]);
  });

  test(`${r.name}: a body bigger than the cap is stopped at the cap, whatever Content-Length says, and the ticket is spent`, async () => {
    for (const said of [undefined, 'nonsense', '10', '-5', ' 12 ']) {
      const s = await setup();
      const url = await r.url(s);
      const a = actionCtx(s);
      const b = body(r.maxBytes + MB);
      const answer = await take(a, r, post(url, b.stream, said === undefined ? {} : { 'Content-Length': said }));
      assert.equal(answer.status, 413, `Content-Length ${said}`);
      assert.ok(b.seen.bytes <= r.maxBytes + 256 * 1024, `read ${b.seen.bytes} bytes with Content-Length ${said}`);
      assert.equal(a.stores.length, 0, 'nothing stored');
      // Spent: the same ticket again with a small body is refused before reading it.
      const again = body(10);
      assert.equal((await take(a, r, post(url, again.stream))).status, 403);
      assert.equal(again.seen.bytes, 0);
    }
  });

  test(`${r.name}: an honest Content-Length over the cap is refused before the ticket is claimed`, async () => {
    const s = await setup();
    const url = await r.url(s);
    const a = actionCtx(s);
    const big = body(r.maxBytes + 1);
    assert.equal((await take(a, r, post(url, big.stream, { 'Content-Length': String(r.maxBytes + 1) }))).status, 413);
    assert.equal(big.seen.bytes, 0);
    const ok = body(1000);
    assert.equal((await take(a, r, post(url, ok.stream))).status, 200, 'the ticket is still good');
  });

  test(`${r.name}: a body that breaks off spends the ticket and stores nothing`, async () => {
    const s = await setup();
    const url = await r.url(s);
    const a = actionCtx(s);
    const b = body(2 * MB, { breakAt: MB });
    assert.equal((await take(a, r, post(url, b.stream))).status, 400);
    assert.equal(a.stores.length, 0);
    const again = body(10);
    assert.equal((await take(a, r, post(url, again.stream))).status, 403);
    assert.equal(again.seen.bytes, 0);
  });

  test(`${r.name}: a finish that fails takes the stored file with it`, async () => {
    const s = await setup();
    const url = await r.url(s);
    const a = actionCtx(s, (ref) => ref === r.finish);
    await assert.rejects(take(a, r, post(url, body(1000).stream)), /the mutation failed/);
    assert.equal(a.stores.length, 1);
    assert.equal(s.hasFile(a.stores[0]!), false, 'deleted');
  });

  test(`${r.name}: a bad ticket reads nothing`, async () => {
    const s = await setup();
    const a = actionCtx(s);
    const url = await r.url(s);
    for (const bad of ['', 'nope', 'f'.repeat(48)]) {
      const b = body(1000);
      const u = new URL(url);
      u.searchParams.set('ticket', bad);
      assert.equal((await take(a, r, post(u.href, b.stream))).status, 403);
      assert.equal(b.seen.bytes, 0, bad);
    }
    assert.equal(a.stores.length, 0);
  });

  test(`${r.name}: a finish needs the claim it was given, so a request that didn't claim can't store with the ticket`, async () => {
    const s = await setup();
    const url = await r.url(s);
    const ticket = new URL(url).searchParams.get('ticket');
    const mod = { sync, phone, relay }[String(r.claim).split('.')[1]!]!;
    const finish = mod[String(r.finish).split('.')[2]!];
    const claimed = await call(s.ctx, mod[String(r.claim).split('.')[2]!], { ticket });
    assert.equal(typeof claimed.claim, 'string');
    assert.equal(await call(s.ctx, finish, { ticket, claim: 'f'.repeat(32), storageId: s.file('x'), bytes: 10 }), false, 'a guessed claim');
    assert.equal(await call(s.ctx, finish, { ticket, claim: claimed.claim, storageId: s.file('y'), bytes: 10 }), true);
    assert.equal(await call(s.ctx, finish, { ticket, claim: claimed.claim, storageId: s.file('z'), bytes: 10 }), false, 'finished once only');
  });
}

test('/runner/upload: a ticket whose key was revoked, that has run out, or whose move ended reads nothing', async () => {
  const r = ROUTES[0]!;
  // Revoked.
  {
    const s = await setup();
    const url = await r.url(s);
    await call(s.ctx, sync.revokeRunnerKey, { key: OWNER, runnerId: MINI });
    const b = body(1000);
    assert.equal((await take(actionCtx(s), r, post(url, b.stream))).status, 403);
    assert.equal(b.seen.bytes, 0);
  }
  // Run out (an hour on).
  {
    const s = await setup();
    const url = await r.url(s);
    const realNow = Date.now;
    Date.now = () => realNow() + 61 * 60_000;
    try {
      const b = body(1000);
      assert.equal((await take(actionCtx(s), r, post(url, b.stream))).status, 403);
      assert.equal(b.seen.bytes, 0);
    } finally {
      Date.now = realNow;
    }
  }
  // A move's ticket once the move has ended.
  {
    const s = await setup();
    const now = Date.now();
    await s.ctx.db.insert('threadMoves', { moveId: 'm1', threadId: 'T', from: MINI, to: 'laptop000001', state: 'failed', createdAt: now, updatedAt: now });
    const ticket = 'a'.repeat(48);
    const keyId = s.rows('runnerKeys')[0]!._id;
    await s.ctx.db.insert('uploadTickets', { ticketHash: await sha256(ticket), runnerId: MINI, keyId, issuedAt: now, moveId: 'm1' });
    const b = body(1000);
    assert.equal((await take(actionCtx(s), r, post(`https://site.example/runner/upload?ticket=${ticket}`, b.stream))).status, 403);
    assert.equal(b.seen.bytes, 0);
  }
});

test('/phone/upload: a revoked phone or one over its day reads nothing', async () => {
  const r = ROUTES[1]!;
  const s = await setup();
  const url = await r.url(s);
  await call(s.ctx, sync.revokeToken, { key: OWNER, id: s.tokenId });
  const b = body(1000);
  assert.equal((await take(actionCtx(s), r, post(url, b.stream))).status, 403);
  assert.equal(b.seen.bytes, 0);
  // Over the day's bytes: 429 before anything is read.
  const s2 = await setup();
  const url2 = await r.url(s2);
  const now = Date.now();
  for (let i = 0; i < 30; i++) await s2.ctx.db.insert('phoneUploads', { ticketHash: `h${i}`, tokenId: s2.tokenId, issuedAt: now - 2 * 60 * 60_000 - i, usedAt: now, bytes: 10 * MB });
  const b2 = body(1000);
  assert.equal((await take(actionCtx(s2), r, post(url2, b2.stream))).status, 429);
  assert.equal(b2.seen.bytes, 0);
});
