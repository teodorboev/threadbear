// Pools shared between runners (docs/protocol.md, "Pools", "Shared between
// runners"): the backend's list names logins by provider and email, and
// each runner turns it into its own pools, and back again on pools.save.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import type { Account, Pool } from '../../src/protocol.ts';
import { fromShared, memberOf, readShared, sameEmail, sameEmailWarnings, toShared, type SharedPool } from '../../src/pools.ts';

const login = (id: string, email: string | undefined, provider: 'claude' | 'codex' = 'claude'): Account => ({
  id, provider, title: email ?? id, configDir: `/tmp/${id}`, ...(email ? { email } : {}),
});

// Mike's Mac (runner m5) and the Hetzner box (runner hz), whose
// claude-main is a different login from the Mac's.
const M5 = [login('claude-main', 'mike@studio.example'), login('claude-1', 'Mike@Example.net'), login('claude-2', 'mike@work.example'), login('claude-3', 'mike@example.org'), login('codex-main', 'mike@example.net', 'codex')];
const HZ = [login('claude-main', 'mike@example.net'), login('codex-main', 'mike@example.net', 'codex')];

const MAC_POOLS: Pool[] = [
  { id: 'claude', title: 'Claude', accountIds: ['claude-main', 'claude-1', 'claude-3'] },
  { id: 'convex', title: 'ConvexClaude', accountIds: ['claude-2'], folders: ['~/dev/convex'] },
  { id: 'codex', title: 'Codex', accountIds: ['codex-main'] },
];

test('memberOf: a login by provider and lower-case email, or by runner and id with none', () => {
  assert.deepEqual(memberOf(M5[1]!, 'm5'), { provider: 'claude', email: 'mike@example.net' });
  assert.deepEqual(memberOf(login('claude-9', undefined), 'm5'), { provider: 'claude', runnerId: 'm5', accountId: 'claude-9' });
});

test("the Mac's pools go up by email and come back as they were", () => {
  const shared = toShared(MAC_POOLS, [], M5, 'm5');
  assert.deepEqual(shared[0]!.members, [
    { provider: 'claude', email: 'mike@studio.example' },
    { provider: 'claude', email: 'mike@example.net' },
    { provider: 'claude', email: 'mike@example.org' },
  ]);
  assert.deepEqual(shared[1]!.folders, ['~/dev/convex']);
  assert.deepEqual(fromShared(shared, M5, 'm5'), MAC_POOLS);
});

test('on another runner each pool is the logins it has, and a pool with none is left out', () => {
  const shared = toShared(MAC_POOLS, [], M5, 'm5');
  assert.deepEqual(fromShared(shared, HZ, 'hz'), [
    { id: 'claude', title: 'Claude', accountIds: ['claude-main'] },
    { id: 'codex', title: 'Codex', accountIds: ['codex-main'] },
  ]);
});

test('a save on a runner that sees less keeps what it cannot see', () => {
  const before = toShared(MAC_POOLS, [], M5, 'm5');
  // The box renames Claude and changes nothing else it can see.
  const seen = fromShared(before, HZ, 'hz');
  seen[0]!.title = 'Claude subs';
  const after = toShared(seen, before, HZ, 'hz');
  assert.equal(after[0]!.title, 'Claude subs');
  assert.deepEqual(after[0]!.members, before[0]!.members, "the Mac's studio and org stay, in their places");
  assert.deepEqual(after.map((p) => p.id), ['claude', 'codex', 'convex'], 'the convex pool it never saw is kept, after its own');
  assert.deepEqual(after[2], before[1]);
});

test('a login taken out on one runner goes; one added joins after the kept ones', () => {
  const before = toShared(MAC_POOLS, [], M5, 'm5');
  const seen = fromShared(before, M5, 'm5');
  seen[0]!.accountIds = ['claude-1', 'claude-3'];
  assert.deepEqual(toShared(seen, before, M5, 'm5')[0]!.members.map((m) => m.email), ['mike@example.net', 'mike@example.org']);
  // The box adds a Codex login to Claude: it joins the end.
  const box = fromShared(before, HZ, 'hz');
  box[0]!.accountIds = ['claude-main', 'codex-main'];
  const after = toShared(box, before, HZ, 'hz');
  assert.deepEqual(after[0]!.members.map((m) => `${m.provider}:${m.email}`), ['claude:mike@studio.example', 'claude:mike@example.net', 'claude:mike@example.org', 'codex:mike@example.net']);
});

test('a pool deleted where it was seen goes; a login with no email only counts on its own runner', () => {
  const before = toShared(MAC_POOLS, [], M5, 'm5');
  const seen = fromShared(before, HZ, 'hz').filter((p) => p.id !== 'codex');
  assert.deepEqual(toShared(seen, before, HZ, 'hz').map((p) => p.id), ['claude', 'convex']);

  const odd = login('claude-9', undefined);
  const shared: SharedPool[] = toShared([{ id: 'x', title: 'X', accountIds: ['claude-9'] }], [], [odd], 'm5');
  assert.deepEqual(fromShared(shared, [odd], 'm5')[0]!.accountIds, ['claude-9']);
  assert.deepEqual(fromShared(shared, [odd], 'hz'), [], 'claude-9 on another runner is some other login');
});

// Issue 18: two config folders on one Mac signed in to the same account.
// Email alone can't tell them apart, so the pool member carries the id too.
const TWIN = [login('claude-main', 'mike@example.net'), login('claude-2', 'Mike@Example.net'), login('claude-3', 'mike@example.org')];
const TWIN_POOLS: Pool[] = [
  { id: 'claude', title: 'Claude', accountIds: ['claude-main', 'claude-3'] },
  { id: 'hooks', title: 'Hooks', accountIds: ['claude-2'] },
  { id: 'both', title: 'Both', accountIds: ['claude-2', 'claude-main'] },
];
// What the backend and shared-pools.json hand back: JSON, read again.
const roundTrip = (list: SharedPool[]) => readShared(JSON.parse(JSON.stringify(list)));

test('sameEmail: the other logins of the provider here with the address, ignoring case', () => {
  assert.deepEqual(sameEmail(TWIN[0]!, TWIN).map((a) => a.id), ['claude-2']);
  assert.deepEqual(sameEmail(TWIN[2]!, TWIN), []);
  assert.deepEqual(sameEmail(login('codex-main', 'mike@example.net', 'codex'), TWIN), [], 'another provider is another subscription');
});

test('two logins sharing an email go up with their ids and come back as they were on their runner', () => {
  const shared = roundTrip(toShared(TWIN_POOLS, [], TWIN, 'm5'));
  assert.deepEqual(shared[1]!.members, [{ provider: 'claude', email: 'mike@example.net', runnerId: 'm5', accountId: 'claude-2' }]);
  assert.deepEqual(shared[2]!.members.map((m) => m.accountId), ['claude-2', 'claude-main'], 'both kept, not merged');
  assert.deepEqual(shared[0]!.members[1], { provider: 'claude', email: 'mike@example.org' }, 'an email only one login has goes up as before');
  assert.deepEqual(fromShared(shared, TWIN, 'm5'), TWIN_POOLS);
  // Saving the same again changes nothing.
  assert.deepEqual(toShared(fromShared(shared, TWIN, 'm5'), shared, TWIN, 'm5'), shared);
});

test('on another runner a pinned member is matched by email', () => {
  const shared = roundTrip(toShared(TWIN_POOLS, [], TWIN, 'm5'));
  assert.deepEqual(fromShared(shared, HZ, 'hz'), [
    { id: 'claude', title: 'Claude', accountIds: ['claude-main'] },
    { id: 'hooks', title: 'Hooks', accountIds: ['claude-main'] },
    { id: 'both', title: 'Both', accountIds: ['claude-main'] },
  ]);
  // A runner key gets it without the Mac's id (sync:pools): the same.
  const stripped = shared.map((p) => ({ ...p, members: p.members.map(({ provider, email }) => ({ provider, email })) }));
  assert.deepEqual(fromShared(stripped, HZ, 'hz'), fromShared(shared, HZ, 'hz'));
});

test("a save on another runner keeps the Mac's pinned members, and drops them with the login", () => {
  const shared = roundTrip(toShared(TWIN_POOLS, [], TWIN, 'm5'));
  const box = fromShared(shared, HZ, 'hz');
  box[1]!.title = 'Hook work';
  const after = roundTrip(toShared(box, shared, HZ, 'hz'));
  assert.equal(after[1]!.title, 'Hook work');
  assert.deepEqual(after.map((p) => p.members), shared.map((p) => p.members), 'the ids the Mac saved stay');
  assert.deepEqual(fromShared(after, TWIN, 'm5').map((p) => p.accountIds), TWIN_POOLS.map((p) => p.accountIds));
  // The box takes its login out of Both: the Mac's two go with it, as an email member would.
  box[2]!.accountIds = ['codex-main'];
  assert.deepEqual(roundTrip(toShared(box, shared, HZ, 'hz'))[2]!.members, [{ provider: 'codex', email: 'mike@example.net' }]);
});

test('a pinned login that has gone from its runner falls back to its email there', () => {
  const shared = roundTrip(toShared(TWIN_POOLS, [], TWIN, 'm5'));
  const gone = TWIN.filter((a) => a.id !== 'claude-2');
  assert.deepEqual(fromShared(shared, gone, 'm5')[1]!.accountIds, ['claude-main']);
});

test('a list saved before ids were carried still reads, by email as before', () => {
  const old = roundTrip([{ id: 'hooks', title: 'Hooks', members: [{ provider: 'claude', email: 'mike@example.net' }] }]);
  assert.deepEqual(old[0]!.members, [{ provider: 'claude', email: 'mike@example.net' }]);
  assert.deepEqual(fromShared(old, TWIN, 'm5')[0]!.accountIds, ['claude-main']);
  // Saved again on the Mac with claude-2 picked, it now goes up pinned.
  const again = toShared([{ id: 'hooks', title: 'Hooks', accountIds: ['claude-2'] }], old, TWIN, 'm5');
  assert.deepEqual(fromShared(again, TWIN, 'm5')[0]!.accountIds, ['claude-2']);
});

test('pin false (a backend before API 75) sends the email alone, as before', () => {
  assert.deepEqual(memberOf(TWIN[1]!, 'm5', TWIN, false), { provider: 'claude', email: 'mike@example.net' });
  assert.deepEqual(toShared([TWIN_POOLS[2]!], [], TWIN, 'm5', false)[0]!.members, [{ provider: 'claude', email: 'mike@example.net' }]);
});

test('sameEmailWarnings: one line per shared address, for tb accounts', () => {
  assert.deepEqual(sameEmailWarnings(TWIN), ["claude-main and claude-2 share an email, so they share one usage limit, and other runners can't tell them apart in a shared pool."]);
  assert.deepEqual(sameEmailWarnings(M5), [], 'claude-1 and codex-main are different providers');
});

test("a reorder on another runner moves the Mac's pinned member too", () => {
  const shared = roundTrip(toShared([{ id: 'mix', title: 'Mix', accountIds: ['claude-2', 'codex-main'] }], [], [...TWIN, login('codex-main', 'mike@example.net', 'codex')], 'm5'));
  const after = toShared([{ id: 'mix', title: 'Mix', accountIds: ['codex-main', 'claude-main'] }], shared, HZ, 'hz');
  assert.deepEqual(after[0]!.members, [{ provider: 'codex', email: 'mike@example.net' }, { provider: 'claude', email: 'mike@example.net', runnerId: 'm5', accountId: 'claude-2' }]);
});
