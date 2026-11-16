// Who gets a push (convex/pushTargets.ts) and how the push relay checks a
// new backend's registration (relay/convex/verify.ts): the plain parts of
// both, without a deployment.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { hasLiveToken, tiedToRevoked } from '../../../convex/pushTargets.ts';
import { convexSite, deploymentVouches } from '../../../relay/convex/verify.ts';

const now = 1_000_000;
const tok = (id: string, name: string, extra: { expiresAt?: number; revokedAt?: number } = {}) => ({ _id: id, name, expiresAt: now + 1000, ...extra });

test('a phone registered with a token gets pushes only while that token is live', () => {
  const phone = { tokenId: 't1', tokenName: 'iphone' };
  assert.equal(hasLiveToken(phone, [tok('t1', 'iphone')], now), true);
  assert.equal(hasLiveToken(phone, [tok('t1', 'iphone', { revokedAt: now - 1 })], now), false);
  assert.equal(hasLiveToken(phone, [tok('t1', 'iphone', { expiresAt: now - 1 })], now), false);
  assert.equal(hasLiveToken(phone, [], now), false);
  // Another live token of the same name doesn't keep a revoked one's phone.
  assert.equal(hasLiveToken(phone, [tok('t1', 'iphone', { revokedAt: now - 1 }), tok('t2', 'iphone')], now), false);
});

test('an old phone row with only a token name gets no pushes, whatever tokens have its name', () => {
  // Token names aren't unique, so a name can't say which phone a row is:
  // the phone registers again with its token on its next launch.
  const old = { tokenName: 'iphone' };
  assert.equal(hasLiveToken(old, [tok('t2', 'iphone')], now), false);
  assert.equal(hasLiveToken(old, [tok('t1', 'iphone', { revokedAt: now - 1 }), tok('t2', 'iphone')], now), false);
  assert.equal(hasLiveToken(old, [], now), false);
});

test('two phones paired under one name: revoking one leaves its old row no push', () => {
  // Alex's lost phone has an old name-only row; his new one is paired as
  // "iphone" too, with a row of its own token.
  const lost = { tokenName: 'iphone' };
  const kept = { tokenId: 't2', tokenName: 'iphone' };
  const revoked = { _id: 't1', name: 'iphone' };
  const after = [tok('t1', 'iphone', { revokedAt: now }), tok('t2', 'iphone')];
  assert.equal(hasLiveToken(lost, after, now), false);
  assert.equal(tiedToRevoked(lost, revoked), true);
  assert.equal(hasLiveToken(kept, after, now), true);
  assert.equal(tiedToRevoked(kept, revoked), false);
});

test('revoking a token drops its phones and old rows of its name', () => {
  const revoked = { _id: 't1', name: 'iphone' };
  assert.equal(tiedToRevoked({ tokenId: 't1', tokenName: 'iphone' }, revoked), true);
  assert.equal(tiedToRevoked({ tokenId: 't3', tokenName: 'ipad' }, revoked), false);
  assert.equal(tiedToRevoked({ tokenName: 'iphone' }, revoked), true);
  assert.equal(tiedToRevoked({ tokenName: 'ipad' }, revoked), false);
});

test('the relay only calls back to a Convex cloud deployment address', () => {
  assert.equal(convexSite('https://opulent-moose-249.ap-southeast-2.convex.site'), 'https://opulent-moose-249.ap-southeast-2.convex.site');
  assert.equal(convexSite('https://happy-otter-123.convex.site/'), 'https://happy-otter-123.convex.site');
  for (const bad of [undefined, '', 'not a url', 'http://happy-otter-123.convex.site', 'https://happy-otter-123.convex.site:8443', 'https://happy-otter-123.convex.site/x', 'https://happy-otter-123.convex.site?x=1', 'https://u:p@happy-otter-123.convex.site', 'https://evil.com', 'https://convex.site.evil.com', 'https://a.b.c.convex.site', 'http://127.0.0.1:3211', 'https://-x.convex.site']) {
    assert.equal(convexSite(bad), null, String(bad));
  }
});

test('a registration counts as the deployment\'s only when it answers ok for the nonce', async () => {
  const calls: { url: string; body: string }[] = [];
  const answer = (status: number, body: unknown) => (async (url: string | URL | Request, init?: RequestInit) => {
    calls.push({ url: String(url), body: String(init?.body) });
    return new Response(JSON.stringify(body), { status });
  }) as typeof fetch;
  assert.equal(await deploymentVouches('https://a-b-1.convex.site', 'n1', answer(200, { ok: true })), true);
  assert.deepEqual(calls[0], { url: 'https://a-b-1.convex.site/relay/verify', body: JSON.stringify({ nonce: 'n1' }) });
  assert.equal(await deploymentVouches('https://a-b-1.convex.site', 'n1', answer(404, { ok: false })), false);
  assert.equal(await deploymentVouches('https://a-b-1.convex.site', 'n1', answer(200, { ok: 'yes' })), false);
  // The static site's index.html for an old backend with no such route.
  assert.equal(await deploymentVouches('https://a-b-1.convex.site', 'n1', (async () => new Response('<html>', { status: 200 })) as typeof fetch), false);
  assert.equal(await deploymentVouches('https://a-b-1.convex.site', 'n1', (async () => { throw new Error('offline'); }) as typeof fetch), false);
});
