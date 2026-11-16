import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { tmpDir } from '../helpers/fakes.ts';
import { execFileSync } from 'node:child_process';
import { CONVEX_CLI, ConvexKeys, cliError, convexFolderFor } from '../../src/convex-keys.ts';
import { ErrorCode, type RpcError } from '../../src/protocol.ts';
import type { CommandOutcome } from '../../src/patches.ts';

// Stands in for `convex deployment token create ... --save-env <file>`.
function fakeCli(key = 'dev:calm-cat-1|secretsecret') {
  const calls: { command: string; cwd: string }[] = [];
  const run = async (command: string, cwd: string): Promise<CommandOutcome> => {
    calls.push({ command, cwd });
    const file = /--save-env '([^']+)'/.exec(command)?.[1];
    if (file) fs.writeFileSync(file, `CONVEX_DEPLOY_KEY=${key}\n`);
    return { ok: true, text: '✔ Saved deploy key', truncated: false, durationMs: 1 };
  };
  return { calls, run };
}

test('a deploy key is made once per deployment, kept private, and made again on renew', async () => {
  const dir = path.join(tmpDir(), 'secrets', 'convex-panels');
  const cli = fakeCli();
  const keys = new ConvexKeys(dir, cli.run, 'Alexs-MacBook.local');
  const [a, b] = await Promise.all([keys.key('calm-cat-1', '/proj'), keys.key('calm-cat-1', '/proj')]);
  assert.equal(a, 'dev:calm-cat-1|secretsecret');
  assert.equal(b, a);
  assert.equal(cli.calls.length, 1, 'two panels opening together make one key');
  const name = /token create (threadbear-panel-alexs-macbook-[0-9a-f]{6}) --deployment calm-cat-1 --save-env '/.exec(cli.calls[0].command)?.[1];
  assert.ok(name, cli.calls[0].command);
  assert.ok(cli.calls[0].command.startsWith(`${CONVEX_CLI} deployment token create `), cli.calls[0].command);
  assert.equal(cli.calls[0].cwd, '/proj');
  const file = path.join(dir, 'calm-cat-1.json');
  assert.equal(fs.statSync(file).mode & 0o777, 0o600);
  assert.equal(fs.statSync(dir).mode & 0o777, 0o700);
  assert.deepEqual(fs.readdirSync(dir), ['calm-cat-1.json'], 'the CLI\'s scratch file is gone');
  assert.equal(await keys.key('calm-cat-1', '/proj'), a);
  assert.equal(cli.calls.length, 1, 'kept for next time');
  // Renewing takes the old key off Convex, then makes one with a new name
  // (names are unique on a deployment).
  await keys.key('calm-cat-1', '/proj', true);
  assert.equal(cli.calls[1].command, `${CONVEX_CLI} deployment token delete ${name} --deployment calm-cat-1`);
  assert.match(cli.calls[2].command, /token create threadbear-panel-alexs-macbook-[0-9a-f]{6} /);
  assert.ok(!cli.calls[2].command.includes(name!));
  assert.equal(JSON.parse(fs.readFileSync(file, 'utf8')).name, /token create (\S+)/.exec(cli.calls[2].command)![1]);
});

// A login file the fake CLI is "signed in" with.
function loginFile(signedIn = true) {
  const file = path.join(tmpDir(), 'config.json');
  if (signedIn) fs.writeFileSync(file, JSON.stringify({ accessToken: 'tok' }));
  return file;
}

const failing = (text: string) => async (): Promise<CommandOutcome> => ({ ok: false, text, truncated: false, durationMs: 1 });

test('a failed CLI call says why in plain words, with a code the panel acts on, and makes no key file', async () => {
  const dir = path.join(tmpDir(), 'convex-panels');
  // Real convex 1.48 output, signed out.
  const out = new ConvexKeys(dir, failing('\x1b[31m✖ Creating a deploy key requires being logged in with a personal access token. Run npx convex login and try again.\x1b[0m\n'), 'mac-mini.tail1234.ts.net', { configFile: loginFile(false) });
  await assert.rejects(out.key('calm-cat-1', '/proj'), (e: RpcError) => e.code === ErrorCode.convexSignedOut
    && /The Convex CLI on mac-mini isn't signed in to Convex/.test(e.message) && !e.message.includes('tail1234'));
  assert.deepEqual(fs.readdirSync(dir), []);
  // An expired login still has a token in the file: the CLI's 401 says so.
  const expired = new ConvexKeys(dir, failing('✖ Error fetching GET https://api.convex.dev/api/x 401 Unauthorized: MissingAccessToken'), 'mac', { configFile: loginFile() });
  await assert.rejects(expired.key('calm-cat-1', '/proj'), (e: RpcError) => e.code === ErrorCode.convexSignedOut);
  const elsewhere = new ConvexKeys(dir, failing("✖ You don't have access to the selected project. Run `npx convex dev` to select a different project."), 'mac', { configFile: loginFile() });
  await assert.rejects(elsewhere.key('calm-cat-1', '/proj', false, { team: 'acme' }), (e: RpcError) => e.code === ErrorCode.convexNoAccess
    && /can't reach calm-cat-1, which is on the team acme\. Sign in with an account on that team/.test(e.message));
  const odd = new ConvexKeys(dir, failing('✖ Something else broke'), 'mac', { configFile: loginFile() });
  await assert.rejects(odd.key('calm-cat-1', '/proj'), (e: RpcError) => e.code === ErrorCode.other && /The Convex CLI said: Something else broke/.test(e.message));
  await assert.rejects(odd.key('x; rm -rf ~', '/proj'), /isn't a Convex deployment name/);
  assert.equal(cliError(''), 'the Convex CLI gave no reason');
});

test('the project folder for a deployment is the thread\'s or one of its projects\'', () => {
  const home = tmpDir();
  const mk = (rel: string, deployment: string) => {
    const dir = path.join(home, rel);
    fs.mkdirSync(path.join(dir, 'convex'), { recursive: true });
    fs.writeFileSync(path.join(dir, 'package.json'), '{}');
    fs.writeFileSync(path.join(dir, '.env.local'), `CONVEX_DEPLOYMENT=dev:${deployment}\n`);
    return dir;
  };
  const app = mk('dev/app', 'app-1');
  const web = mk('dev/web', 'web-2');
  fs.mkdirSync(path.join(app, 'src'));
  assert.equal(convexFolderFor('app-1', [path.join(app, 'src'), web], home), app);
  assert.equal(convexFolderFor('web-2', [path.join(app, 'src'), web], home), web);
  assert.equal(convexFolderFor('other-3', [app, web], home), undefined);
});

test('deployment choices resolve dev and prod through the CLI without changing project settings or making keys', async () => {
  const folder = tmpDir();
  fs.writeFileSync(path.join(folder, '.env.local'), 'CONVEX_DEPLOYMENT=dev:calm-cat-1\n');
  const calls: string[] = [];
  const keys = new ConvexKeys(path.join(folder, 'keys'), async (command, cwd) => {
    assert.equal(cwd, folder);
    calls.push(command);
    return { ok: true, text: JSON.stringify(command.includes('--deployment prod')
      ? 'https://busy-bear-2.ap-southeast-2.convex.cloud' : 'https://calm-cat-1.convex.cloud'), truncated: false, durationMs: 1 };
  });
  const configured = { deployment: 'calm-cat-1', deploymentUrl: 'https://calm-cat-1.convex.cloud' };
  const listed = await keys.deployments(folder, configured);
  assert.deepEqual(listed, { deployments: [
    { ...configured, label: 'Development' },
    { deployment: 'busy-bear-2', deploymentUrl: 'https://busy-bear-2.ap-southeast-2.convex.cloud', label: 'Production' },
  ] });
  assert.ok(calls.every(c => c.startsWith(`${CONVEX_CLI} run --inline-query`) && !c.includes('--push') && !c.includes('token')));
  assert.deepEqual(await keys.deployments(folder, configured), listed);
  assert.equal(calls.length, 2, 'discovery is cached');
  await keys.deployments(folder, configured, true);
  assert.equal(calls.length, 4, 'refresh bypasses the cache');
  assert.equal(fs.readFileSync(path.join(folder, '.env.local'), 'utf8'), 'CONVEX_DEPLOYMENT=dev:calm-cat-1\n');
  assert.ok(!fs.existsSync(path.join(folder, 'keys')));
});

test('unavailable choices leave the configured deployment usable and explain why', async () => {
  const folder = tmpDir();
  const configured = { deployment: 'calm-cat-1', deploymentUrl: 'https://calm-cat-1.convex.cloud' };
  const keys = new ConvexKeys(path.join(folder, 'keys'), async () => ({ ok: false, text: 'No production deployment', truncated: false, durationMs: 1 }), 'mac', { configFile: loginFile() });
  const listed = await keys.deployments(folder, configured);
  assert.deepEqual(listed.deployments, [{ ...configured, label: 'Configured' }]);
  assert.match(listed.note!, /Production: No production deployment/);
});

// THREADBEAR-6: a project on convex 1.32 has no `deployment token` command
// or `--inline-query`, so its own CLI printed its help instead. The runner's
// own CLI works in any project folder, whatever version the project is on.
test('the runner\'s own Convex CLI is used, not the project\'s older one', () => {
  const folder = tmpDir();
  const old = path.join(folder, 'node_modules', 'convex');
  fs.mkdirSync(path.join(old, 'bin'), { recursive: true });
  fs.writeFileSync(path.join(old, 'package.json'), JSON.stringify({ name: 'convex', version: '1.32.0', bin: { convex: 'bin/main.js' } }));
  fs.writeFileSync(path.join(old, 'bin', 'main.js'), 'console.log("Usage: convex [options] [command]")');
  fs.writeFileSync(path.join(folder, 'package.json'), JSON.stringify({ dependencies: { convex: '1.32.0' } }));
  const help = (args: string) => execFileSync('/bin/sh', ['-c', `${CONVEX_CLI} ${args} --help`], { cwd: folder, encoding: 'utf8' });
  assert.match(help('deployment token create'), /Creates a deploy key/);
  assert.match(help('deployment token delete'), /Usage: convex deployment token delete/);
  assert.match(help('run'), /--inline-query/);
});

test('signed out, the deployment list says so once instead of the CLI\'s error twice', async () => {
  const folder = tmpDir();
  const configured = { deployment: 'calm-cat-1', deploymentUrl: 'https://calm-cat-1.convex.cloud' };
  const keys = new ConvexKeys(path.join(folder, 'keys'), failing('✖ 401 Unauthorized: MissingAccessToken'), 'mac', { configFile: loginFile(false) });
  const listed = await keys.deployments(folder, configured);
  assert.deepEqual(listed, { deployments: [{ ...configured, label: 'Configured' }], note: "Sign in to Convex to list the project's other deployments." });
});

test('a saved key that Convex says is revoked is replaced by itself; the check only goes to the deployment\'s own address', async () => {
  const dir = path.join(tmpDir(), 'convex-panels');
  const cli = fakeCli();
  const checked: string[] = [];
  let verdict: 'ok' | 'bad' = 'ok';
  const keys = new ConvexKeys(dir, cli.run, 'mac', { configFile: loginFile(), checkKey: async (url) => { checked.push(url); return verdict; } });
  const at = { deploymentUrl: 'https://calm-cat-1.ap-southeast-2.convex.cloud' };
  await keys.key('calm-cat-1', '/proj', false, at);
  assert.equal(cli.calls.length, 1);
  assert.deepEqual(checked, [], 'a key just made isn\'t checked');
  await keys.key('calm-cat-1', '/proj', false, at);
  assert.deepEqual(checked, ['https://calm-cat-1.ap-southeast-2.convex.cloud']);
  assert.equal(cli.calls.length, 1, 'a working key is kept');
  verdict = 'bad';
  await keys.key('calm-cat-1', '/proj', false, at);
  assert.match(cli.calls[1].command, /deployment token delete threadbear-panel-mac-/);
  assert.match(cli.calls[2].command, /deployment token create /);
  // A project's env file could name any URL: the key never goes there.
  await keys.key('calm-cat-1', '/proj', false, { deploymentUrl: 'https://evil.example' });
  await keys.key('calm-cat-1', '/proj', false, { deploymentUrl: 'https://calm-cat-1.convex.cloud.evil.example' });
  assert.equal(checked.length, 2);
  assert.equal(cli.calls.length, 3);
});

// Stands in for `convex login --login-flow poll`: prints the page and code
// like convex 1.48 does, then "approves" by writing the login file.
function fakeLogin(config: string, { exit = 0, approveAfterMs = 150, write = true } = {}) {
  const script = `
    console.log('Visit https://auth.convex.dev/device?user_code=BRTV-XJWX to finish logging in.');
    console.log('You should see the following code which expires in 299 seconds: BRTV-XJWX');
    setTimeout(() => {
      if (${write}) require('fs').writeFileSync(${JSON.stringify(config)}, JSON.stringify({ accessToken: 'new' }));
      if (${exit}) console.error('✖ Login denied');
      process.exit(${exit});
    }, ${approveAfterMs});`;
  return [process.execPath, '-e', script];
}

async function until<T extends { state: string }>(get: () => Promise<T>, state: string): Promise<T> {
  for (let i = 0; i < 300; i++) {
    const s = await get();
    if (s.state === state) return s;
    await new Promise((r) => setTimeout(r, 50));
  }
  throw new Error(`never reached ${state}`);
}

test('a sign-in from the panel hands back the page and code, then finishes by itself and lists deployments again', async () => {
  const config = loginFile(false);
  let listings = 0;
  const keys = new ConvexKeys(path.join(tmpDir(), 'keys'), async () => { listings++; return { ok: false, text: 'nope', truncated: false, durationMs: 1 }; }, 'mac', { configFile: config, login: fakeLogin(config) });
  assert.deepEqual(await keys.login(true), { state: 'idle' });
  const folder = tmpDir();
  await keys.deployments(folder, { deployment: 'calm-cat-1', deploymentUrl: 'https://calm-cat-1.convex.cloud' });
  const started = await keys.login();
  assert.deepEqual(started, { state: 'waiting', url: 'https://auth.convex.dev/device?user_code=BRTV-XJWX', code: 'BRTV-XJWX' });
  assert.equal((await keys.login()).state, 'waiting', 'a second click joins the sign-in going');
  await until(() => keys.login(true), 'done');
  assert.ok(keys.signedIn());
  await keys.deployments(folder, { deployment: 'calm-cat-1', deploymentUrl: 'https://calm-cat-1.convex.cloud' });
  assert.equal(listings, 4, 'the cached list went with the new login');
});

test('a sign-in that fails or is never approved says so, and can be tried again', async () => {
  const config = loginFile(false);
  const denied = new ConvexKeys(tmpDir(), failing(''), 'mac', { configFile: config, login: fakeLogin(config, { exit: 1, write: false }) });
  await denied.login();
  const failed = await until(() => denied.login(true), 'failed');
  assert.match(failed.message!, /The sign-in didn't finish\. The Convex CLI said: Login denied/);
  const slow = new ConvexKeys(tmpDir(), failing(''), 'mac', { configFile: config, login: fakeLogin(config, { approveAfterMs: 60_000 }), loginTimeoutMs: 5000 });
  await slow.login();
  assert.equal((await until(() => slow.login(true), 'failed')).message, "The sign-in wasn't approved in time. Try again.");
  assert.equal((await slow.login()).state, 'waiting', 'a new click starts again');
  await slow.login(true);
});
