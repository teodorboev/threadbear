// shared-pools.ts: following the backend's pools, putting runner.json's up
// when its list is empty, saving, and starting from the copy on disk.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import type { Account, Pool } from '../../src/protocol.ts';
import type { SharedPool } from '../../src/pools.ts';
import { SharedPools, type PoolBackend } from '../../src/shared-pools.ts';

const login = (id: string, email?: string, provider: 'claude' | 'codex' = 'claude'): Account => ({ id, provider, title: id, configDir: `/tmp/${id}`, ...(email ? { email } : {}) });
const MAC = [login('claude-main', 'mike@studio.example'), login('claude-1', 'mike@example.net'), login('codex-main', 'mike@example.net', 'codex')];
const BOX = [login('claude-main', 'mike@example.net'), login('codex-main', 'mike@example.net', 'codex')];
const MAC_POOLS: Pool[] = [{ id: 'claude', title: 'Claude', accountIds: ['claude-main', 'claude-1'] }, { id: 'codex', title: 'Codex', accountIds: ['codex-main'] }];

// A backend with one list that pushes to every subscriber, like Convex.
function fakeBackend() {
  let rows: SharedPool[] = [];
  const subs: ((r: SharedPool[]) => void)[] = [];
  const push = () => subs.forEach((f) => f(structuredClone(rows)));
  const calls: string[] = [];
  const backend: PoolBackend = {
    subscribe: (onRows) => {
      subs.push(onRows);
      queueMicrotask(() => onRows(structuredClone(rows)));
      return () => {};
    },
    save: async (pools) => {
      calls.push('save');
      rows = structuredClone(pools);
      push();
    },
    adopt: async (pools) => {
      calls.push('adopt');
      if (rows.length) return false;
      rows = structuredClone(pools);
      push();
      return true;
    },
  };
  return { backend, calls, rows: () => rows };
}

function follower(accounts: Account[], runnerId: string, local?: Pool[]) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'tb-shared-pools-'));
  let changes = 0;
  const sp = new SharedPools({
    cachePath: path.join(dir, 'shared-pools.json'),
    runnerId,
    accounts: () => accounts,
    localPools: () => local,
    onChange: () => changes++,
    log: { info() {}, warn() {} },
  });
  return { sp, dir, changes: () => changes };
}

const settle = () => new Promise((r) => setTimeout(r, 10));

test("the Mac's pools go up into an empty list, and the box follows them", async () => {
  const b = fakeBackend();
  const mac = follower(MAC, 'm5', MAC_POOLS);
  const box = follower(BOX, 'hz');
  assert.equal(box.sp.pools(BOX), undefined, 'its own pools until there is a shared list');
  box.sp.attach(b.backend);
  mac.sp.attach(b.backend);
  await settle();
  assert.deepEqual(b.calls, ['adopt']);
  assert.deepEqual(mac.sp.pools(MAC), MAC_POOLS);
  assert.deepEqual(box.sp.pools(BOX), [{ id: 'claude', title: 'Claude', accountIds: ['claude-main'] }, { id: 'codex', title: 'Codex', accountIds: ['codex-main'] }]);
  assert.ok(box.changes() > 0, 'the box heard');
  // A restart with no network starts from the copy on disk.
  const again = new SharedPools({ cachePath: path.join(box.dir, 'shared-pools.json'), runnerId: 'hz', accounts: () => BOX, localPools: () => undefined, onChange() {}, log: { info() {}, warn() {} } });
  again.loadCache();
  assert.deepEqual(again.pools(BOX), box.sp.pools(BOX));
});

test("a save on the box keeps the Mac's logins it can't see", async () => {
  const b = fakeBackend();
  const mac = follower(MAC, 'm5', MAC_POOLS);
  const box = follower(BOX, 'hz');
  mac.sp.attach(b.backend);
  box.sp.attach(b.backend);
  await settle();
  await box.sp.save([{ id: 'claude', title: 'Claude subs', accountIds: ['claude-main'] }, { id: 'codex', title: 'Codex', accountIds: ['codex-main'] }]);
  await settle();
  assert.deepEqual(mac.sp.pools(MAC)![0], { id: 'claude', title: 'Claude subs', accountIds: ['claude-main', 'claude-1'] });
});

test('nothing goes up while a login has no email yet, or when runner.json has no pools', async () => {
  const b = fakeBackend();
  const noEmail = [login('claude-main'), ...MAC.slice(1)];
  const early = follower(noEmail, 'm5', MAC_POOLS);
  early.sp.attach(b.backend);
  await settle();
  assert.deepEqual(b.calls, []);
  const plain = follower(BOX, 'hz');
  plain.sp.attach(b.backend);
  plain.sp.attach(b.backend);
  await settle();
  assert.deepEqual(b.calls, []);
  assert.equal(plain.sp.pools(BOX), undefined);
});

test("a runner with none of the shared pools' logins keeps its own", async () => {
  const b = fakeBackend();
  const mac = follower(MAC, 'm5', [{ id: 'cann', title: 'Cann', accountIds: ['claude-main'] }]);
  mac.sp.attach(b.backend);
  const other = follower([login('claude-main', 'someone@else.com')], 'x');
  other.sp.attach(b.backend);
  await settle();
  assert.equal(other.sp.pools([login('claude-main', 'someone@else.com')]), undefined);
});
