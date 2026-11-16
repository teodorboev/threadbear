// Pairing codes in the backend's own functions (convex/tokens.ts, sync.ts),
// against the in-memory database: Daybreak finding 127 (15 Nov 2026). The
// Mac's QR code used to carry the phone's 90-day token in a threadbear://pair
// link any app could claim; now it carries a one-use, ten-minute code the
// phone's own scanner trades for its token with tokens:claim.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { call, load, memoryBackend, outcome } from '../helpers/convex-memory.ts';

const OWNER = 'owner-key';
const MINI_KEY = 'tbr_mac-mini';
const { sha256 } = load('lib') as { sha256: (s: string) => Promise<string> };
const sync = load('sync');
const tokens = load('tokens');
const phone = load('phone');

async function setup() {
  process.env.RUNNER_KEY = OWNER;
  delete process.env.RUNNER_KEY_NEXT;
  const b = memoryBackend();
  await b.ctx.db.insert('runnerKeys', { runnerId: 'cb921b2447ea', keyHash: await sha256(MINI_KEY), name: 'mac-mini', createdAt: 1 });
  return b;
}

const claim = (ctx: unknown, code: string) => call(ctx, tokens.claim, { code });
const refusal = async (p: Promise<unknown>) => {
  try {
    await p;
    return 'accepted';
  } catch (e) {
    return String((e as { data?: { message?: string } }).data?.message ?? (e as Error).message);
  }
};

test('a pairing code is traded for a working phone token once, and only its hash is kept', async () => {
  const s = await setup();
  const made = await call(s.ctx, sync.issuePairingCode, { key: OWNER, name: 'iPhone' });
  assert.match(made.code, /^tbp_[0-9a-f]{48}$/);
  const row = s.rows('pairingCodes')[0]!;
  assert.equal(row.hash, await sha256(made.code));
  assert.ok(!JSON.stringify(s.rows('pairingCodes')).includes(made.code), 'the plain code was stored');
  assert.ok(row.expiresAt - Date.now() <= 10 * 60_000 && row.expiresAt - Date.now() > 9 * 60_000, 'not ten minutes');
  assert.equal(s.rows('tokens').length, 0, 'a token exists before the phone has claimed it');
  const got = await claim(s.ctx, made.code);
  assert.match(got.token, /^tb_/);
  const t = s.rows('tokens')[0]!;
  assert.equal(t.name, 'iPhone');
  assert.equal(t.kind, 'phone');
  assert.equal(t.issuedBy, 'owner');
  assert.match(t.ownerKeyPrint, /^[0-9a-f]{8}$/);
  assert.ok(t.expiresAt - Date.now() > 89 * 86_400_000);
  const me = await call(s.ctx, phone.me, { token: got.token });
  assert.ok(me);
  // Replayed: refused, and no second token.
  assert.match(await refusal(claim(s.ctx, made.code)), /expired or was already used/);
  assert.equal(s.rows('tokens').length, 1);
});

test('an expired, unknown or malformed code gets no token', async () => {
  const s = await setup();
  const made = await call(s.ctx, sync.issuePairingCode, { key: OWNER, name: 'iPhone' });
  const row = s.rows('pairingCodes')[0]!;
  await s.ctx.db.patch(row._id, { expiresAt: Date.now() - 1 });
  assert.match(await refusal(claim(s.ctx, made.code)), /expired or was already used/);
  // A code from another deployment is just one this one never made.
  assert.match(await refusal(claim(s.ctx, 'tbp_' + '0'.repeat(48))), /expired or was already used/);
  assert.match(await refusal(claim(s.ctx, 'tb_' + '0'.repeat(48))), /expired or was already used/);
  assert.match(await refusal(claim(s.ctx, 'tbp_' + 'x'.repeat(5000))), /expired or was already used/);
  // The hash itself is no code.
  assert.match(await refusal(claim(s.ctx, row.hash)), /expired or was already used/);
  assert.equal(s.rows('tokens').length, 0);
});

test('two claims of one code (two phones racing): one token', async () => {
  const s = await setup();
  const made = await call(s.ctx, sync.issuePairingCode, { key: OWNER, name: 'iPhone' });
  // Convex runs each mutation as a transaction and retries one whose read
  // (the code's row) another changed, so racing claims come out as one after
  // the other: the memory backend has no transactions, so they're run so.
  const first = await refusal(claim(s.ctx, made.code));
  const second = await refusal(claim(s.ctx, made.code));
  assert.equal(first, 'accepted');
  assert.match(second, /already used/);
  assert.equal(s.rows('tokens').length, 1);
});

test('only the main Mac\'s key makes pairing codes', async () => {
  const s = await setup();
  assert.equal(await outcome(call(s.ctx, sync.issuePairingCode, { key: MINI_KEY, name: 'iPhone' })), 'forbidden');
  assert.notEqual(await outcome(call(s.ctx, sync.issuePairingCode, { key: 'wrong', name: 'iPhone' })), 'ok');
  assert.equal(s.rows('pairingCodes').length, 0);
});

test('spent and expired codes are swept a few at a time', async () => {
  const s = await setup();
  for (let i = 0; i < 30; i++) await s.ctx.db.insert('pairingCodes', { hash: `h${i}`, name: 'old', kind: 'phone', createdAt: 1, expiresAt: 2 });
  await call(s.ctx, sync.issuePairingCode, { key: OWNER, name: 'iPhone' });
  assert.equal(s.rows('pairingCodes').length, 11);
  await call(s.ctx, sync.issuePairingCode, { key: OWNER, name: 'iPhone' });
  assert.equal(s.rows('pairingCodes').length, 2);
});

test("an older runner's sync:issueToken gets no phone token to put in a link, and a browser still signs in", async () => {
  const s = await setup();
  assert.match(await refusal(call(s.ctx, sync.issueToken, { key: OWNER, kind: 'phone', name: 'iPhone' })), /too old to pair a phone/);
  assert.equal(s.rows('tokens').length, 0);
  const browser = await call(s.ctx, sync.issueToken, { key: OWNER, kind: 'browser', name: 'Chrome' });
  assert.match(browser.token, /^tb_/);
});

test('pair-qr.swift\'s tokens:issueCode makes a code, not a token', async () => {
  const s = await setup();
  const made = await call(s.ctx, tokens.issueCode, { name: 'iphone', days: 90 });
  assert.match(made.code, /^tbp_/);
  assert.equal(s.rows('tokens').length, 0);
  assert.match((await claim(s.ctx, made.code)).token, /^tb_/);
});
