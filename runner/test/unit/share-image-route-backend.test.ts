// A shared thread's pictures go to anyone with the link through
// /share/image/<slug>/<id> (convex/http.ts), which asks share:shareImageFile
// at every request, not as the files' own storage URLs, which kept working
// for a former viewer after sharing stopped until the runner's cleanup
// deleted them (Daybreak finding 135, 15 Nov 2026). Against the in-memory
// database; the route itself does no more than this query says.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { call, load, memoryBackend, THREAD_BASICS } from '../helpers/convex-memory.ts';

const OWNER = 'owner-key';
const TOKEN = 'phone-token';
const LAPTOP = 'laptop000001';
const DAY = 24 * 60 * 60_000;

const { sha256 } = load('lib') as { sha256: (s: string) => Promise<string> };
const sync = load('sync');
const share = load('share');

async function setup() {
  process.env.RUNNER_KEY = OWNER;
  delete process.env.RUNNER_KEY_NEXT;
  process.env.CONVEX_SITE_URL = 'https://site.example';
  const b = memoryBackend();
  const now = Date.now();
  await b.ctx.db.insert('tokens', { hash: await sha256(TOKEN), name: 'iphone', kind: 'phone', expiresAt: now + 100 * DAY, createdAt: now });
  await b.ctx.db.insert('runners', { runnerId: LAPTOP, name: 'Laptop', version: 'v', startedAt: 'x', draining: false, threadCount: 1, lastSeenAt: now, byOwner: true });
  const sharedAs = (threadId: string, slug: string, access: 'link' | 'off') =>
    call(b.ctx, sync.upsertThreads, { key: OWNER, threads: [{ threadId, runnerId: LAPTOP, ...THREAD_BASICS, title: 'T', view: 'inbox', needsYou: false, share: { slug, access } }] });
  await sharedAs('T', 'slug-one', 'link');
  await sharedAs('U', 'slug-other', 'link');
  await call(b.ctx, sync.appendMessages, { key: OWNER, messages: [{ threadId: 'T', seq: 1, role: 'assistant', text: 'look', at: 'now' }] });
  await call(b.ctx, sync.addShareImage, { key: OWNER, threadId: 'T', seq: 1, path: '/a.png', storageId: b.file('pic'), mediaType: 'image/png' });
  await call(b.ctx, sync.addShareImage, { key: OWNER, threadId: 'U', seq: 1, path: '/b.png', storageId: b.file('other'), mediaType: 'image/svg+xml' });
  const id = b.rows('shareImages').find((r) => r.threadId === 'T')!._id as string;
  const otherId = b.rows('shareImages').find((r) => r.threadId === 'U')!._id as string;
  return { ...b, sharedAs, id, otherId };
}

const fetchAs = (s: { ctx: unknown }, slug: string, id: string) => call(s.ctx, share.shareImageFile, { slug, id });

test('anyone with the link gets the pictures through the route, not their storage URLs; Mike gets the files', async () => {
  const s = await setup();
  const page = await call(s.ctx, share.view, { slug: 'slug-one' });
  assert.deepEqual(page.images.map((i: { url: string }) => i.url), [`https://site.example/share/image/slug-one/${encodeURIComponent(s.id)}`]);
  const card = await call(s.ctx, share.preview, { slug: 'slug-one' });
  assert.equal(card.image, `https://site.example/share/image/slug-one/${encodeURIComponent(s.id)}`);
  const mikes = await call(s.ctx, share.view, { slug: 'slug-one', token: TOKEN });
  assert.deepEqual(mikes.images.map((i: { url: string }) => i.url), ['https://files.example/pic']);
  assert.deepEqual(await fetchAs(s, 'slug-one', s.id), { storageId: 'pic', mediaType: 'image/png' });
});

test('stopping sharing stops the picture at once, before the runner drops the pictures, and sharing again brings it back', async () => {
  const s = await setup();
  await s.sharedAs('T', 'slug-one', 'off');
  assert.ok(s.rows('shareImages').some((r) => r.threadId === 'T'), "the runner's cleanup hasn't run");
  assert.equal(await fetchAs(s, 'slug-one', s.id), null);
  await s.sharedAs('T', 'slug-one', 'link');
  assert.notEqual(await fetchAs(s, 'slug-one', s.id), null);
});

test('Share again with a new link: the old link’s picture URLs stop', async () => {
  const s = await setup();
  await s.sharedAs('T', 'slug-two', 'link');
  assert.equal(await fetchAs(s, 'slug-one', s.id), null);
  assert.notEqual(await fetchAs(s, 'slug-two', s.id), null);
});

test("a link reaches only its own thread's pictures, and a stored type the page doesn't show goes as plain bytes", async () => {
  const s = await setup();
  assert.equal(await fetchAs(s, 'slug-one', s.otherId), null, "another shared thread's picture");
  assert.equal(await fetchAs(s, 'slug-one', 'messages:1'), null, 'a row of another table');
  assert.equal(await fetchAs(s, 'slug-one', 'nonsense'), null);
  assert.equal(await fetchAs(s, 'nope', s.id), null);
  assert.deepEqual(await fetchAs(s, 'slug-other', s.otherId), { storageId: 'other', mediaType: 'application/octet-stream' });
});
