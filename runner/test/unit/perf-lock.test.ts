// Real kernel locks and harmless sleeping children, never a build or a window.
import { spawn, spawnSync, type ChildProcess } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import assert from 'node:assert/strict';
import { test, type TestContext } from 'node:test';

const root = path.resolve(import.meta.dirname, '../../..');
const helper = path.join(root, 'scripts/perf-lock.pl');
const pause = (ms: number) => new Promise((r) => setTimeout(r, ms));
async function until(check: () => boolean, message: string) {
  for (let i = 0; i < 500; i++) {
    if (check()) return;
    await pause(40);
  }
  assert.fail(message);
}
function host(t: TestContext) {
  const home = fs.mkdtempSync(path.join(os.tmpdir(), 'tb-perf-lock-'));
  const env = { ...process.env, HOME: home, THREADBEAR_PERF_LOCK_OWNER: '' };
  const children: (ChildProcess & { done: Promise<number | null> })[] = [];
  t.after(async () => {
    fs.writeFileSync(path.join(home, 'resume-scan'), '');
    fs.rmSync(path.join(home, 'fail-scan'), { force: true });
    fs.rmSync(path.join(home, 'blocked-probe'), { force: true });
    for (const child of children) child.stdin?.end();
    for (const child of children) await child.done;
    fs.rmSync(home, { recursive: true, force: true });
  });
  function run(args: string[], extra: NodeJS.ProcessEnv = {}, deskPid?: string, detached = false, program = helper) {
    const child = spawn('perl', [program, ...(deskPid ? ['--desk', deskPid] : ['-']), ...args], { env: { ...env, ...extra }, stdio: ['pipe', 'pipe', 'pipe'], detached });
    const result = child as ChildProcess & { output: string; done: Promise<number | null> };
    result.output = '';
    child.stdout!.on('data', (s) => { result.output += s; });
    child.stderr!.on('data', (s) => { result.output += s; });
    result.done = new Promise((r) => child.on('exit', r));
    children.push(result);
    return result;
  }
  return { home, env, run };
}

function pausedSnapshot(home: string) {
  const bin = path.join(home, 'bin');
  fs.mkdirSync(bin);
  fs.writeFileSync(path.join(bin, 'ps'), `#!/usr/bin/perl
    use strict; use warnings;
    my $base = $ENV{HOME};
    if ($ARGV[0] eq '-axo' && $ARGV[1] eq 'pid=,ppid=,pgid=,uid=,stat=,lstart=' && -e "$base/fail-scan") {
      open(my $seen, '>', "$base/scan-failed"); close $seen; exit 1;
    }
    if ($ARGV[0] eq '-axo' && $ARGV[1] eq 'pid=,ppid=,pgid=,uid=,stat=,lstart=' && unlink("$base/pause-scan")) {
      open(my $ps, '-|', '/bin/ps', @ARGV) or die $!;
      my $rows = do { local $/; <$ps> }; close $ps or die 'ps failed';
      open(my $out, '>', "$base/snapshot") or die $!; print {$out} $rows; close $out;
      open(my $ready, '>', "$base/scan-ready") or die $!; close $ready;
      select(undef, undef, undef, 0.02) until -e "$base/resume-scan";
      open(my $saved, '<', "$base/snapshot") or die $!; print while <$saved>; exit;
    }
    exec '/bin/ps', @ARGV;
  `, { mode: 0o755 });
  fs.writeFileSync(path.join(home, 'pause-scan'), '');
  return { PATH: `${bin}:${process.env.PATH}` };
}

test('measurements wait, explain why, and keep both locks for their whole run', async (t) => {
  const h = host(t);
  const first = h.run(['sh', '-c', 'echo first; sleep 1']);
  await until(() => first.output.includes('first'), 'first did not start');
  const second = h.run(['sh', '-c', 'echo second']);
  await until(() => second.output.includes('Waiting for'), 'no waiting message');
  assert.ok(!second.output.includes('second'));
  assert.equal(await first.done, 0);
  assert.equal(await second.done, 0);
  assert.match(second.output, /second/);
});

test('an updater defers without queueing when either host lock is busy', async (t) => {
  const h = host(t);
  for (const name of ['.cache/threadbear-perf.lock', '.cache/threadbear-check/lock']) {
    fs.mkdirSync(path.dirname(path.join(h.home, name)), { recursive: true });
    const holder = spawn('perl', ['-MFcntl=:flock', '-e', 'open(my $f, ">>", $ARGV[0]) or die $!; flock($f, LOCK_EX) or die $!; $|=1; print "held\\n"; sleep 60', path.join(h.home, name)]);
    const done = new Promise((resolve) => holder.on('exit', resolve));
    t.after(() => holder.kill());
    await new Promise((resolve) => holder.stdout!.once('data', resolve));
    // h.run adds the pidfile argument; put --try before it in a tiny shim.
    const shim = path.join(h.home, 'try.pl');
    fs.writeFileSync(shim, `exec 'perl', ${JSON.stringify(helper)}, '--try', @ARGV;`);
    const updater = h.run(['sh', '-c', 'echo updater-ran'], {}, undefined, false, shim);
    const result = await Promise.race([updater.done, pause(2000).then(() => 'queued')]);
    if (result === 'queued') updater.stdin!.end();
    assert.equal(result, 75, updater.output);
    assert.doesNotMatch(updater.output, /updater-ran/);
    holder.kill();
    await done;
    // In particular, the first lock isn't retained while deferring on the second.
    const next = h.run(['sh', '-c', 'echo next']);
    assert.equal(await next.done, 0, next.output);
  }
});

test('an updater leaves a free host to an existing live waiter', async (t) => {
  const h = host(t);
  const waiting = path.join(h.home, '.cache/threadbear-perf.lock.waiting');
  fs.mkdirSync(waiting, { recursive: true });
  fs.writeFileSync(path.join(waiting, String(process.pid)), `${Math.floor(Date.now() / 1000)}\n`);
  const shim = path.join(h.home, 'try.pl');
  fs.writeFileSync(shim, `exec 'perl', ${JSON.stringify(helper)}, '--try', @ARGV;`);
  const updater = h.run(['sh', '-c', 'echo updater-ran'], {}, undefined, false, shim);
  assert.equal(await updater.done, 75, updater.output);
  assert.doesNotMatch(updater.output, /updater-ran/);
  // A note left by a killed waiter, whose pid now belongs to this newer
  // process, cannot hold updates forever.
  fs.writeFileSync(path.join(waiting, String(process.pid)), '1\n');
  const reused = h.run(['sh', '-c', 'echo updater-ran'], {}, undefined, false, shim);
  assert.equal(await reused.done, 0, reused.output);
  fs.rmSync(path.join(waiting, String(process.pid)));
  fs.writeFileSync(path.join(waiting, '99999999'), '1\n');
  const free = h.run(['sh', '-c', 'echo updater-ran'], {}, undefined, false, shim);
  assert.equal(await free.done, 0, free.output);
  assert.match(free.output, /updater-ran/);
});

test('a waiter is told who holds the perf lock, and the note goes with the holder', async (t) => {
  // lsof can't tell a holder from a waiter: both have the file open.
  const h = host(t);
  const note = path.join(h.home, '.cache/threadbear-perf.lock.holder');
  const first = h.run(['sh', '-c', 'echo first; sleep 3'], { THREADBEAR_PERF_LOCK_LABEL: 'remote.sh from slot-9 --desk: film the paw' });
  await until(() => first.output.includes('first'), 'first did not start');
  assert.match(fs.readFileSync(note, 'utf8'), new RegExp(`^${first.pid}\\n\\d+\\nremote.sh from slot-9 --desk: film the paw\\n$`));
  const second = h.run(['sh', '-c', 'echo second']);
  await until(() => second.output.includes('Waiting for'), 'no waiting message');
  assert.match(second.output, new RegExp(`held by perf-lock pid ${first.pid} for 0m, running: remote.sh from slot-9 --desk: film the paw\\.\\.\\.`));
  await pause(1100); // the queue is ordered by whole seconds
  const third = h.run(['sh', '-c', 'echo third']);
  await until(() => third.output.includes('Waiting for'), 'third did not wait');
  assert.match(third.output, /; 1 other run waiting since before this one\.\.\./);
  assert.equal(await first.done, 0);
  assert.equal(await second.done, 0);
  assert.equal(await third.done, 0);
  assert.ok(!fs.existsSync(note), 'the last holder left its note behind');
  assert.deepEqual(fs.readdirSync(path.join(h.home, '.cache/threadbear-perf.lock.waiting')), [], 'a waiter left its file behind');
});

test('a check lock acquired first blocks measurement, and no check can start during it', async (t) => {
  const h = host(t);
  fs.mkdirSync(path.join(h.home, '.cache/threadbear-check'), { recursive: true });
  const lock = path.join(h.home, '.cache/threadbear-check/lock');
  const check = spawn('perl', ['-MFcntl=:flock', '-e', 'open(my $f, ">>", $ARGV[0]) or die $!; flock($f, LOCK_EX); $|=1; print "ready\\n"; sleep 1', lock], { stdio: ['ignore', 'pipe', 'inherit'] });
  let ready = '';
  check.stdout!.on('data', (s) => { ready += s; });
  await until(() => ready.includes('ready'), 'check did not take lock');
  const run = h.run(['sh', '-c', 'echo measuring; sleep 1']);
  await until(() => run.output.includes('Waiting for'), 'measurement did not wait');
  assert.ok(!run.output.includes('measuring'));
  await until(() => run.output.includes('measuring'), 'measurement did not start');
  const probe = spawn('perl', ['-MFcntl=:flock', '-e', 'open(my $f, ">>", $ARGV[0]) or die $!; exit(flock($f, LOCK_EX|LOCK_NB) ? 0 : 75)', lock]);
  assert.equal(await new Promise((r) => probe.on('exit', r)), 75);
  assert.equal(await run.done, 0);
});

test('nested helper calls and node entry points inherit one lock without deadlock', async (t) => {
  const h = host(t);
  const entry = path.join(h.home, 'entry.mjs');
  const api = path.join(root, 'features/tools/perf-lock.ts');
  fs.writeFileSync(entry, `import { ensurePerfLock } from ${JSON.stringify(api)}; await ensurePerfLock(); console.log('nested');`);
  const child = h.run(['perl', helper, '-', process.execPath, entry]);
  assert.equal(await child.done, 0, child.output);
  assert.match(child.output, /nested/);
  assert.doesNotMatch(child.output, /Waiting/);
});

for (const signal of ['SIGTERM', 'SIGKILL'] as const) test(`${signal} retains locks until a TERM-resistant descendant has stopped`, async (t) => {
  const h = host(t);
  const pidfile = path.join(h.home, 'pid');
  const first = h.run(['sh', '-c', 'trap "" TERM; echo $$ > "$1"; echo ready; while :; do sleep 1; done', 'sh', pidfile]);
  await until(() => first.output.includes('ready'), 'child did not start');
  const pid = Number(fs.readFileSync(pidfile, 'utf8'));
  first.kill(signal);
  const second = h.run(['sh', '-c', 'echo next']);
  await until(() => second.output.includes('Waiting'), 'lock released before cleanup');
  assert.ok(!second.output.includes('next'));
  await first.done;
  assert.equal(await second.done, 0, second.output);
  assert.throws(() => process.kill(pid, 0));
});

test('stdin closing cancels a queued measurement before its command starts', async (t) => {
  const h = host(t);
  const first = h.run(['sh', '-c', 'echo ready; sleep 1']);
  await until(() => first.output.includes('ready'), 'first did not start');
  const second = h.run(['sh', '-c', 'echo should-not-run']);
  await until(() => second.output.includes('Waiting'), 'second did not wait');
  second.stdin!.end();
  assert.notEqual(await second.done, 0);
  assert.doesNotMatch(second.output, /should-not-run/);
});

test('direct node tools acquire the locks and preserve a failing exit status', async (t) => {
  const h = host(t);
  const entry = path.join(h.home, 'entry.mjs');
  fs.writeFileSync(entry, `import { ensurePerfLock } from ${JSON.stringify(path.join(root, 'features/tools/perf-lock.ts'))}; await ensurePerfLock(); console.log('entry ran'); process.exit(17);`);
  const child = spawn(process.execPath, [entry], { env: h.env });
  let out = '';
  child.stdout!.on('data', (s) => { out += s; });
  assert.equal(await new Promise((r) => child.on('exit', r)), 17);
  assert.match(out, /entry ran/);
});


test('legacy lockf jobs contend with both helper locks on macOS', { skip: process.platform !== 'darwin' }, async (t) => {
  const h = host(t);
  fs.mkdirSync(path.join(h.home, '.cache/threadbear-check'), { recursive: true });
  const perf = path.join(h.home, '.cache/threadbear-perf.lock');
  const check = path.join(h.home, '.cache/threadbear-check/lock');
  const legacy = spawn('/usr/bin/lockf', ['-k', perf, '/usr/bin/lockf', '-k', check, 'sh', '-c', 'echo legacy; sleep 1']);
  let output = '';
  legacy.stdout!.on('data', (s) => { output += s; });
  await until(() => output.includes('legacy'), 'legacy job did not acquire locks');
  const measured = h.run(['sh', '-c', 'echo measuring; sleep 1']);
  await until(() => measured.output.includes('Waiting'), 'helper did not contend with lockf');
  assert.doesNotMatch(measured.output, /measuring/);
  await until(() => measured.output.includes('measuring'), 'measurement did not start');
  for (const file of [perf, check]) {
    const probe = spawnSync('/usr/bin/lockf', ['-k', '-t', '0', file, 'true']);
    assert.equal(probe.status, 75, file);
  }
  assert.equal(await measured.done, 0);
});

test('remote build wrappers background builds without lowering the benchmark shell', (t) => {
  const h = host(t);
  const bin = path.join(h.home, 'bin');
  fs.mkdirSync(bin);
  fs.writeFileSync(path.join(bin, 'cargo'), '#!/bin/sh\necho fake-cargo\n', { mode: 0o755 });
  const run = spawnSync('/bin/bash', ['-c', '. "$1"; cargo build; echo benchmark; test -d "$threadbear_build_bin"', 'bash', path.join(root, 'scripts/build-priority.sh')], {
    env: { ...h.env, TMPDIR: h.home, PATH: `${bin}:${process.env.PATH}` }, encoding: 'utf8',
  });
  assert.equal(run.status, 0, run.stderr);
  assert.match(run.stdout, /fake-cargo\nbenchmark/);
  assert.ok(!fs.readdirSync(h.home).some((n) => n.startsWith('threadbear-build-bin.')));
});

test('real-screen stays a safe import inside a check already holding the host lock', () => {
  const shared = fs.readFileSync(path.join(root, 'features/tools/real-screen.ts'), 'utf8');
  assert.doesNotMatch(shared, /ensurePerfLock/);
  for (const name of ['scroll-perf.ts', 'switch-perf.ts', 'long-thread.ts', 'real-runner.ts', 'real-gestures.ts', 'real-select.ts', 'real-typing.ts', 'real-review-paste.ts']) {
    assert.match(fs.readFileSync(path.join(root, 'features/tools', name), 'utf8'), /await ensurePerfLock\(\)/, name);
  }
});

test('free check probes keep the same inode that waiting measurements have open', () => {
  const check = fs.readFileSync(path.join(root, 'scripts/check-remote.sh'), 'utf8');
  for (const probe of check.matchAll(/\/usr\/bin\/lockf -t 0 ([^;\n]*?)true/g)) {
    assert.match(probe[1], /\b-k\b|^-k\s/, probe[0]);
  }
});

test('Desk Runner receives lock ownership explicitly through a fresh service environment', async (t) => {
  const h = host(t);
  const bin = path.join(h.home, '.local/bin');
  fs.mkdirSync(bin, { recursive: true });
  fs.writeFileSync(path.join(bin, 'desk'), '#!/bin/sh\nexec env -i HOME="$HOME" PATH="$PATH" /bin/bash -c "$1"\n', { mode: 0o755 });
  const entry = path.join(h.home, "desk entry's test.mjs");
  fs.writeFileSync(entry, `import fs from 'node:fs'; import { ensurePerfLock } from ${JSON.stringify(path.join(root, 'features/tools/perf-lock.ts'))}; await ensurePerfLock(); if (fs.readFileSync(process.argv[2], 'utf8').trim() !== String(process.pid)) throw Error('wrapper did not record desk pid'); console.log('desk inherited');`);
  const child = h.run([process.execPath, entry, path.join(h.home, 'desk.pid')], {}, path.join(h.home, 'desk.pid'));
  assert.equal(await child.done, 0, child.output);
  assert.match(child.output, /desk inherited/);
  assert.doesNotMatch(child.output, /Waiting/);
});

test('an old unlinking lockf owner cannot strand the helper on a deleted inode', { skip: process.platform !== 'darwin' }, async (t) => {
  const h = host(t);
  fs.mkdirSync(path.join(h.home, '.cache'), { recursive: true });
  const lock = path.join(h.home, '.cache/threadbear-perf.lock');
  const legacy = spawn('/usr/bin/lockf', [lock, 'sh', '-c', 'echo ready; sleep 1']);
  let output = '';
  legacy.stdout!.on('data', (s) => { output += s; });
  await until(() => output.includes('ready'), 'legacy job did not start');
  const measured = h.run(['sh', '-c', 'echo measuring; sleep 1']);
  await until(() => measured.output.includes('Waiting'), 'measurement did not wait');
  await until(() => measured.output.includes('measuring'), 'measurement did not acquire current inode');
  assert.equal(spawnSync('/usr/bin/lockf', ['-k', '-t', '0', lock, 'true']).status, 75);
  assert.equal(await measured.done, 0, measured.output);
});

test('a whole process-group TERM leaves the guard alive to stop its workload', async (t) => {
  const h = host(t);
  const pidfile = path.join(h.home, 'pid');
  const first = h.run(['sh', '-c', 'trap "" TERM; echo $$ > "$1"; echo ready; while :; do sleep 1; done', 'sh', pidfile], {}, undefined, true);
  await until(() => first.output.includes('ready'), 'child did not start');
  const pid = Number(fs.readFileSync(pidfile, 'utf8'));
  process.kill(-first.pid!, 'SIGTERM');
  const next = h.run(['sh', '-c', 'echo next']);
  await until(() => next.output.includes('Waiting'), 'turn released before cleanup');
  await first.done;
  assert.equal(await next.done, 0);
  assert.throws(() => process.kill(pid, 0));
});

test('KILL of the actual measurement worker reaps its detached driver children before unlock', async (t) => {
  const h = host(t);
  const workerPid = path.join(h.home, 'worker.pid');
  const appPid = path.join(h.home, 'app.pid');
  const entry = path.join(h.home, 'worker.mjs');
  const registryFile = path.join(h.home, 'registry');
  fs.writeFileSync(entry, `import fs from 'node:fs'; import { GpuiDriver } from ${JSON.stringify(path.join(root, 'features/support/gpui.ts'))}; fs.writeFileSync(${JSON.stringify(registryFile)}, process.env.THREADBEAR_PERF_CHILDREN); const driver = new GpuiDriver(); driver.child('sh', ['-c', 'trap "" TERM; echo $$ > "$1"; while :; do sleep 1; done', 'sh', ${JSON.stringify(appPid)}], {}); fs.writeFileSync(${JSON.stringify(workerPid)}, String(process.pid)); setInterval(() => {}, 1000);`);
  const first = h.run([process.execPath, entry]);
  await until(() => fs.existsSync(appPid), 'driver child did not start');
  const worker = Number(fs.readFileSync(workerPid, 'utf8'));
  const app = Number(fs.readFileSync(appPid, 'utf8'));
  process.kill(worker, 'SIGKILL');
  const registry = fs.readFileSync(registryFile, 'utf8');
  await until(() => fs.existsSync(`${registry}.reaping`), 'registry was not atomically retired');
  const lateFile = path.join(h.home, 'late-started');
  const late = spawn('perl', [path.join(root, 'scripts/perf-children.pl'), '--child', registry, 'touch', lateFile], { detached: true });
  assert.equal(await new Promise((r) => late.on('exit', r)), 129);
  assert.ok(!fs.existsSync(lateFile), 'late workload started during registry removal');
  const next = h.run(['sh', '-c', 'echo next']);
  await until(() => next.output.includes('Waiting'), 'worker death released locks before reaping apps');
  assert.equal(await first.done, 137, first.output);
  assert.equal(await next.done, 0, next.output);
  assert.throws(() => process.kill(app, 0));
  assert.ok(!fs.readdirSync(path.join(h.home, '.cache/threadbear-check')).some((n) => n.startsWith('perf-children-')));
});

for (const loss of ['worker', 'owner', 'transport', 'app', 'app-pty', 'closed-proof'] as const) test(`${loss} loss stops an app's separate terminal session before the next lease`, async (t) => {
  const h = host(t);
  const terminalPid = path.join(h.home, 'terminal.pid');
  const workerPid = path.join(h.home, 'worker.pid');
  const app = path.join(h.home, 'app.pl');
  const ptyApp = path.join(h.home, 'pty-app.py');
  fs.writeFileSync(ptyApp, `import os, pty, sys, time
with open(os.environ['HOME'] + '/app.pid', 'w') as f: f.write(str(os.getpid()))
pid, fd = pty.fork()
if pid == 0:
    # Exercise macOS's default interactive login shell without user profiles.
    shell = '/bin/zsh' if sys.platform == 'darwin' else '/bin/sh'
    args = ['zsh', '-d', '-f', '-i', '-l'] if sys.platform == 'darwin' else ['sh']
    os.execv(shell, args + ['-c', 'trap "" HUP TERM; echo $$ > "$1"; while :; do sleep 1; done', 'shell', sys.argv[1]])
while True: time.sleep(1)
`);
  const clean = path.join(h.home, 'clean-terminal.pl');
  fs.writeFileSync(clean, `$SIG{HUP} = $SIG{TERM} = 'IGNORE'; open(my $f, '>', $ARGV[0]); print {$f} $$; close $f; sleep 1 while 1;`);
  // Like forkpty: a child in a different session, ignoring the signals an
  // app's exit normally sends. Sleeping keeps the regression off Mike's CPU.
  fs.writeFileSync(app, `use POSIX qw(setsid);
    open(my $app, '>', "$ENV{HOME}/app.pid"); print {$app} $$; close $app;
    my $pid = fork() // die $!;
    if (!$pid) { setsid() >= 0 or die $!; $SIG{HUP} = $SIG{TERM} = 'IGNORE';
      if ($ARGV[1]) { POSIX::close(197); exec 'env', '-u', 'THREADBEAR_PERF_CHILDREN', 'perl', $ARGV[1], $ARGV[0]; die $!; }
      open(my $f, '>', $ARGV[0]) or die $!; print {$f} $$; close $f;
      sleep 1 while 1;
    } sleep 1 while 1;`);
  const entry = path.join(h.home, 'worker.mjs');
  fs.writeFileSync(entry, `import fs from 'node:fs'; import { GpuiDriver } from ${JSON.stringify(path.join(root, 'features/support/gpui.ts'))};
    fs.writeFileSync(${JSON.stringify(workerPid)}, String(process.pid));
    new GpuiDriver().child(${JSON.stringify(loss === 'app-pty' ? 'python3' : 'perl')}, ${JSON.stringify([loss === 'app-pty' ? ptyApp : app, terminalPid, ...(loss === 'closed-proof' ? [clean] : [])])}, {});
    setInterval(() => {}, 1000);`);
  const first = h.run([process.execPath, entry]);
  await until(() => fs.existsSync(terminalPid) && Number(fs.readFileSync(terminalPid, 'utf8')) > 1, 'terminal session did not start');
  const pid = Number(fs.readFileSync(terminalPid, 'utf8'));
  const born = spawnSync('ps', ['-p', String(pid), '-o', 'lstart='], { encoding: 'utf8' }).stdout.trim();
  t.after(() => {
    const now = spawnSync('ps', ['-p', String(pid), '-o', 'lstart='], { encoding: 'utf8' }).stdout.trim();
    if (born && now === born) { try { process.kill(pid, 'SIGKILL'); } catch {} }
  });
  if (loss.startsWith('app')) process.kill(Number(fs.readFileSync(path.join(h.home, 'app.pid'), 'utf8')), 'SIGKILL');
  if (loss === 'worker' || loss.startsWith('app') || loss === 'closed-proof') process.kill(Number(fs.readFileSync(workerPid, 'utf8')), 'SIGKILL');
  else if (loss === 'owner') first.kill('SIGKILL');
  else { first.stdout!.destroy(); first.stderr!.destroy(); first.stdin!.end(); }
  const probe = path.join(h.home, 'probe.mjs');
  fs.writeFileSync(probe, `import { spawnSync } from 'node:child_process';
    const state = spawnSync('ps', ['-p', '${pid}', '-o', 'stat='], { encoding: 'utf8' }).stdout.trim();
    if (state && !state.startsWith('Z')) { console.error('terminal survived unlock: ' + state); process.exit(1); }`);
  const next = h.run([process.execPath, probe]);
  assert.equal(await next.done, 0, next.output);
});

test('a fork after discovery is found even when its parent exits before the rescan', async (t) => {
  const h = host(t);
  const env = pausedSnapshot(h.home);
  const script = path.join(h.home, 'fork.pl');
  const pidfile = path.join(h.home, 'late.pid');
  fs.writeFileSync(script, `use POSIX qw(setsid); open(my $ready, '>', "$ENV{HOME}/app-ready"); close $ready;
    select(undef, undef, undef, 0.01) until -e "$ENV{HOME}/fork-now";
    if (!fork()) { setsid(); $SIG{TERM} = $SIG{HUP} = 'IGNORE';
      open(my $f, '>', $ARGV[0]); print {$f} $$; close $f;
      sleep 1 while 1;
    } exit 0;`);
  const workerPid = path.join(h.home, 'worker.pid');
  const entry = path.join(h.home, 'worker.mjs');
  fs.writeFileSync(entry, `import fs from 'node:fs'; import { GpuiDriver } from ${JSON.stringify(path.join(root, 'features/support/gpui.ts'))};
    fs.writeFileSync(${JSON.stringify(workerPid)}, String(process.pid));
    new GpuiDriver().child('perl', [${JSON.stringify(script)}, ${JSON.stringify(pidfile)}], {});
    setInterval(() => {}, 1000);`);
  const first = h.run([process.execPath, entry], env);
  await until(() => fs.existsSync(path.join(h.home, 'app-ready')), 'app did not start');
  process.kill(Number(fs.readFileSync(workerPid, 'utf8')), 'SIGKILL');
  await until(() => fs.existsSync(path.join(h.home, 'scan-ready')), 'scan did not pause');
  fs.writeFileSync(path.join(h.home, 'fork-now'), '');
  await until(() => fs.existsSync(pidfile) && Number(fs.readFileSync(pidfile, 'utf8')) > 1, 'late fork did not start');
  const pid = Number(fs.readFileSync(pidfile, 'utf8'));
  const born = spawnSync('ps', ['-p', String(pid), '-o', 'lstart='], { encoding: 'utf8' }).stdout.trim();
  t.after(() => {
    const now = spawnSync('ps', ['-p', String(pid), '-o', 'lstart='], { encoding: 'utf8' }).stdout.trim();
    if (born && now === born) { try { process.kill(pid, 'SIGKILL'); } catch {} }
  });
  fs.writeFileSync(path.join(h.home, 'resume-scan'), '');
  assert.notEqual(await first.done, 0);
  const state = spawnSync('ps', ['-p', String(pid), '-o', 'stat='], { encoding: 'utf8' }).stdout.trim();
  assert.ok(!state || state.startsWith('Z'), `late fork survived: ${state}`);
});

test('stale registration and snapshot identities never stop an unrelated process', async (t) => {
  const h = host(t);
  const env = pausedSnapshot(h.home);
  const registryFile = path.join(h.home, 'registry');
  const first = h.run(['sh', '-c', 'echo "$THREADBEAR_PERF_CHILDREN" > "$1"; echo ready; while :; do sleep 1; done', 'sh', registryFile], env);
  await until(() => first.output.includes('ready'), 'worker did not start');
  const registry = fs.readFileSync(registryFile, 'utf8').trim();
  // A matching argument is not an inherited environment entry.
  const unrelated = spawn('perl', ['-e', 'my $p = fork() // die $!; if ($p) { $|=1; print "$p\\n"; } sleep 1 while 1;', `THREADBEAR_PERF_CHILDREN=${registry}`], { detached: true, stdio: ['ignore', 'pipe', 'ignore'] });
  let childPid = '';
  unrelated.stdout!.on('data', (s) => { childPid += s; });
  const unrelatedDone = new Promise((r) => unrelated.on('exit', r));
  t.after(async () => { process.kill(-unrelated.pid!, 'SIGKILL'); await unrelatedDone; });
  await until(() => childPid.includes('\n'), 'unrelated subtree did not start');
  fs.writeFileSync(path.join(registry, String(unrelated.pid)), 'stale registration');
  const childBorn = spawnSync('ps', ['-p', childPid.trim(), '-o', 'lstart='], { encoding: 'utf8', env: { ...process.env, LC_ALL: 'C', TZ: 'UTC' } }).stdout.trim();
  fs.writeFileSync(path.join(registry, `anchor-${unrelated.pid}`), `${childPid.trim()}\n${childBorn}\nstale registration`);
  first.stdin!.end();
  await until(() => fs.existsSync(path.join(h.home, 'scan-ready')), 'scan did not pause');
  // Pretend a recorded descendant PID was reused after discovery. Its live
  // start check must disagree, and its alleged group must never be signalled.
  const snapshotFile = path.join(h.home, 'snapshot');
  const rows = fs.readFileSync(snapshotFile, 'utf8');
  const worker = fs.readdirSync(`${registry}.reaping`).find((n) => /^\d+$/.test(n));
  assert.ok(worker);
  fs.writeFileSync(snapshotFile, rows.split('\n').map((row) => {
    if (!new RegExp(`^\\s*${unrelated.pid}\\s`).test(row)) return row;
    return `${unrelated.pid} ${worker} ${unrelated.pid} ${process.getuid!()} S stale-start-time`;
  }).join('\n'));
  fs.writeFileSync(path.join(h.home, 'resume-scan'), '');
  await first.done;
  for (const pid of [String(unrelated.pid), childPid.trim()]) {
    const state = spawnSync('ps', ['-p', pid, '-o', 'stat='], { encoding: 'utf8' }).stdout.trim();
    assert.ok(state && !/^[TZ]/.test(state), `unrelated process ${pid} was stopped: ${state}`);
  }
});

test('an unavailable process snapshot keeps both leases held until discovery recovers', async (t) => {
  const h = host(t);
  const env = pausedSnapshot(h.home);
  fs.unlinkSync(path.join(h.home, 'pause-scan'));
  const first = h.run(['sh', '-c', 'echo ready; while :; do sleep 1; done'], env);
  await until(() => first.output.includes('ready'), 'worker did not start');
  fs.writeFileSync(path.join(h.home, 'fail-scan'), '');
  first.stdin!.end();
  await until(() => fs.existsSync(path.join(h.home, 'scan-failed')), 'failed snapshot not attempted');
  const next = h.run(['sh', '-c', 'echo entered-next-lease']);
  await until(() => next.output.includes('Waiting'), 'failed snapshot released locks');
  assert.doesNotMatch(next.output, /entered-next-lease/);
  fs.unlinkSync(path.join(h.home, 'fail-scan'));
  assert.equal(await next.done, 0, next.output);
});

test('registered background helpers keep their graceful TERM cleanup', async (t) => {
  const h = host(t);
  const ready = path.join(h.home, 'helper-ready');
  const stopped = path.join(h.home, 'helper-stopped');
  const script = path.join(h.home, 'graceful.pl');
  fs.writeFileSync(script, `my $p = fork() // die $!;
    if (!$p) { $SIG{TERM} = sub { open(my $f, '>', $ARGV[1]); close $f; exit 0; };
      open(my $f, '>', $ARGV[0]); close $f; sleep 1 while 1;
    }
    select(undef, undef, undef, .01) until -e $ARGV[0]; exit 0;`);
  const run = h.run(['perl', script, ready, stopped]);
  assert.equal(await run.done, 0, run.output);
  assert.ok(fs.existsSync(stopped), 'registered helper was killed without TERM');
});

test('worker death reaps children that Node launched without the proof descriptor', async (t) => {
  const h = host(t);
  const entry = path.join(h.home, 'worker.mjs');
  const pidfile = path.join(h.home, 'unmarked.pid');
  const workerPid = path.join(h.home, 'worker.pid');
  fs.writeFileSync(entry, `import fs from 'node:fs'; import { spawn } from 'node:child_process';
    fs.writeFileSync(${JSON.stringify(workerPid)}, String(process.pid));
    spawn('perl', ['-MPOSIX', '-e', 'POSIX::close(197); $SIG{HUP}=$SIG{TERM}="IGNORE"; open(my $f, ">", $ARGV[0]); print {$f} $$; close $f; sleep 1 while 1;', ${JSON.stringify(pidfile)}], { stdio: 'ignore' });
    setInterval(() => {}, 1000);`);
  const first = h.run([process.execPath, entry]);
  await until(() => fs.existsSync(pidfile) && Number(fs.readFileSync(pidfile, 'utf8')) > 1, 'background child did not start');
  const pid = Number(fs.readFileSync(pidfile, 'utf8'));
  const born = spawnSync('ps', ['-p', String(pid), '-o', 'lstart='], { encoding: 'utf8' }).stdout.trim();
  t.after(() => {
    const now = spawnSync('ps', ['-p', String(pid), '-o', 'lstart='], { encoding: 'utf8' }).stdout.trim();
    if (born && now === born) { try { process.kill(pid, 'SIGKILL'); } catch {} }
  });
  process.kill(Number(fs.readFileSync(workerPid, 'utf8')), 'SIGKILL');
  const next = h.run(['sh', '-c', 'echo next']);
  assert.equal(await next.done, 0, next.output);
  const state = spawnSync('ps', ['-p', String(pid), '-o', 'stat='], { encoding: 'utf8' }).stdout.trim();
  assert.ok(!state || state.startsWith('Z'), `unmarked group member survived unlock: ${state}`);
});

test('an anchor with a different freshly queried group cannot authorize group KILL', async (t) => {
  const h = host(t);
  const bin = path.join(h.home, 'bin');
  fs.mkdirSync(bin);
  fs.writeFileSync(path.join(bin, 'ps'), `#!/usr/bin/perl
    if ($ARGV[0] eq '-p' && $ARGV[3] eq 'lstart=' && !-e "$ENV{HOME}/resume-scan" && -e "$ENV{HOME}/child.pid") {
      open(my $f, '<', "$ENV{HOME}/child.pid"); my $child = <$f>; close $f;
      if ($ARGV[1] == $child) { print 'wrong-child-birth'; exit 0; }
    }
    if ($ARGV[0] eq '-p' && $ARGV[3] eq 'pgid=,uid=,stat=,lstart=') {
      if (!-e "$ENV{HOME}/rejected-anchor") {
        open(my $ps, '-|', '/bin/ps', @ARGV); my $row = <$ps>; close $ps or die;
        open(my $f, '>', "$ENV{HOME}/rejected-anchor"); print {$f} $ARGV[1]; close $f;
        $row =~ s/^\\s*\\d+/0/; print $row; exit 0;
      }
      open(my $f, '>', "$ENV{HOME}/query-paused"); close $f;
      select(undef, undef, undef, .02) until -e "$ENV{HOME}/resume-scan";
    }
    exec '/bin/ps', @ARGV;
  `, { mode: 0o755 });
  const pidfile = path.join(h.home, 'child.pid');
  const command = `use POSIX; my $p = fork() // die $!; if (!$p) {
    POSIX::close(197); $SIG{TERM} = $SIG{HUP} = 'IGNORE';
    open(my $f, '>', $ARGV[0]); print {$f} $$; close $f; sleep 1 while 1;
  } select(undef, undef, undef, .01) until -s $ARGV[0]; exit 0;`;
  const first = h.run(['perl', '-e', command, pidfile], { PATH: `${bin}:${process.env.PATH}` });
  await until(() => fs.existsSync(path.join(h.home, 'query-paused')), 'group query did not reject and retry');
  const anchor = Number(fs.readFileSync(path.join(h.home, 'rejected-anchor'), 'utf8'));
  assert.ok(anchor > 1);
  const state = spawnSync('/bin/ps', ['-p', String(anchor), '-o', 'stat='], { encoding: 'utf8' }).stdout.trim();
  assert.ok(state && !state.startsWith('Z'), 'group KILL ran despite the mismatching fresh PGID');
  const child = fs.readFileSync(pidfile, 'utf8').trim();
  const childState = spawnSync('/bin/ps', ['-p', child, '-o', 'stat='], { encoding: 'utf8' }).stdout.trim();
  assert.ok(childState && !childState.startsWith('Z'), 'the pinned child must keep its anchor from retiring');
  const next = h.run(['true']);
  await until(() => next.output.includes('Waiting'), 'rejected group query released locks');
  fs.writeFileSync(path.join(h.home, 'resume-scan'), '');
  assert.equal(await first.done, 0, first.output);
  assert.equal(await next.done, 0, next.output);
});

test('loss of an anchor cannot unlock a live group whose ownership is now uncertain', async (t) => {
  const h = host(t);
  const entry = path.join(h.home, 'worker.mjs');
  const childFile = path.join(h.home, 'child.pid');
  const workerFile = path.join(h.home, 'worker.pid');
  const registryFile = path.join(h.home, 'registry');
  fs.writeFileSync(entry, `import fs from 'node:fs'; import { spawn } from 'node:child_process';
    fs.writeFileSync(${JSON.stringify(workerFile)}, String(process.pid));
    fs.writeFileSync(${JSON.stringify(registryFile)}, process.env.THREADBEAR_PERF_CHILDREN);
    spawn('perl', ['-MPOSIX', '-e', 'POSIX::close(197); $SIG{HUP}=$SIG{TERM}="IGNORE"; open(my $f, ">", $ARGV[0]); print {$f} $$; close $f; sleep 1 while 1;', ${JSON.stringify(childFile)}], { stdio: 'ignore' });
    setInterval(() => {}, 1000);`);
  const first = h.run([process.execPath, entry]);
  await until(() => fs.existsSync(childFile) && Number(fs.readFileSync(childFile, 'utf8')) > 1, 'child did not start');
  const child = Number(fs.readFileSync(childFile, 'utf8'));
  const born = spawnSync('ps', ['-p', String(child), '-o', 'lstart='], { encoding: 'utf8' }).stdout.trim();
  const stopChild = () => {
    const now = spawnSync('ps', ['-p', String(child), '-o', 'lstart='], { encoding: 'utf8' }).stdout.trim();
    if (born && now === born) { try { process.kill(child, 'SIGKILL'); } catch {} }
  };
  t.after(stopChild);
  const worker = Number(fs.readFileSync(workerFile, 'utf8'));
  const registry = fs.readFileSync(registryFile, 'utf8');
  const [anchor, anchorBorn] = fs.readFileSync(path.join(registry, `anchor-${worker}`), 'utf8').split('\n');
  assert.equal(spawnSync('ps', ['-p', anchor, '-o', 'lstart='], { encoding: 'utf8', env: { ...process.env, LC_ALL: 'C', TZ: 'UTC' } }).stdout.trim(), anchorBorn);
  process.kill(Number(anchor), 'SIGKILL');
  process.kill(worker, 'SIGKILL');
  const next = h.run(['sh', '-c', 'echo entered-next-lease']);
  try {
    await until(() => fs.existsSync(`${registry}.reaping`), 'cleanup did not retire registry');
    await pause(1500);
    assert.match(next.output, /Waiting/);
    assert.doesNotMatch(next.output, /entered-next-lease/);
    const state = spawnSync('ps', ['-p', String(child), '-o', 'stat='], { encoding: 'utf8' }).stdout.trim();
    assert.ok(state && !state.startsWith('Z'), 'cleanup guessed ownership after losing its anchor');
  } finally { stopChild(); }
  assert.equal(await next.done, 0, next.output);
  await first.done;
});

test('stopped child groups retire their anchors during a continuing lease', async (t) => {
  const h = host(t);
  const entry = path.join(h.home, 'worker.mjs');
  const groupsFile = path.join(h.home, 'groups.json');
  fs.writeFileSync(entry, `import fs from 'node:fs'; import { GpuiDriver } from ${JSON.stringify(path.join(root, 'features/support/gpui.ts'))};
    const driver = new GpuiDriver(); const groups = [];
    for (let i = 0; i < 20; i++) {
      const file = process.env.HOME + '/' + i + '.pid';
      const c = driver.child('perl', ['-e', 'open(my $f, ">", $ARGV[0]); print {$f} $$; close $f; sleep 30;', file], {});
      while (!fs.existsSync(file) || !fs.readFileSync(file, 'utf8')) await new Promise(r => setTimeout(r, 10));
      const done = new Promise(r => c.once('exit', r)); await driver.killChild(c); await done;
      groups.push(c.pid);
    }
    fs.writeFileSync(${JSON.stringify(groupsFile)}, JSON.stringify({ registry: process.env.THREADBEAR_PERF_CHILDREN, groups }));
    console.log('stopped twenty'); setInterval(() => {}, 1000);`);
  const first = h.run([process.execPath, entry]);
  await until(() => first.output.includes('stopped twenty'), 'sequential children did not stop');
  const { registry, groups } = JSON.parse(fs.readFileSync(groupsFile, 'utf8')) as { registry: string; groups: number[] };
  const live = () => {
    const rows = spawnSync('ps', ['-axo', 'pid=,stat=,lstart='], { encoding: 'utf8', env: { ...process.env, LC_ALL: 'C', TZ: 'UTC' } }).stdout;
    return groups.filter((group) => {
      let record: string;
      try { record = fs.readFileSync(path.join(registry, `anchor-${group}`), 'utf8'); }
      catch (error) {
        if ((error as NodeJS.ErrnoException).code !== 'ENOENT') throw error;
        record = fs.readFileSync(path.join(registry, `retired-${group}`), 'utf8');
      }
      const [pid, born] = record.split('\n');
      return rows.split('\n').some((row) => {
        const m = row.match(/^\s*(\d+)\s+(\S+)\s+(.+?)\s*$/);
        return m && m[1] === pid && !m[2].startsWith('Z') && m[3] === born;
      });
    });
  };
  await until(() => live().length === 0, 'stopped children left resident anchors until lease cleanup');
});

test('repeated same-group registration keeps one holder and clears stale retirement', async (t) => {
  const h = host(t);
  const entry = path.join(h.home, 'reenter.pl');
  const gate = path.join(root, 'scripts/perf-children.pl');
  fs.writeFileSync(entry, `my $left = shift; my $dir = $ENV{THREADBEAR_PERF_CHILDREN};
    if ($left) {
      open(my $in, '<', "$dir/anchor-$$") or die $!;
      open(my $out, '>', "$dir/retired-$$") or die $!; print {$out} $_ while <$in>; close $out; close $in;
      exec 'perl', $ARGV[0], '--child', $dir, 'perl', $0, $left-1, $ARGV[0]; die $!;
    }
    $|=1; print "ready-reentered $$\\n"; sleep 1 while 1;`);
  const first = h.run(['perl', entry, '5', gate]);
  await until(() => first.output.includes('ready-reentered'), 'same-group reentry did not finish');
  const group = Number(first.output.match(/ready-reentered (\d+)/)![1]);
  const rows = spawnSync('ps', ['-axo', 'pid=,pgid=,stat='], { encoding: 'utf8' }).stdout;
  const holders = rows.split('\n').filter((row) => {
    const m = row.match(/^\s*(\d+)\s+(\d+)\s+(\S+)/);
    return m && Number(m[2]) === group && Number(m[1]) !== group && !m[3].startsWith('Z');
  });
  assert.equal(holders.length, 1, 're-entering the gate created duplicate resident holders');
  const base = path.join(h.home, '.cache/threadbear-check');
  const registry = fs.readdirSync(base).find((name) => name.startsWith('perf-children-'))!;
  assert.ok(!fs.existsSync(path.join(base, registry, `retired-${group}`)), 'new registration retained a stale retirement marker');
});

test('a retired group number cannot block cleanup when an unrelated leaderless group reuses it', async (t) => {
  const h = host(t);
  const bin = path.join(h.home, 'bin');
  fs.mkdirSync(bin);
  fs.writeFileSync(path.join(bin, 'ps'), `#!/usr/bin/perl
    if ($ARGV[0] eq '-axo' && $ARGV[1] eq 'pid=,ppid=,pgid=,uid=,stat=,lstart=' && -e "$ENV{HOME}/alias-target") {
      open(my $f, '<', "$ENV{HOME}/alias-target"); my ($pid,$group) = split / /, <$f>; close $f;
      open(my $ps, '-|', '/bin/ps', @ARGV) or die;
      while (my $row = <$ps>) { $row =~ s/^(\\s*$pid\\s+\\d+\\s+)\\d+/$1 . $group/e; print $row; }
      close $ps or die; exit 0;
    }
    exec '/bin/ps', @ARGV;
  `, { mode: 0o755 });
  const entry = path.join(h.home, 'worker.mjs');
  const data = path.join(h.home, 'group.json');
  fs.writeFileSync(entry, `import fs from 'node:fs'; import { GpuiDriver } from ${JSON.stringify(path.join(root, 'features/support/gpui.ts'))};
    const file = process.env.HOME + '/ready'; const d = new GpuiDriver();
    const c = d.child('perl', ['-e', 'open(my $f, ">", $ARGV[0]); print {$f} $$; close $f; sleep 30;', file], {});
    while (!fs.existsSync(file) || !fs.readFileSync(file,'utf8')) await new Promise(r => setTimeout(r,10));
    const done = new Promise(r => c.once('exit',r)); await d.killChild(c); await done;
    fs.writeFileSync(${JSON.stringify(data)}, JSON.stringify({ group:c.pid, worker:process.pid, registry:process.env.THREADBEAR_PERF_CHILDREN }));
    console.log('stopped one'); setInterval(() => {},1000);`);
  const first = h.run([process.execPath, entry], { PATH: `${bin}:${process.env.PATH}` });
  await until(() => first.output.includes('stopped one'), 'registered child did not stop');
  const { group, worker, registry } = JSON.parse(fs.readFileSync(data, 'utf8')) as { group: number; worker: number; registry: string };
  await until(() => fs.existsSync(path.join(registry, `retired-${group}`)), 'holder did not retire');
  assert.notEqual(spawnSync('/bin/ps', ['-p', String(group), '-o', 'pid=']).status, 0, 'original leader must be absent');
  const other = spawn('perl', ['-e', 'sleep 30'], { detached: true, stdio: 'ignore' });
  const otherDone = new Promise((r) => other.once('exit', r));
  let next: ReturnType<typeof h.run> | undefined;
  try {
    fs.writeFileSync(path.join(h.home, 'alias-target'), `${other.pid} ${group}`);
    process.kill(worker, 'SIGKILL');
    next = h.run(['sh', '-c', 'echo entered-next-lease']);
    await until(() => next!.output.includes('entered-next-lease'), 'retired registration treated the unrelated group as uncertain');
    assert.equal(other.exitCode, null, 'cleanup signalled an unrelated process');
    assert.equal(other.signalCode, null, 'cleanup signalled an unrelated process');
  } finally {
    if (other.exitCode === null && other.signalCode === null) other.kill('SIGKILL');
    await otherDone;
  }
  assert.equal(await next!.done, 0, next!.output);
});

for (const loss of ['owner', 'transport'] as const) test(`${loss} loss reaps a nested orphan that lost the descriptor`, async (t) => {
  const h = host(t);
  const orphanFile = path.join(h.home, 'orphan.pid');
  const ready = path.join(h.home, 'nested-ready');
  const nested = path.join(h.home, 'nested.mjs');
  fs.writeFileSync(nested, `import fs from 'node:fs'; import { spawn } from 'node:child_process';
    const c = spawn('perl', ['-MPOSIX', '-e', 'POSIX::close(197); $SIG{HUP}=$SIG{TERM}="IGNORE"; my $p=fork() // die $!; exit 0 if $p; open(my $f, ">", $ARGV[0]); print {$f} $$; close $f; sleep 1 while 1;', process.argv[2]], { stdio: 'ignore' });
    c.once('exit', () => fs.writeFileSync(process.argv[3], ''));
    setInterval(() => {}, 1000);`);
  const entry = path.join(h.home, 'worker.mjs');
  fs.writeFileSync(entry, `import { GpuiDriver } from ${JSON.stringify(path.join(root, 'features/support/gpui.ts'))};
    new GpuiDriver().child(process.execPath, ${JSON.stringify([nested, orphanFile, ready])}, {}); setInterval(() => {}, 1000);`);
  const first = h.run([process.execPath, entry]);
  await until(() => fs.existsSync(ready) && fs.existsSync(orphanFile) && Number(fs.readFileSync(orphanFile, 'utf8')) > 1, 'nested orphan did not start');
  const pid = Number(fs.readFileSync(orphanFile, 'utf8'));
  const born = spawnSync('ps', ['-p', String(pid), '-o', 'lstart='], { encoding: 'utf8' }).stdout.trim();
  const stopOrphan = () => {
    const now = spawnSync('ps', ['-p', String(pid), '-o', 'lstart='], { encoding: 'utf8' }).stdout.trim();
    if (born && now === born) { try { process.kill(pid, 'SIGKILL'); } catch {} }
  };
  if (loss === 'owner') first.kill('SIGKILL');
  else { first.stdout!.destroy(); first.stderr!.destroy(); first.stdin!.end(); }
  const next = h.run(['sh', '-c', 'echo entered-next-lease']);
  try {
    await until(() => next.output.includes('entered-next-lease'), 'cancelled lease did not finish nested group cleanup');
    const state = spawnSync('ps', ['-p', String(pid), '-o', 'stat='], { encoding: 'utf8' }).stdout.trim();
    assert.ok(!state || state.startsWith('Z'), `nested orphan survived ${loss} cancellation: ${state}`);
  } finally { stopOrphan(); }
  assert.equal(await next.done, 0, next.output);
});

for (const failure of ['stderr', 'timeout', 'closed-pipes', 'empty'] as const) test(`descriptor probe ${failure} still stops registered work and keeps both locks`, { skip: process.platform !== 'darwin' }, async (t) => {
  const h = host(t);
  const scripts = path.join(h.home, 'helpers');
  fs.mkdirSync(scripts);
  for (const name of ['perf-lock.pl', 'hangup-guard.sh']) fs.copyFileSync(path.join(root, 'scripts', name), path.join(scripts, name));
  const probe = path.join(scripts, 'probe');
  fs.writeFileSync(probe, `#!/usr/bin/perl
    if (-e "$ENV{HOME}/blocked-probe") {
      ${failure === 'stderr' ? 'print STDERR "unavailable\\n"; exit 2;' : failure === 'empty' ? 'exit 0;' : `${failure === 'closed-pipes' ? 'close STDOUT; close STDERR;' : ''} sleep 1 while 1;`}
    }
    exec '/usr/sbin/lsof', @ARGV;
  `, { mode: 0o755 });
  const source = fs.readFileSync(path.join(root, 'scripts/perf-children.pl'), 'utf8');
  const patched = source.replace("'/usr/sbin/lsof'", () => `'${probe}'`);
  assert.notEqual(source, patched);
  fs.writeFileSync(path.join(scripts, 'perf-children.pl'), patched);
  const stopped = path.join(h.home, 'terminated');
  const ready = path.join(h.home, 'ready');
  const command = `my $p = fork() // die $!; if (!$p) {
    $SIG{TERM} = sub { open(my $f, '>', $ARGV[1]); close $f; exit 0; };
    open(my $f, '>', $ARGV[0]); close $f; sleep 1 while 1;
  } select(undef, undef, undef, .01) until -e $ARGV[0]; exit 0;`;
  fs.writeFileSync(path.join(h.home, 'blocked-probe'), '');
  const first = h.run(['perl', '-e', command, ready, stopped], {}, undefined, false, path.join(scripts, 'perf-lock.pl'));
  await until(() => fs.existsSync(stopped), 'probe failure prevented registered TERM cleanup');
  const next = h.run(['sh', '-c', 'echo entered-next-lease']);
  await until(() => next.output.includes('Waiting'), 'probe failure released locks');
  await pause(3000);
  assert.doesNotMatch(next.output, /entered-next-lease/, 'probe failure declared quiet after registered work stopped');
  fs.unlinkSync(path.join(h.home, 'blocked-probe'));
  assert.equal(await first.done, 0, first.output);
  assert.equal(await next.done, 0, next.output);
});

test('queued remote copies and targets stay marked in use before the host locks are acquired', async (t) => {
  const h = host(t);
  const source = path.join(h.home, 'src-copy');
  const target = path.join(h.home, 'target-copy');
  for (const dir of [source, target]) { fs.mkdirSync(dir); fs.utimesSync(dir, 1, 1); }
  const first = h.run(['sh', '-c', 'echo ready; sleep 1']);
  await until(() => first.output.includes('ready'), 'first did not start');
  const next = h.run(['true'], { THREADBEAR_REMOTE_SOURCE: source, THREADBEAR_REMOTE_TARGET: target });
  await until(() => next.output.includes('Waiting'), 'next did not wait');
  for (const dir of [source, target]) {
    assert.ok(fs.existsSync(path.join(dir, '.in-use')));
    assert.ok(Date.now() - fs.statSync(dir).mtimeMs < 5000);
  }
  assert.equal(await next.done, 0, next.output);
});

test('a measurement joins the blocking check queue ahead of later checks', async (t) => {
  const h = host(t);
  const base = path.join(h.home, '.cache/threadbear-check');
  fs.mkdirSync(base, { recursive: true });
  const lock = path.join(base, 'lock');
  const order = path.join(h.home, 'order');
  const first = spawn('perl', ['-MFcntl=:flock', '-e', 'open(my $f, ">>", $ARGV[0]); flock($f, LOCK_EX); $|=1; print "ready\\n"; sleep 2', lock]);
  let output = '';
  first.stdout!.on('data', (s) => { output += s; });
  await until(() => output.includes('ready'), 'first check did not start');
  const measurement = h.run(['sh', '-c', 'echo measured >> "$1"; sleep .2', 'sh', order]);
  await until(() => measurement.output.includes('Waiting'), 'measurement did not queue');
  await pause(150);
  const later = spawn('perl', ['-MFcntl=:flock', '-e', 'open(my $f, ">>", $ARGV[0]); flock($f, LOCK_EX); open(my $o, ">>", $ARGV[1]); print {$o} "later\\n";', lock, order]);
  const done = new Promise((r) => later.on('exit', r));
  assert.equal(await measurement.done, 0, measurement.output);
  assert.equal(await done, 0);
  assert.equal(fs.readFileSync(order, 'utf8'), 'measured\nlater\n');
});

test('a successful shell command cannot leave background work holding the host after unlock', async (t) => {
  const h = host(t);
  const pidfile = path.join(h.home, 'background.pid');
  const run = h.run(['sh', '-c', 'sleep 60 & echo $! > "$1"', 'sh', pidfile]);
  assert.equal(await run.done, 0, run.output);
  const pid = Number(fs.readFileSync(pidfile, 'utf8'));
  assert.throws(() => process.kill(pid, 0));
});

test('a Desk client finishing early cannot release locks while its GUI workload lives', async (t) => {
  const h = host(t);
  const bin = path.join(h.home, '.local/bin');
  const appPid = path.join(h.home, 'app.pid');
  fs.mkdirSync(bin, { recursive: true });
  fs.writeFileSync(path.join(bin, 'desk'), '#!/bin/bash\nbash -c "$1" >"$HOME/desk.log" 2>&1 </dev/null &\nfor i in {1..100}; do [ ! -f "$HOME/app.pid" ] || exit 0; sleep .02; done\nexit 1\n', { mode: 0o755 });
  const first = h.run(['sh', '-c', 'trap "" TERM; echo $$ > "$1"; while :; do sleep 1; done', 'sh', appPid], {}, path.join(h.home, 'desk.pid'));
  await until(() => fs.existsSync(appPid), 'GUI workload did not start');
  const pid = Number(fs.readFileSync(appPid, 'utf8'));
  const next = h.run(['true']);
  await until(() => next.output.includes('Waiting'), 'early Desk client exit released the turn');
  assert.equal(await first.done, 0, first.output);
  assert.equal(await next.done, 0, next.output);
  assert.throws(() => process.kill(pid, 0));
});
