// The order Mike puts his runners in on the phone (convex/phone.ts,
// setRunnerOrder, API 56): phone:runners lists them in it, and a new thread
// that names no runner goes to the first one online in it, so the M5 asleep
// doesn't leave the phone with nowhere to start one (6 Nov).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { call, load, memoryBackend, outcome } from '../helpers/convex-memory.ts';

const OWNER = 'owner-key';
const MINI_KEY = 'tbr_mac-mini';
const LAPTOP = 'laptop000001';
const MINI = 'cb921b2447ea';
const CLOUD = 'cloud0000001';

const { sha256 } = load('lib') as { sha256: (s: string) => Promise<string> };
const sync = load('sync');
const phone = load('phone');

async function setup({ laptopAwake }: { laptopAwake: boolean }) {
  process.env.RUNNER_KEY = OWNER;
  delete process.env.RUNNER_KEY_NEXT;
  process.env.CONVEX_SITE_URL = 'https://site.example';
  const b = memoryBackend();
  const now = Date.now();
  await b.ctx.db.insert('runnerKeys', { runnerId: MINI, keyHash: await sha256(MINI_KEY), name: 'mac-mini', createdAt: 1 });
  // Made in this order, so the table's own order is cloud, Mac mini, the laptop.
  await b.ctx.db.insert('runners', { runnerId: CLOUD, name: 'cloud', version: 'v', startedAt: 'x', draining: false, threadCount: 5, lastSeenAt: now - 60 * 60_000 });
  await b.ctx.db.insert('runners', { runnerId: MINI, name: 'mac-mini', version: 'v', startedAt: 'x', draining: false, threadCount: 1_000_000, lastSeenAt: now });
  await b.ctx.db.insert('runners', { runnerId: LAPTOP, name: 'laptop', version: 'v', startedAt: 'x', draining: false, threadCount: 100, lastSeenAt: laptopAwake ? now : now - 10 * 60_000, byOwner: true });
  const token = 'phone-token';
  await b.ctx.db.insert('tokens', { hash: await sha256(token), name: 'phone', expiresAt: now + 100_000, createdAt: now });
  const listed = async () => ((await call(b.ctx, phone.runners, { token })) as { runnerId: string }[]).map((r) => r.runnerId);
  const startsOn = async () => {
    await call(b.ctx, phone.create, { token, text: 'hi' });
    return b.rows('commands').at(-1)!.runnerId as string;
  };
  return { ...b, token, listed, startsOn };
}

test('with no order, the main Mac is listed first asleep or not, the rest as they first came, never by thread count', async () => {
  const s = await setup({ laptopAwake: false });
  assert.deepEqual(await s.listed(), [LAPTOP, CLOUD, MINI]);
  // And a new thread still waits for it, as before there was an order.
  assert.equal(await s.startsOn(), 'any');
});

test('a new thread goes to the first runner online in the order, so the laptop asleep sends it to the next', async () => {
  const s = await setup({ laptopAwake: false });
  await call(s.ctx, phone.setRunnerOrder, { token: s.token, runnerIds: [LAPTOP, CLOUD, MINI] });
  assert.deepEqual(await s.listed(), [LAPTOP, CLOUD, MINI]);
  // The laptop and the cloud box are both offline: Mac mini, the next one up.
  assert.equal(await s.startsOn(), MINI);
  // Mac mini's key sees it, being where Alex's order put it.
  const miniSees = (await call(s.ctx, sync.pendingCommands, { key: MINI_KEY, runnerId: MINI })) as unknown[];
  assert.equal(miniSees.length, 1);
});

test('with the laptop awake and first in the order, new threads go to it', async () => {
  const s = await setup({ laptopAwake: true });
  await call(s.ctx, phone.setRunnerOrder, { token: s.token, runnerIds: [LAPTOP, MINI] });
  assert.equal(await s.startsOn(), LAPTOP);
});

test("a runner left out of the order goes after it; none of the order online falls back to the main Mac's rule", async () => {
  const s = await setup({ laptopAwake: true });
  await call(s.ctx, phone.setRunnerOrder, { token: s.token, runnerIds: [CLOUD, 'gone00000001'] });
  assert.deepEqual(await s.listed(), [CLOUD, LAPTOP, MINI]);
  // Only the placed one says so, for the phone's automatic pick.
  const rows = (await call(s.ctx, phone.runners, { token: s.token })) as { runnerId: string; placed?: boolean }[];
  assert.deepEqual(rows.filter((r) => r.placed).map((r) => r.runnerId), [CLOUD]);
  assert.deepEqual(rows.filter((r) => (r as { main?: boolean }).main).map((r) => r.runnerId), [LAPTOP]);
  // The cloud box is offline and the other id has no runner: the main Mac, not Mac mini.
  assert.equal(await s.startsOn(), LAPTOP);
});

test('a draining runner is passed over, and saving again replaces the order', async () => {
  const s = await setup({ laptopAwake: true });
  await call(s.ctx, phone.setRunnerOrder, { token: s.token, runnerIds: [MINI, LAPTOP] });
  await s.ctx.db.patch(s.rows('runners').find((r) => r.runnerId === MINI)!._id, { draining: true });
  assert.equal(await s.startsOn(), LAPTOP);
  await call(s.ctx, phone.setRunnerOrder, { token: s.token, runnerIds: [LAPTOP, LAPTOP, MINI] });
  assert.equal(s.rows('runnerOrder').length, 1);
  assert.deepEqual(s.rows('runnerOrder')[0]!.runnerIds, [LAPTOP, MINI]);
});

test("only a phone token sets the order, and it's size-checked", async () => {
  const s = await setup({ laptopAwake: true });
  assert.equal(await outcome(call(s.ctx, phone.setRunnerOrder, { token: 'not-a-token', runnerIds: [MINI] })), 'auth');
  assert.equal(await outcome(call(s.ctx, phone.setRunnerOrder, { token: s.token, runnerIds: [''] })), 'invalid');
  assert.equal(await outcome(call(s.ctx, phone.setRunnerOrder, { token: s.token, runnerIds: Array.from({ length: 51 }, (_, i) => `r${i}`) })), 'invalid');
  assert.equal(s.rows('runnerOrder').length, 0);
});

test('with no owner-key Mac known, no runner is marked main, so the phone leaves the choice to the backend', async () => {
  const s = await setup({ laptopAwake: false });
  await s.ctx.db.patch(s.rows('runners').find((r) => r.runnerId === LAPTOP)!._id, { byOwner: undefined });
  const rows = (await call(s.ctx, phone.runners, { token: s.token })) as { main?: boolean }[];
  assert.equal(rows.filter((r) => r.main).length, 0);
  assert.equal(await s.startsOn(), 'any');
});

test("the Mac sets the same order with the main key, and a runner key can't read or move it", async () => {
  const s = await setup({ laptopAwake: false });
  await call(s.ctx, sync.setRunnerOrder, { key: OWNER, runnerIds: [LAPTOP, MINI, CLOUD] });
  assert.deepEqual(await call(s.ctx, sync.runnerOrder, { key: OWNER }), [LAPTOP, MINI, CLOUD]);
  assert.deepEqual(await s.listed(), [LAPTOP, MINI, CLOUD]);
  assert.equal(await s.startsOn(), MINI);
  assert.equal(await outcome(call(s.ctx, sync.setRunnerOrder, { key: MINI_KEY, runnerIds: [MINI, LAPTOP] })), 'forbidden');
  assert.equal(await outcome(call(s.ctx, sync.runnerOrder, { key: MINI_KEY })), 'forbidden');
  assert.deepEqual(s.rows('runnerOrder')[0]!.runnerIds, [LAPTOP, MINI, CLOUD]);
});
