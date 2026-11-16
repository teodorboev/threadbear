// devices.list, devices.pair and devices.revoke (src/devices.ts) against
// a fake backend: the links they hand back, what they refuse, and that the
// token never goes anywhere but the reply.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { makeHarness } from '../helpers/fakes.ts';
import { ConvexSync } from '../../src/sync.ts';
import { devicesMethods, pairingText, readPairingText, signInLink, siteOf } from '../../src/devices.ts';
import { RpcError } from '../../src/protocol.ts';
import type { Logger } from '../../src/log.ts';

type Call = { kind: string; fn: string; args: Record<string, unknown> };

function setup(opts: { url?: string; site?: string; key?: string; boxKey?: string; fail?: (fn: string) => Error | undefined } = {}) {
  const h = makeHarness();
  const calls: Call[] = [];
  const logged: string[] = [];
  const log: Logger = {
    info: (m, d) => logged.push(m + JSON.stringify(d ?? {})),
    warn: (m, d) => logged.push(m + JSON.stringify(d ?? {})),
    error: (m, d) => logged.push(m + JSON.stringify(d ?? {})),
    debug: (m, d) => logged.push(m + JSON.stringify(d ?? {})),
  } as Logger;
  const tokens = new Map<string, { id: string; kind: string; name: string; createdAt: string; expiresAt: string; expired: boolean }>();
  let n = 0;
  const post = async (kind: 'mutation' | 'query', fn: string, args: Record<string, unknown>) => {
    const err = opts.fail?.(fn);
    if (err) throw err;
    calls.push({ kind, fn, args });
    if (fn === 'sync:issueToken') {
      const id = `tok${++n}`;
      tokens.set(id, { id, kind: args.kind as string, name: args.name as string, createdAt: '2026-10-28T00:00:00.000Z', expiresAt: '2027-01-26T00:00:00.000Z', expired: false });
      return { id, token: `tb_secret${n}`, expiresAt: '2027-01-26T00:00:00.000Z' };
    }
    if (fn === 'sync:issuePairingCode') return { id: `code${++n}`, code: `tbp_onceonly${n}`, expiresAt: '2026-10-28T00:10:00.000Z' };
    if (fn === 'sync:listTokens') return [...tokens.values()];
    if (fn === 'sync:revokeToken') return tokens.delete(args.id as string);
    return null;
  };
  const sync = new ConvexSync({
    config: { enabled: true, url: 'url' in opts ? opts.url : 'https://x.convex.cloud', runnerId: 'r1', ...(opts.site ? { site: opts.site } : {}) },
    key: 'key' in opts ? opts.key : 'k',
    runner: h.runner,
    version: '0.1.0+test',
    startedAt: '2026-10-27T00:00:00.000Z',
    clock: h.clock,
    log,
    post,
    ...(opts.boxKey ? { secretBox: { publicKey: () => opts.boxKey!, open: () => '' } } : {}),
  });
  return { sync, calls, logged, methods: devicesMethods(sync) };
}

test('links: the pairing text carries the deployment and a one-use code, the sign-in link puts the token in the fragment', () => {
  const text = pairingText('https://x.convex.cloud', 'tbp_1');
  assert.match(text, /^TBPAIR1_[A-Za-z0-9_-]+$/);
  assert.equal(readPairingText(text)!.toString(), 'url=https%3A%2F%2Fx.convex.cloud&code=tbp_1');
  assert.equal(signInLink('https://x.convex.site', 'tb_1'), 'https://x.convex.site/#token=tb_1');
  assert.equal(siteOf('https://x.convex.cloud/'), 'https://x.convex.site');
  assert.equal(siteOf('http://127.0.0.1:3210'), undefined);
  assert.equal(siteOf(undefined), undefined);
});

test('links: the pairing text carries each runner key the phone may seal to', () => {
  const key = 'pOCSkrZRwni5dyxWn1+puxPZBrRqtoyd+dwrRAn4ogk=';
  const text = pairingText('https://x.convex.cloud', 'tbp_1', [{ runnerId: '0a1b2c', key }]);
  assert.deepEqual(readPairingText(text)!.getAll('key'), [`0a1b2c:${key}`]);
});

test('pair: the phone code carries this runner\'s id and secret-box key', async () => {
  const key = 'pOCSkrZRwni5dyxWn1+puxPZBrRqtoyd+dwrRAn4ogk=';
  const { methods } = setup({ boxKey: key });
  const phone = (await methods['devices.pair']({ kind: 'phone' })) as { pairingLink?: string };
  assert.deepEqual(readPairingText(phone.pairingLink!)!.getAll('key'), [`r1:${key}`]);
});

// Daybreak 127 (15 Nov 2026): the QR code was a threadbear://pair link with
// the phone's 90-day token in it, and any app can claim that scheme.
test('pair: a phone gets no token at all, only a one-use code in text no app can claim as a link', async () => {
  const { methods, calls } = setup();
  const phone = (await methods['devices.pair']({ kind: 'phone' })) as { device: { id: string; kind: string; name: string }; pairingLink?: string };
  assert.equal(phone.device.kind, 'phone');
  assert.equal(phone.device.name, 'iPhone');
  assert.equal(calls.filter((c) => c.fn === 'sync:issueToken').length, 0, 'a phone token was issued for the QR code');
  assert.deepEqual(calls.filter((c) => c.fn === 'sync:issuePairingCode').map((c) => c.args), [{ key: 'k', name: 'iPhone', days: 90 }]);
  const text = phone.pairingLink!;
  assert.ok(!/^[a-z][a-z0-9+.-]*:/i.test(text), `the pairing code is a URL some app could claim: ${text}`);
  assert.ok(!text.includes('.'), 'a dot could make a link finder take it for a web address');
  assert.ok(!text.includes('tb_secret'), 'the token is in the QR code');
  const q = readPairingText(text)!;
  assert.equal(q.get('code'), 'tbp_onceonly1');
  assert.equal(q.get('token'), null);
  assert.equal(q.get('url'), 'https://x.convex.cloud');
});

test('pair: a browser gets a sign-in link, and no token or code is logged', async () => {
  const { methods, calls, logged } = setup();
  await methods['devices.pair']({ kind: 'phone' });
  const browser = (await methods['devices.pair']({ kind: 'browser', name: 'Chrome', days: 30 })) as { device: { name: string }; signInLink?: string; pairingLink?: string };
  assert.equal(browser.device.name, 'Chrome');
  assert.equal(browser.signInLink, 'https://x.convex.site/#token=tb_secret2');
  assert.equal(browser.pairingLink, undefined);
  assert.deepEqual(calls.filter((c) => c.fn === 'sync:issueToken').map((c) => c.args), [{ key: 'k', kind: 'browser', name: 'Chrome', days: 30 }]);
  assert.ok(!logged.some((l) => l.includes('tb_secret') || l.includes('tbp_')), 'a token or code was logged');
});

test('pair: a backend on this Mac only (setup --self-hosted --local) signs a browser in but never issues a phone a link to 127.0.0.1', async () => {
  const { methods, calls } = setup({ url: 'http://127.0.0.1:13210', site: 'http://127.0.0.1:13211' });
  await assert.rejects(methods['devices.pair']({ kind: 'phone' }) as Promise<unknown>, (e: RpcError) => /on this Mac only/.test(e.message) && /--tailnet/.test(e.message));
  assert.equal(calls.filter((c) => c.fn === 'sync:issueToken' || c.fn === 'sync:issuePairingCode').length, 0, 'a token was issued for a phone that cannot use it');
  const browser = (await methods['devices.pair']({ kind: 'browser', name: 'Safari' })) as { signInLink?: string };
  assert.equal(browser.signInLink, 'http://127.0.0.1:13211/#token=tb_secret1');
});

test('list and revoke go through the backend each time; revoking an unknown id is bad params', async () => {
  const { methods } = setup();
  await methods['devices.pair']({ kind: 'phone' });
  await methods['devices.pair']({ kind: 'browser', name: 'Safari' });
  await methods['devices.pair']({ kind: 'browser', name: 'Chrome' });
  const list = (await methods['devices.list']({})) as { devices: { id: string; name: string }[]; site?: string };
  assert.deepEqual(list.devices.map((d) => d.name), ['Safari', 'Chrome']);
  assert.equal(list.site, 'https://x.convex.site');
  const after = (await methods['devices.revoke']({ deviceId: list.devices[0]!.id })) as { devices: { name: string }[] };
  assert.deepEqual(after.devices.map((d) => d.name), ['Chrome']);
  await assert.rejects(methods['devices.revoke']({ deviceId: 'nope' }) as Promise<unknown>, (e: RpcError) => e.code === -32602);
});

test('bad params are refused before the backend is asked', async () => {
  const { methods, calls } = setup();
  await assert.rejects(methods['devices.pair']({ kind: 'watch' }) as Promise<unknown>, (e: RpcError) => e.code === -32602 && /kind/.test(e.message));
  await assert.rejects(methods['devices.pair']({ kind: 'phone', days: 0 }) as Promise<unknown>, (e: RpcError) => e.code === -32602 && /days/.test(e.message));
  await assert.rejects(methods['devices.pair']({ kind: 'phone', name: '  ' }) as Promise<unknown>, (e: RpcError) => e.code === -32602 && /name/.test(e.message));
  await assert.rejects(methods['devices.revoke']({}) as Promise<unknown>, (e: RpcError) => e.code === -32602);
  assert.equal(calls.length, 0);
});

test('with sync off, list says why and pair and revoke refuse with the reason', async () => {
  const { methods, calls } = setup({ key: undefined });
  const list = (await methods['devices.list']({})) as { devices: unknown[]; reason?: string };
  assert.deepEqual(list.devices, []);
  assert.equal(list.reason, 'no secrets/convex-key');
  await assert.rejects(methods['devices.pair']({ kind: 'phone' }) as Promise<unknown>, (e: RpcError) => e.code === -32000 && /convex-key/.test(e.message) && /threadbear setup/.test(e.message));
  await assert.rejects(methods['devices.revoke']({ deviceId: 'x' }) as Promise<unknown>, (e: RpcError) => e.code === -32000);
  assert.equal(calls.length, 0);
});

test('a backend without the token functions is said in words, and other failures pass through', async () => {
  const old = setup({ fail: (fn) => (fn.startsWith('sync:') ? new Error(`Could not find public function for '${fn}'. Did you forget to run \`npx convex dev\` or \`npx convex deploy\`?`) : undefined) });
  await assert.rejects(old.methods['devices.pair']({ kind: 'phone' }) as Promise<unknown>, (e: RpcError) => e.code === -32000 && /older than this runner/.test(e.message) && !/npx convex/.test(e.message));
  await assert.rejects(old.methods['devices.list']({}) as Promise<unknown>, (e: RpcError) => e.code === -32000 && /older than this runner/.test(e.message));
  const down = setup({ fail: () => new Error('fetch failed') });
  await assert.rejects(down.methods['devices.pair']({ kind: 'browser' }) as Promise<unknown>, (e: RpcError) => e.code === -32000 && e.message === 'fetch failed');
});

test('a local deployment with no sync.site has no website, and no phone either', async () => {
  const { methods } = setup({ url: 'http://127.0.0.1:3210' });
  // Nor a phone: it can't reach this Mac's 127.0.0.1 (and the iPhone app
  // refuses an http link anyway).
  await assert.rejects(methods['devices.pair']({ kind: 'phone' }) as Promise<unknown>, (e: RpcError) => e.code === -32000 && /on this Mac only/.test(e.message));
  await assert.rejects(methods['devices.pair']({ kind: 'browser' }) as Promise<unknown>, (e: RpcError) => e.code === -32000 && /website/.test(e.message));
  const list = (await methods['devices.list']({})) as { site?: string };
  assert.equal(list.site, undefined);
});
