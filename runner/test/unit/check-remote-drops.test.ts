// scripts/check-remote.sh against fake hosts (helpers/check-remote-harness.ts
// says how, and why it's several files): hosts that stop answering, connections lost, and runs past their limits.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { GiB, SCRIPT, macDf, setup, shortHome } from '../helpers/check-remote-harness.ts';

// THREADBEAR_NODE: find-node.sh finds the Node nvm installed in its home.
function onHost(checkExit: number, df: string, opts: { hang?: boolean; leave?: string; env?: Record<string, string> } = {}) {
  const home = shortHome();
  const base = path.join(home, '.cache', 'threadbear-check');
  const copy = path.join(base, 'runs', '1-1-0');
  fs.mkdirSync(path.join(copy, 'scripts'), { recursive: true });
  fs.copyFileSync(SCRIPT, path.join(copy, 'scripts', 'check-remote.sh'));
  fs.copyFileSync(path.join(path.dirname(SCRIPT), 'limit-step.mjs'), path.join(copy, 'scripts', 'limit-step.mjs'));
  fs.mkdirSync(path.join(copy, 'runner', 'scripts'), { recursive: true });
  fs.copyFileSync(path.join(path.dirname(SCRIPT), '..', 'runner', 'scripts', 'find-node.sh'), path.join(copy, 'runner', 'scripts', 'find-node.sh'));
  fs.writeFileSync(path.join(copy, 'check.sh'), `#!/bin/bash\necho made >"$TMPDIR/big"\n${opts.hang ? 'sleep 60\n' : ''}${opts.leave ? `mkdir -p "$TMPDIR/wt"; (cd "$TMPDIR/wt" && exec sleep 300) >/dev/null 2>&1 </dev/null &\necho $! >'${opts.leave}'\n(exec sleep 300) >/dev/null 2>&1 </dev/null &\necho $! >>'${opts.leave}'\n` : ''}exit ${checkExit}\n`, { mode: 0o755 });
  const nodeBin = path.join(home, '.nvm', 'versions', 'node', 'v24.0.0', 'bin');
  fs.mkdirSync(nodeBin, { recursive: true });
  fs.symlinkSync(process.execPath, path.join(nodeBin, 'node'));
  fs.writeFileSync(path.join(nodeBin, 'df'), `#!/bin/bash\n[ "$*" = "-Pk ." ] || exit 9\ncat <<'END'\n${df}END\n`, { mode: 0o755 });
  const empty = path.join(home, 'empty-bin');
  fs.mkdirSync(empty, { recursive: true });
  const env: Record<string, string | undefined> = { ...process.env, HOME: home, PATH: empty, ...opts.env };
  delete env.THREADBEAR_NODE;
  delete env.NVM_DIR;
  const r = spawnSync('/bin/bash', [path.join(copy, 'scripts', 'check-remote.sh'), '--on-host', '1-1-0'], { encoding: 'utf8', env });
  return { r, base, kept: path.join(base, 'kept-1-1-0', 'free-kb') };
}

test('--on-host records the free space of a failed run for this Mac, before clearing up', () => {
  const { r, base, kept } = onHost(3, macDf(GiB));
  assert.equal(r.status, 3, r.stdout + r.stderr);
  assert.equal(fs.readFileSync(kept, 'utf8').trim(), String(GiB));
  assert.ok(!fs.existsSync(path.join(base, 'tmp-1-1-0')), 'temp files gone');
  assert.ok(!fs.existsSync(path.join(base, 'src')), 'copy gone');
});

test('--on-host records nothing when the run passed', () => {
  const { r, kept } = onHost(0, macDf(GiB));
  assert.equal(r.status, 0, r.stdout + r.stderr);
  assert.ok(!fs.existsSync(kept));
});

// A test that timed out left a `tb land` in its temp worktree, and with the
// run gone it sat on Tinker Desk for days (7 to 14 Nov).
test('--on-host stops what the run left working in its temp folder or its copy', () => {
  const pidFile = path.join(shortHome(), 'left.pid');
  const { r } = onHost(0, macDf(100 * GiB), { leave: pidFile });
  assert.equal(r.status, 0, r.stdout + r.stderr);
  // One in the temp folder, one in the run's copy of the checkout.
  const pids = fs.readFileSync(pidFile, 'utf8').trim().split('\n').map(Number);
  assert.equal(pids.length, 2);
  for (const pid of pids) assert.throws(() => process.kill(pid, 0), /ESRCH/, `${pid} is gone`);
});

// A host that stops answering partway: 8 Nov, Tinker Desk stopped taking ssh
// connections and a check sat in its ssh for 79 minutes, holding every landing.
const quickPing = { THREADBEAR_CHECK_PING_SECS: '1', THREADBEAR_CHECK_PING_MISSES: '2' };

test('a host that stops answering during the check is dropped, and the next one runs it', () => {
  const s = setup({ h1: { state: 'free', hang: true }, h2: { state: 'free' } });
  const started = Date.now();
  const r = s.run('h1,h2', 3600, '', quickPing);
  assert.equal(r.status, 0, r.stdout + r.stderr);
  assert.ok(Date.now() - started < 60_000, 'not left to wait out the hang');
  assert.match(r.stderr, /h1 stopped answering over ssh \(2 tries in a row, 1 s apart, couldn't log in\); trying the others\./);
  assert.match(r.stdout, /ran on h2 now/);
  assert.match(r.ssh, /^-O exit h1$/m, "h1's shared connection is closed");
});

test('with no other host, one that stops answering ends as unreachable, so check.sh runs it here', () => {
  const s = setup({ h1: { state: 'free', hang: true } });
  const r = s.run('h1', 3600, '', quickPing);
  assert.equal(r.status, 255, r.stdout + r.stderr);
  assert.match(r.stderr, /h1 stopped answering/);
  assert.match(r.stderr, /None of h1 can be reached/);
});

test('a host that stops answering during the copy is dropped too', () => {
  const s = setup({ h1: { state: 'free', hangCopy: true }, h2: { state: 'free' } });
  const r = s.run('h1,h2', 3600, '', quickPing);
  assert.equal(r.status, 0, r.stdout + r.stderr);
  assert.match(r.stderr, /h1 stopped answering/);
  assert.match(r.stdout, /ran on h2 now/);
});

test('a host that is slow but still answers its pings is left to finish', () => {
  // Every ping logs in, so a check that's merely quiet is never cut off.
  const s = setup({ h1: { state: 'free' } });
  const r = s.run('h1', 3600, 'sleep 4', quickPing);
  assert.equal(r.status, 0, r.stdout + r.stderr);
  assert.doesNotMatch(r.stderr, /stopped answering/);
  assert.match(r.ssh, /^h1 true$/m, 'it was pinged while it ran');
});

test('a run past the overall limit is stopped, and not as unreachable', () => {
  const s = setup({ h1: { state: 'free' } });
  const r = s.run('h1', 3600, 'sleep 30', { THREADBEAR_CHECK_PING_SECS: '1', THREADBEAR_CHECK_MAX_SECS: '3' });
  assert.equal(r.status, 1, r.stdout + r.stderr);
  assert.match(r.stderr, /This check has been going for 0 minutes, waiting in line included, so it was stopped\./);
});

test('a connection lost during the check goes to the next host, and with none to this Mac only if the host is gone (255)', () => {
  const s = setup({ h1: { state: 'free', drop: true }, h2: { state: 'free' } });
  const r = s.run('h1,h2', 3600);
  assert.equal(r.status, 0, r.stdout + r.stderr);
  assert.match(r.stderr, /Lost the connection to h1 during the check; trying the others\./);
  assert.match(r.stdout, /ran on h2 now/);
  // The host gone with it: unreachable, so this Mac's.
  const gone = setup({ h1: { state: 'free', drop: true, dropDead: true } }).run('h1', 3600);
  assert.equal(gone.status, 255, gone.stdout + gone.stderr);
  // Still answering a new connection: the check's channel broke, not the
  // host, so it fails rather than run here (Codex, 14 Nov).
  const alone = setup({ h1: { state: 'free', drop: true } }).run('h1', 3600);
  assert.equal(alone.status, 1, alone.stdout + alone.stderr);
  assert.match(alone.stderr, /h1: the check's connection broke, though it still answers/);
});

test('--on-host stops a run past its own limit, so a host this Mac gave up on frees its lock', () => {
  const started = Date.now();
  const { r } = onHost(0, macDf(100 * GiB), { hang: true, env: { THREADBEAR_CHECK_HOST_MAX_SECS: '2' } });
  // Not 124, which this Mac takes for a host that dropped out.
  assert.equal(r.status, 1, r.stdout + r.stderr);
  assert.ok(Date.now() - started < 40_000);
  assert.match(r.stdout, /limit-step: error: still going after 2 s/);
});

test('a hung check that ignores SIGTERM is still killed, and the next host runs it', () => {
  const s = setup({ h1: { state: 'free', stubborn: true }, h2: { state: 'free' } });
  const started = Date.now();
  const r = s.run('h1,h2', 3600, '', quickPing);
  assert.equal(r.status, 0, r.stdout + r.stderr);
  assert.ok(Date.now() - started < 60_000, `took ${Date.now() - started} ms`);
  assert.match(r.stdout, /ran on h2 now/);
});

test("a host whose probe fails as ssh's own 255 but takes a fresh connection is never this Mac's to stand in for", () => {
  const s = setup({ h1: { state: 'free' } });
  fs.writeFileSync(path.join(s.fake, 'h1', 'probe-255'), '');
  const r = s.run('h1', 3600);
  assert.equal(r.status, 1, r.stdout + r.stderr);
  assert.match(r.stderr, /but h1 still answers, so it doesn't run on this Mac instead/);
});

test('a login whose shell exits 124 still counts as answering, not as a timeout', () => {
  const s = setup({ h1: { state: 'free' } });
  fs.writeFileSync(path.join(s.fake, 'h1', 'probe-255'), '');
  fs.writeFileSync(path.join(s.fake, 'h1', 'login-124'), '');
  const r = s.run('h1', 3600);
  assert.equal(r.status, 1, r.stdout + r.stderr);
  assert.match(r.stderr, /but h1 still answers/);
});
