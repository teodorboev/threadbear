// How setup self-hosts the backend (scripts/self-hosted-plan.mjs): on the
// tailnet, on this Mac only, or at an address of your own, which a later run
// keeps, and the container settings for each, never published beyond loopback.
import { test } from 'node:test';
import assert from 'node:assert/strict';

type Flags = { selfHosted?: boolean; local?: boolean; tailnet?: boolean; url?: string; siteUrl?: string };
type Plan = { selfHosted?: boolean; mode?: 'local' | 'tailnet' | 'url'; from?: string; fail?: string; url?: string; site?: string; readdressed?: boolean };
type Container = { env: Record<string, string>; runArgs: string[]; cmd: string[] };
// Plain JS with no types, as setup runs before any packages are installed.
const planModule: string = '../../../scripts/self-hosted-plan.mjs';
const m = (await import(planModule)) as {
  selfHostedPlan: (p: { flags?: Flags; joining?: boolean; adminKeyFile?: boolean; syncUrl?: string; syncSite?: string }) => Plan;
  backendContainer: (mode: string, at?: { name: string; ip: string } | { url: string; site: string }) => Container;
  origins: (mode: string, at?: string | { url: string; site: string }) => { url: string; site: string; dashboard: string; dashboardBackend: string };
  reachOf: (syncUrl: string) => string;
  ownAddress: (raw: string, flag: string) => { origin?: string; fail?: string };
  isLoopbackUrl: (u: string) => boolean;
  servedOn: (status: unknown, tailnetPort: number) => { Proxy?: string }[] | undefined;
  mustSeeTailnetOff: (a: { from?: string; pending?: boolean }) => boolean;
  tailnetPendingFiles: (defaultHome: string, home: string, path: unknown) => string[];
  offTailnetStep: (a: { cli: boolean; handlers?: { Proxy?: string }[]; tailnetPort: number; localPort: number; moving: boolean }) => { do: string; message?: string };
};

const LOCAL_URL = 'http://127.0.0.1:13210';
const TAILNET_URL = 'https://mac.tail1.ts.net:3210';
const OWN_URL = 'https://threadbear.example.com';
const OWN_SITE = 'https://threadbear-site.example.com';
const OWN = { url: OWN_URL, siteUrl: OWN_SITE };

test('a first setup self-hosts only when asked, on the tailnet unless --local', () => {
  assert.deepEqual(m.selfHostedPlan({}), { selfHosted: false });
  assert.deepEqual(m.selfHostedPlan({ flags: { selfHosted: true } }), { selfHosted: true, mode: 'tailnet' });
  assert.deepEqual(m.selfHostedPlan({ flags: { selfHosted: true, local: true } }), { selfHosted: true, mode: 'local' });
  // --local on its own means self-hosted too.
  assert.deepEqual(m.selfHostedPlan({ flags: { local: true } }), { selfHosted: true, mode: 'local' });
  assert.ok(m.selfHostedPlan({ flags: { local: true, tailnet: true } }).fail);
});

test('setup run again keeps the way it was set up, and only a flag moves it', () => {
  // No flags: the admin key file says self-hosted, the loopback sync.url says local.
  assert.deepEqual(m.selfHostedPlan({ adminKeyFile: true, syncUrl: LOCAL_URL }), { selfHosted: true, mode: 'local' });
  assert.deepEqual(m.selfHostedPlan({ adminKeyFile: true, syncUrl: TAILNET_URL }), { selfHosted: true, mode: 'tailnet' });
  // Plain --self-hosted on a local Mac stays local: an agent rerunning the
  // docs' command doesn't put it on a (work) tailnet.
  assert.deepEqual(m.selfHostedPlan({ flags: { selfHosted: true }, adminKeyFile: true, syncUrl: LOCAL_URL }), { selfHosted: true, mode: 'local' });
  assert.deepEqual(m.selfHostedPlan({ flags: { tailnet: true }, adminKeyFile: true, syncUrl: LOCAL_URL }), { selfHosted: true, mode: 'tailnet', from: 'local' });
  assert.deepEqual(m.selfHostedPlan({ flags: { selfHosted: true, local: true }, adminKeyFile: true, syncUrl: TAILNET_URL }), { selfHosted: true, mode: 'local', from: 'tailnet' });
  // An admin key with no sync.url: a first setup stopped partway. Without a
  // flag it asks rather than guess the tailnet; with one it carries on.
  assert.ok(m.selfHostedPlan({ adminKeyFile: true }).fail);
  assert.ok(m.selfHostedPlan({ flags: { selfHosted: true }, adminKeyFile: true }).fail);
  assert.deepEqual(m.selfHostedPlan({ flags: { local: true }, adminKeyFile: true }), { selfHosted: true, mode: 'local' });
  // A loopback sync.url without the admin key file isn't a self-hosted Mac
  // (a dev deployment, say): setup doesn't take it for one.
  assert.deepEqual(m.selfHostedPlan({ syncUrl: LOCAL_URL }), { selfHosted: false });
});

test('a Mac joining another has no backend, and the self-hosted flags are refused there', () => {
  assert.deepEqual(m.selfHostedPlan({ joining: true, adminKeyFile: false }), { selfHosted: false });
  for (const flags of [{ selfHosted: true }, { local: true }, { tailnet: true }]) assert.ok(m.selfHostedPlan({ joining: true, flags }).fail);
});

test('every port either backend publishes is on 127.0.0.1, never the LAN', () => {
  for (const c of [m.backendContainer('local'), m.backendContainer('tailnet', { name: 'mac.tail1.ts.net', ip: '100.64.0.1' }), m.backendContainer('url', { url: OWN_URL, site: OWN_SITE })]) {
    const published = c.runArgs.flatMap((a, i) => (c.runArgs[i - 1] === '-p' ? [a] : []));
    assert.equal(published.length, 2);
    for (const p of published) assert.match(p, /^127\.0\.0\.1:\d+:\d+$/);
    assert.ok(!c.runArgs.includes('--network') && !c.runArgs.includes('--net'));
  }
});

test('a local backend has one origin for the browser, the runner and the container itself', () => {
  const c = m.backendContainer('local');
  const o = m.origins('local');
  assert.equal(o.url, LOCAL_URL);
  assert.equal(c.env.CONVEX_CLOUD_ORIGIN, o.url);
  assert.equal(c.env.CONVEX_SITE_ORIGIN, o.site);
  // Published on the same port inside as out, so the origin reaches the
  // backend from inside the container too (the website's stored files).
  assert.ok(c.runArgs.includes('127.0.0.1:13210:13210') && c.runArgs.includes('127.0.0.1:13211:13211'));
  const script = c.cmd.join(' ');
  assert.match(script, /--port 13210/);
  assert.match(script, /--site-proxy-port 13211/);
  // It stops rather than run on the image's own ports if the script it edits changes.
  assert.match(script, /grep -q/);
  assert.ok(!c.runArgs.includes('--add-host'));
  assert.ok(Object.values(o).every(m.isLoopbackUrl));
});

test('a tailnet backend is unchanged by the local way (7 Nov containers aren\'t made again)', () => {
  const c = m.backendContainer('tailnet', { name: 'mac.tail1.ts.net', ip: '100.64.0.1' });
  assert.deepEqual(c.cmd, []);
  assert.deepEqual(c.runArgs, ['-p', '127.0.0.1:13210:3210', '-p', '127.0.0.1:13211:3211', '-v', 'threadbear-convex-data:/convex/data', '--add-host', 'mac.tail1.ts.net:100.64.0.1']);
  assert.deepEqual(c.env, { CONVEX_CLOUD_ORIGIN: TAILNET_URL, CONVEX_SITE_ORIGIN: 'https://mac.tail1.ts.net:3211', DISABLE_BEACON: 'true' });
  assert.equal(m.origins('tailnet', 'mac.tail1.ts.net').url, TAILNET_URL);
});

test('only loopback URLs count as local', () => {
  for (const u of [LOCAL_URL, 'http://localhost:13210', 'http://[::1]:13210']) assert.ok(m.isLoopbackUrl(u), u);
  for (const u of [TAILNET_URL, 'https://happy-otter-123.convex.cloud', 'http://127.0.0.1.evil.com', 'not a url']) assert.ok(!m.isLoopbackUrl(u), u);
});

// Daybreak, 11 Nov 2026 (finding 101): moving to --local while Tailscale was
// stopped read its failed `serve status` as "nothing served", saved the
// local URL and left the backend's ports on the tailnet, saved for when
// Tailscale came back; later runs, now local, never looked again.
test("a Tailscale that can't say what it serves is never read as serving nothing", () => {
  const status = (stdout: string, code = 0, error?: Error) => ({ status: code, stdout, stderr: '', error });
  const served = JSON.stringify({ Web: { 'mac.tail1.ts.net:3210': { Handlers: { '/': { Proxy: 'http://127.0.0.1:13210' } } } } });
  assert.deepEqual(m.servedOn(status(served), 3210), [{ Proxy: 'http://127.0.0.1:13210' }]);
  assert.deepEqual(m.servedOn(status(served), 3211), []);
  assert.deepEqual(m.servedOn(status('{}'), 3210), []);
  assert.deepEqual(m.servedOn(status(''), 3210), []);
  assert.equal(m.servedOn(status('', 1), 3210), undefined); // its daemon not running
  assert.equal(m.servedOn(status(served, 1), 3210), undefined);
  assert.equal(m.servedOn(status('not json'), 3210), undefined);
  assert.equal(m.servedOn(status('[]'), 3210), undefined);
  assert.equal(m.servedOn(status('null'), 3210), undefined);
  assert.equal(m.servedOn(status(JSON.stringify({ Web: 'x' })), 3210), undefined);
  assert.equal(m.servedOn(status(JSON.stringify({ Web: { 'mac:3210': null } })), 3210), undefined);
  assert.equal(m.servedOn({ status: null, stdout: '', error: new Error('ENOENT') }, 3210), undefined); // didn't run
  assert.equal(m.servedOn(undefined, 3210), undefined);
});

test('a move off the tailnet stops unless it can see each port is off', () => {
  const ours = [{ Proxy: 'http://127.0.0.1:13210/' }];
  const step = (a: Partial<Parameters<typeof m.offTailnetStep>[0]>) => m.offTailnetStep({ cli: true, tailnetPort: 3210, localPort: 13210, moving: true, ...a });
  // Couldn't ask, or no Tailscale found: stops, so the tailnet URL stays and the next run tries again.
  assert.equal(step({ handlers: undefined }).do, 'fail');
  assert.equal(step({ cli: false, handlers: undefined }).do, 'fail');
  assert.match(step({ cli: false }).message ?? '', /can't find Tailscale/);
  // Serving it: off. Serving nothing, or only someone else's: nothing.
  assert.equal(step({ handlers: ours }).do, 'off');
  assert.equal(step({ handlers: [] }).do, 'nothing');
  assert.equal(step({ handlers: [{ Proxy: 'http://127.0.0.1:8080' }] }).do, 'nothing');
  // Ours and someone else's on one port: can't take only ours off.
  assert.equal(step({ handlers: [...ours, { Proxy: 'http://127.0.0.1:8080' }] }).do, 'fail');
  assert.equal(step({ handlers: [{}] }).do, 'nothing');
  // A local run that isn't a move still takes ours off, every time, and only says it when it can't ask.
  assert.equal(step({ moving: false, handlers: ours }).do, 'off');
  assert.equal(step({ moving: false, handlers: undefined }).do, 'note');
  assert.equal(step({ moving: false, cli: false }).do, 'nothing');
});

// Cross-lab review of that fix: a --tailnet run that served the ports and
// then stopped before saving its URL left a local URL, so a later --local
// run with Tailscale stopped only said it couldn't check. The note setup
// leaves before serving makes that run check too.
test('a local run after the backend was served on the tailnet has to see it off, URL or not', () => {
  assert.equal(m.mustSeeTailnetOff({ from: 'tailnet' }), true);
  assert.equal(m.mustSeeTailnetOff({ pending: true }), true);
  assert.equal(m.mustSeeTailnetOff({ from: undefined, pending: false }), false);
  const step = (moving: boolean) => m.offTailnetStep({ cli: true, handlers: undefined, tailnetPort: 3210, localPort: 13210, moving });
  assert.equal(step(m.mustSeeTailnetOff({ pending: true })).do, 'fail');
  assert.equal(step(m.mustSeeTailnetOff({ pending: false })).do, 'note');
});

// The order that makes it hold: the note is written before the first
// tailscale serve, and removed only after every port's step, in setup itself.
test('setup leaves the tailnet note before serving and clears it only after every port is off', async () => {
  const fs = await import('node:fs');
  const src = fs.readFileSync(new URL('../../../scripts/setup.mjs', import.meta.url), 'utf8');
  const write = src.indexOf('fs.writeFileSync(pendingFile');
  const serveLoop = src.indexOf('serve(tailnet.ts, tailnetPort, localPort)');
  const unserveLoop = src.indexOf('unserve(ts, tailnetPort, localPort, moving)');
  const remove = src.indexOf('for (const f of pendingFiles) fs.rmSync(f');
  assert.ok(write > 0 && write < serveLoop, 'written before serving');
  assert.ok(unserveLoop > 0 && unserveLoop < remove, 'removed after the ports');
});

// Daybreak 12 Nov (finding 110): setup before 4d8cdb23 left the note in the
// --home folder. An interrupted --tailnet run there, then a --local run on
// the same folder with Tailscale unable to say what it serves, found no note
// in the default folder and only noted it, leaving the ports served.
test('a note an older setup left in a custom --home still makes a local run see the ports off', async () => {
  const fs = await import('node:fs');
  const os = await import('node:os');
  const path = await import('node:path');
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'tb-pending-'));
  try {
    const def = path.join(root, 'Threadbear');
    const custom = path.join(root, 'custom-home');
    fs.mkdirSync(def, { recursive: true });
    fs.mkdirSync(custom, { recursive: true });
    const files = m.tailnetPendingFiles(def, custom, path);
    assert.deepEqual(files, [path.join(def, 'self-hosted-on-tailnet'), path.join(custom, 'self-hosted-on-tailnet')]);
    // The legacy note only, as the old setup left it.
    fs.writeFileSync(files[1]!, 'served\n');
    const moving = m.mustSeeTailnetOff({ from: undefined, pending: files.some((f) => fs.existsSync(f)) });
    assert.equal(moving, true);
    assert.equal(m.offTailnetStep({ cli: true, handlers: undefined, tailnetPort: 3210, localPort: 13210, moving }).do, 'fail');
    assert.equal(m.offTailnetStep({ cli: false, tailnetPort: 3210, localPort: 13210, moving }).do, 'fail');
    // The default folder as --home is looked at once.
    assert.deepEqual(m.tailnetPendingFiles(def, def + '/', path), [path.join(def, 'self-hosted-on-tailnet')]);
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
});

test('setup reads and clears the note in both folders', async () => {
  const fs = await import('node:fs');
  const src = fs.readFileSync(new URL('../../../scripts/setup.mjs', import.meta.url), 'utf8');
  assert.match(src, /tailnetPendingFiles\(DEFAULT_HOME, home, path\)/);
  assert.match(src, /pending: pendingFiles\.some\(\(f\) => fs\.existsSync\(f\)\)/);
});

// An address of your own (--url, 14 Nov 2026): for someone who already
// reaches their Mac their own way, so Threadbear needn't care how.
test('--url and --site-url self-host at your own address, together, and only those', () => {
  assert.deepEqual(m.selfHostedPlan({ flags: OWN }), { selfHosted: true, mode: 'url', url: OWN_URL, site: OWN_SITE });
  assert.deepEqual(m.selfHostedPlan({ flags: { selfHosted: true, ...OWN } }), { selfHosted: true, mode: 'url', url: OWN_URL, site: OWN_SITE });
  // Trailing slashes and default ports come off; other ports stay.
  assert.deepEqual(m.selfHostedPlan({ flags: { url: 'https://Tb.Example.com:443/', siteUrl: 'https://tb.example.com:8443' } }), { selfHosted: true, mode: 'url', url: 'https://tb.example.com', site: 'https://tb.example.com:8443' });
  // One without the other, or with another way, or the same address twice.
  assert.match(m.selfHostedPlan({ flags: { url: OWN_URL } }).fail ?? '', /go together/);
  assert.match(m.selfHostedPlan({ flags: { siteUrl: OWN_SITE } }).fail ?? '', /go together/);
  assert.ok(m.selfHostedPlan({ flags: { ...OWN, local: true } }).fail);
  assert.ok(m.selfHostedPlan({ flags: { ...OWN, tailnet: true } }).fail);
  assert.match(m.selfHostedPlan({ flags: { url: OWN_URL, siteUrl: OWN_URL + '/' } }).fail ?? '', /same address/);
  // What --tailnet makes would read back as the tailnet next time, and
  // setup takes this backend's own serve off the tailnet ports, so your proxy
  // can't be on them either.
  assert.match(m.selfHostedPlan({ flags: { url: TAILNET_URL, siteUrl: 'https://mac.tail1.ts.net:8444' } }).fail ?? '', /--tailnet/);
  assert.match(m.selfHostedPlan({ flags: { url: 'https://mac.tail1.ts.net:8443', siteUrl: 'https://mac.tail1.ts.net:3211' } }).fail ?? '', /--site-url .* --tailnet uses/);
  assert.ok(m.selfHostedPlan({ flags: { url: 'https://mac.tail1.ts.net:8443', siteUrl: 'https://mac.tail1.ts.net:6791' } }).fail);
  // Those ports on a name that isn't a tailnet one are fine.
  assert.equal(m.selfHostedPlan({ flags: { url: 'https://tb.example.com:3210', siteUrl: 'https://tb.example.com:3211' } }).mode, 'url');
  // But your own tailnet name on another port is yours.
  assert.equal(m.selfHostedPlan({ flags: { url: 'https://mac.tail1.ts.net:8443', siteUrl: 'https://mac.tail1.ts.net:8444' } }).mode, 'url');
  // A Mac joining another has no backend of its own.
  assert.ok(m.selfHostedPlan({ joining: true, flags: OWN }).fail);
});

test('only an https address a phone could reach is taken as your own', () => {
  for (const bad of ['https://mac.tail1.ts.net.:3210', 'https://tb.example.com.', 'http://threadbear.example.com', 'ftp://x.example.com', 'https://127.0.0.1:13210', 'https://localhost', 'https://[::1]:3210', 'https://user:pw@tb.example.com', 'https://tb.example.com/convex', 'https://tb.example.com/?a=1', 'https://tb.example.com/#x', 'https://happy-otter-123.convex.cloud', 'https://happy-otter-123.convex.site', 'not a url', '']) {
    assert.ok(m.ownAddress(bad, '--url').fail, bad);
  }
  for (const [good, origin] of [['https://tb.example.com', 'https://tb.example.com'], ['https://tb.example.com/', 'https://tb.example.com'], ['https://192.168.1.20:8443', 'https://192.168.1.20:8443'], [' https://tb.example.com ', 'https://tb.example.com']]) {
    assert.equal(m.ownAddress(good!, '--url').origin, origin, good);
  }
});

test('setup run again keeps your own address, and a flag moves it', () => {
  const was = { adminKeyFile: true, syncUrl: OWN_URL, syncSite: OWN_SITE };
  assert.equal(m.reachOf(OWN_URL), 'url');
  assert.equal(m.reachOf(TAILNET_URL), 'tailnet');
  assert.equal(m.reachOf(LOCAL_URL), 'local');
  assert.deepEqual(m.selfHostedPlan(was), { selfHosted: true, mode: 'url', url: OWN_URL, site: OWN_SITE });
  // Plain --self-hosted (the docs' command again) keeps it there too.
  assert.deepEqual(m.selfHostedPlan({ ...was, flags: { selfHosted: true } }), { selfHosted: true, mode: 'url', url: OWN_URL, site: OWN_SITE });
  // The same addresses again aren't a change; new ones are.
  assert.deepEqual(m.selfHostedPlan({ ...was, flags: OWN }), { selfHosted: true, mode: 'url', url: OWN_URL, site: OWN_SITE });
  assert.deepEqual(m.selfHostedPlan({ ...was, flags: { url: 'https://new.example.com', siteUrl: OWN_SITE } }), { selfHosted: true, mode: 'url', url: 'https://new.example.com', site: OWN_SITE, readdressed: true });
  // Moves to and from the other two ways.
  assert.deepEqual(m.selfHostedPlan({ ...was, flags: { local: true } }), { selfHosted: true, mode: 'local', from: 'url' });
  assert.deepEqual(m.selfHostedPlan({ ...was, flags: { tailnet: true } }), { selfHosted: true, mode: 'tailnet', from: 'url' });
  assert.deepEqual(m.selfHostedPlan({ adminKeyFile: true, syncUrl: TAILNET_URL, flags: OWN }), { selfHosted: true, mode: 'url', url: OWN_URL, site: OWN_SITE, from: 'tailnet' });
  assert.deepEqual(m.selfHostedPlan({ adminKeyFile: true, syncUrl: LOCAL_URL, flags: OWN }), { selfHosted: true, mode: 'url', url: OWN_URL, site: OWN_SITE, from: 'local' });
  // A setup that stopped before saving its URL can carry on with --url.
  assert.equal(m.selfHostedPlan({ adminKeyFile: true, flags: OWN }).mode, 'url');
  assert.match(m.selfHostedPlan({ adminKeyFile: true }).fail ?? '', /--url/);
  // Kept, but with no site saved: it asks rather than guess.
  assert.match(m.selfHostedPlan({ adminKeyFile: true, syncUrl: OWN_URL }).fail ?? '', /sync\.site/);
});

test('a backend at your own address uses it as its origin, with its ports left on loopback for your proxy', () => {
  const c = m.backendContainer('url', { url: OWN_URL, site: OWN_SITE });
  assert.deepEqual(c.env, { CONVEX_CLOUD_ORIGIN: OWN_URL, CONVEX_SITE_ORIGIN: OWN_SITE, DISABLE_BEACON: 'true' });
  assert.deepEqual(c.runArgs, ['-p', '127.0.0.1:13210:3210', '-p', '127.0.0.1:13211:3211', '-v', 'threadbear-convex-data:/convex/data']);
  assert.deepEqual(c.cmd, []);
  const o = m.origins('url', { url: OWN_URL, site: OWN_SITE });
  assert.equal(o.url, OWN_URL);
  assert.equal(o.site, OWN_SITE);
  // The dashboard stays on this Mac and talks to the backend there.
  assert.equal(o.dashboard, 'http://127.0.0.1:16791');
  assert.equal(o.dashboardBackend, LOCAL_URL);
  assert.equal(m.origins('tailnet', 'mac.tail1.ts.net').dashboardBackend, TAILNET_URL);
  assert.equal(m.origins('local').dashboardBackend, LOCAL_URL);
});

test('setup checks your address from this Mac and from inside Docker before it carries on', async () => {
  const fs = await import('node:fs');
  const src = fs.readFileSync(new URL('../../../scripts/setup.mjs', import.meta.url), 'utf8');
  assert.match(src, /if \(own\) await checkOwnAddress\(url, site, \{ quiet: prechecked \}\)/);
  // Cross-lab review, 14 Nov 2026: checked against a running backend before
  // anything changes, after the admin key (which marks the Mac self-hosted)
  // on the way through, with the live connection, and not on a dotfile path.
  const pre = src.indexOf('await checkOwnAddress(url, site, { fresh: true });');
  // Waited for, not a single quick try, and from a container set up like the new one.
  assert.match(src, /if \(await answers\(ADMIN_URL, 60\)\)/);
  assert.match(src, /docker\(\['run', '--rm', '--entrypoint', curl\[0\], BACKEND_IMAGE/);
  assert.ok(pre > 0 && pre < src.indexOf('ensureContainer(BACKEND_CONTAINER'), 'pre-check before the containers change');
  assert.ok(pre < src.indexOf('unserve(ts, tailnetPort, localPort, moving);'), 'pre-check before tailnet ports come off');
  assert.ok(src.indexOf("ok(`admin key saved in") < src.indexOf('if (own) await checkOwnAddress(url, site, {'), 'admin key before the check');
  assert.match(src, /new WebSocket\(wsUrl\)/);
  assert.ok(!src.includes('/.threadbear-setup-check-'));
  assert.match(src, /docker\(\['exec', BACKEND_CONTAINER, \.\.\.curl\]\)/);
  // A url run never asks Tailscale for this Mac's name, so it takes the
  // backend's own tailnet ports off (the else of `if (tailnet)`), as a local one does.
  assert.match(src, /else if \(own\) ok\(`your own address[^\n]*\n\s*else \{\n\s*tailnet = tailnetSelf\(\);/);
});
