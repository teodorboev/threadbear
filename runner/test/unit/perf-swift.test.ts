// No app, build or screen: exercise the real SwiftDriver with a fake launcher
// that has the frozen launcher's double-fork and pre-PID-recording gaps.
import { spawn, spawnSync, type ChildProcess } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import assert from 'node:assert/strict';
import { test, type TestContext } from 'node:test';

const root = path.resolve(import.meta.dirname, '../../..');
const pause = (ms: number) => new Promise((r) => setTimeout(r, ms));
async function until(check: () => boolean, message: string) {
  for (let i = 0; i < 500; i++) { if (check()) return; await pause(40); }
  assert.fail(message);
}
function alive(pid: number) {
  const ps = spawnSync('ps', ['-p', String(pid), '-o', 'stat='], { encoding: 'utf8' });
  return ps.status === 0 && !ps.stdout.trim().startsWith('Z');
}
function fixture(t: TestContext, { perf = true, tmpFallback = false } = {}) {
  const home = fs.mkdtempSync(path.join(fs.realpathSync(os.tmpdir()), "tb-swift's $& test-"));
  const repo = path.join(home, 'repo');
  for (const dir of ['features/support', 'scripts/perf-swift-bin', 'app/Tools/mock-runner', 'bin']) fs.mkdirSync(path.join(repo, dir), { recursive: true });
  // Stand-in for LaunchServices. Never call the host's real open command.
  fs.writeFileSync(path.join(repo, 'bin/open'), `#!/usr/bin/perl
use POSIX;
my ($app, $out, $err);
while (@ARGV) {
  my $a=shift;
  if ($a eq '-a') { $app=shift; }
  elsif ($a eq '--stdout') { $out=shift; }
  elsif ($a eq '--stderr') { $err=shift; }
  elsif ($a eq '--env') { my ($k,$v)=split(/=/,shift,2); $ENV{$k}=$v; }
  elsif ($a eq '--args') { last; }
}
open(my $marker,'>',"$ENV{HOME}/ordinary-open"); close $marker;
defined(my $pid=fork) or die $!; exit 0 if $pid; setsid();
open(STDIN,'<','/dev/null'); open(STDOUT,'>',$out); open(STDERR,'>',$err);
exec "$app/Contents/MacOS/Threadbear", @ARGV;
`, { mode: 0o755 });
  for (const file of ['features/support/swift.ts', 'features/support/driver.ts', 'scripts/perf-children.pl', 'scripts/perf-swift.pl']) {
    fs.copyFileSync(path.join(root, file), path.join(repo, file));
  }
  // Also redirect the adapter's absolute fallback, so even a scope regression
  // can only reach our fake LaunchServices, never the host's real open.
  const adapter = path.join(repo, 'scripts/perf-swift.pl');
  const fakeOpen = path.join(repo, 'bin/open').replaceAll('\\', '\\\\').replaceAll("'", "\\'");
  const originalAdapter = fs.readFileSync(adapter, 'utf8');
  const patchedAdapter = originalAdapter.replace(/'\/usr\/bin\/open'/g, () => `'${fakeOpen}'`);
  assert.notEqual(patchedAdapter, originalAdapter, 'adapter fallback was not replaced');
  assert.doesNotMatch(patchedAdapter, /\/usr\/bin\/open/, 'adapter still reaches the real open');
  fs.writeFileSync(adapter, patchedAdapter);
  for (const name of ['open', 'node']) fs.symlinkSync(fs.readlinkSync(path.join(root, 'scripts/perf-swift-bin', name)), path.join(repo, 'scripts/perf-swift-bin', name));
  const entry = path.join(home, 'worker.mjs');
  const verify = path.join(home, 'verify.mjs');
  fs.writeFileSync(verify, `import fs from 'node:fs'; import {spawnSync} from 'node:child_process'; for (const name of ['app','mock','launcher']) { const file=process.env.HOME+'/all-'+name+'.pids'; if (!fs.existsSync(file)) continue; for (const pid of fs.readFileSync(file,'utf8').trim().split('\\n')) { const ps=spawnSync('ps',['-p',pid,'-o','stat=,command='],{encoding:'utf8'}); if(ps.status===0 && !ps.stdout.trim().startsWith('Z') && ps.stdout.includes(process.env.HOME)) { console.error(name+' still active when next lease acquired'); process.exit(23); } } }`);
  fs.writeFileSync(entry, `import fs from 'node:fs'; import { SwiftDriver } from ${JSON.stringify(path.join(repo, 'features/support/swift.ts'))}; fs.writeFileSync(process.env.HOME+'/worker.pid', String(process.pid)); const d = new SwiftDriver(); await d.start(); if (process.env.RELAUNCH) await d.relaunch(); if (process.env.POLL) { while(!fs.existsSync(process.env.HOME+'/mock.pid')) await new Promise(r=>setTimeout(r,20)); const count=()=>fs.readdirSync(process.env.THREADBEAR_PERF_CHILDREN).filter(n=>/^\\d+$/.test(n)).length; const before=count(); for(let i=0;i<20;i++) await d.dump(); fs.writeFileSync(process.env.HOME+'/counts',JSON.stringify([before,count()])); } fs.writeFileSync(process.env.HOME+'/ready', ''); if (process.env.NORMAL_STOP) { await d.stop(); } else { setInterval(()=>{}, 1000); }`);
  fs.writeFileSync(path.join(repo, 'bin/ps'), `#!/bin/bash
if [[ " $* " = *' lstart= '* ]] && [ -e "$HOME/slow-ps" ]; then
  while [ ! -e "$HOME/allow-reap" ]; do sleep .02; done
fi
exec /bin/ps "$@"
`, { mode: 0o755 });
  fs.writeFileSync(path.join(repo, 'app/Tools/mock-runner/server.mjs'), `import fs from 'node:fs'; fs.writeFileSync(process.env.HOME+'/mock.pid', String(process.pid)); fs.appendFileSync(process.env.HOME+'/all-mock.pids', process.pid+'\\n'); process.on('SIGTERM', ()=>{}); setInterval(()=>{},1000);`);
  fs.writeFileSync(path.join(repo, 'app/Tools/dev-app.sh'), `#!/bin/bash
set -eu
echo $$ >>"$HOME/all-launcher.pids"
id=''
for ((i=1;i<=$#;i++)); do if [ "\u0024{!i}" = --id ]; then j=$((i+1)); id="\u0024{!j}"; fi; done
tmp_root="\u0024{TMPDIR:-/tmp}"
mkdir -p "$tmp_root/threadbear-dev-app"
state_root="$(cd "$tmp_root/threadbear-dev-app" && pwd -P)"
dir="$state_root/$id"
printf '%s' "$dir" >"$HOME/instance-dir"
case " $* " in
  *' stop '*) kill "$(cat "$HOME/app.pid")" "$(cat "$HOME/mock.pid")" 2>/dev/null || true; exit 0 ;;
  *' dump '*) echo '{"connection":"connected","sidebar":{"sections":[]},"transcript":{"rows":[]}}'; exit 0 ;;
esac
mkdir -p "$dir/Threadbear.app/Contents/MacOS" "$dir/data"
cat >"$dir/Threadbear.app/Contents/MacOS/Threadbear" <<'APP'
#!/bin/bash
trap '' TERM
echo $$ >"$HOME/app.pid"
echo $$ >>"$HOME/all-app.pids"
printf '%s\\n' "$@" >"$HOME/app.args"
printf '%s' "$BROWSERBEAR_HOME" >"$HOME/app.env"
while :; do sleep 1; done
APP
chmod +x "$dir/Threadbear.app/Contents/MacOS/Threadbear"
touch "$HOME/before-mock"
while [ "\u0024{GAP:-}" = mock ] && [ ! -e "$HOME/proceed" ]; do sleep .02; done
/usr/bin/perl -MPOSIX -e 'my $pid=fork(); die $! unless defined $pid; exit 0 if $pid; POSIX::setsid(); exec @ARGV or die $!;' "$THREADBEAR_NODE" "$(dirname "$0")/mock-runner/server.mjs" --data-dir "$dir/data" </dev/null >"$dir/mock.log" 2>&1
touch "$HOME/before-app"
while [ "\u0024{GAP:-}" = app ] && [ ! -e "$HOME/proceed" ]; do sleep .02; done
open -g -n --env 'BROWSERBEAR_HOME=private browser home' -a "$dir/Threadbear.app" --stdout "$dir/app.stdout" --stderr "$dir/app.stderr" --args --data-dir "$dir/data" --automation-id "$id" --test-owner "$HOME" 'argument with spaces'
touch "$HOME/after-open"
# The real script records app.pid only after discovering the LS process.
while [ "\u0024{GAP:-}" = record ] && [ ! -e "$HOME/proceed" ]; do sleep .02; done
for i in {1..500}; do [ ! -e "$HOME/app.pid" ] || exit 0; sleep .02; done
exit 1
`, { mode: 0o755 });
  const children: (ChildProcess & { done: Promise<number | null>; output: string })[] = [];
  const env = { ...process.env, HOME: home, TMPDIR: home, PATH: `${repo}/bin:${process.env.PATH}`, THREADBEAR_NODE: process.execPath, THREADBEAR_PERF_LOCK_OWNER: '', THREADBEAR_PERF_CHILDREN: '' };
  function run(extra: NodeJS.ProcessEnv = {}, args = [process.execPath, entry]) {
    const child = spawn(perf ? 'perl' : args[0], perf ? [path.join(root, 'scripts/perf-lock.pl'), '-', ...args] : args.slice(1), { env: { ...env, ...extra }, stdio: ['pipe', 'pipe', 'pipe'] });
    const result = child as typeof children[number];
    result.output = '';
    child.stdout!.on('data', (s) => { result.output += s; });
    child.stderr!.on('data', (s) => { result.output += s; });
    result.done = new Promise((r) => child.on('exit', r));
    children.push(result);
    return result;
  }
  t.after(async () => {
    try {
      fs.writeFileSync(path.join(home, 'allow-reap'), '');
      fs.writeFileSync(path.join(home, 'proceed'), '');
      for (const child of children) child.stdin?.end();
      // Non-perf fixture has no supervisor; these are only our fake processes.
      for (const name of ['worker', 'app', 'mock']) {
        try { killOwned(Number(fs.readFileSync(path.join(home, `${name}.pid`), 'utf8'))); } catch {}
      }
      for (const name of ['app', 'mock', 'launcher']) {
        try { for (const pid of fs.readFileSync(path.join(home, `all-${name}.pids`), 'utf8').trim().split('\n')) killOwned(Number(pid)); } catch {}
      }
      for (const child of children) await child.done;
      for (const name of ['app', 'mock', 'launcher']) {
        try { for (const pid of fs.readFileSync(path.join(home, `all-${name}.pids`), 'utf8').trim().split('\n')) killOwned(Number(pid)); } catch {}
      }
      // A TMPDIR-fallback fixture uses /tmp, just as the frozen launcher does.
      if (tmpFallback && fs.existsSync(path.join(home, 'instance-dir'))) {
        const instance = fs.readFileSync(path.join(home, 'instance-dir'), 'utf8');
        const worker = fs.readFileSync(path.join(home, 'worker.pid'), 'utf8');
        assert.equal(path.basename(instance), `spec-${worker}-1`);
        fs.rmSync(instance, { recursive: true, force: true });
      }
    } finally { fs.rmSync(home, { recursive: true, force: true }); }
  });
  function killOwned(pid: number) {
    const ps = spawnSync('ps', ['-p', String(pid), '-o', 'command='], { encoding: 'utf8' });
    if (ps.status === 0 && ps.stdout.includes(home)) process.kill(pid, 'SIGKILL');
  }
  return { home, repo, run, verify: [process.execPath, verify], file: (name: string) => path.join(home, name), pid: (name: string) => Number(fs.readFileSync(path.join(home, `${name}.pid`), 'utf8')) };
}

for (const { gap, signal, owner } of [
  ...['mock', 'app', 'record', 'ready'].map((gap) => ({ gap, signal: 'SIGKILL' as const, owner: false })),
  { gap: 'ready', signal: 'SIGTERM' as const, owner: false },
  { gap: 'ready', signal: 'SIGKILL' as const, owner: true },
]) test(`Swift ${owner ? 'lock holder' : 'worker'} ${signal} at ${gap} cannot leave its private app or mock after unlock`, async (t) => {
  const h = fixture(t);
  const first = h.run({ GAP: gap });
  const marker = { mock: 'before-mock', app: 'before-app', record: 'after-open', ready: 'ready' }[gap as 'mock' | 'app' | 'record' | 'ready'];
  await until(() => fs.existsSync(h.file(marker)) || first.exitCode !== null, `never reached ${gap}: ${first.output}`);
  assert.ok(fs.existsSync(h.file(marker)), first.output);
  if (gap === 'record' || gap === 'ready') await until(() => fs.existsSync(h.file('app.pid')), 'app did not exec');
  process.kill(owner ? first.pid! : h.pid('worker'), signal);
  const next = h.run({}, h.verify);
  assert.equal(await first.done, owner ? null : signal === 'SIGKILL' ? 137 : 143, first.output);
  assert.equal(await next.done, 0, next.output);
  for (const name of ['app', 'mock']) if (fs.existsSync(h.file(`${name}.pid`))) assert.ok(!alive(h.pid(name)), `${name} survived unlock`);
});

test('Swift launch continuing during registry retirement cannot fall back to system open', async (t) => {
  const h = fixture(t);
  const first = h.run({ GAP: 'app' });
  await until(() => fs.existsSync(h.file('before-app')) && fs.existsSync(h.file('mock.pid')), 'did not reach pre-app gap');
  fs.writeFileSync(h.file('slow-ps'), '');
  process.kill(h.pid('worker'), 'SIGKILL');
  const base = h.file('.cache/threadbear-check');
  await until(() => fs.readdirSync(base).some((name) => name.endsWith('.reaping')), 'registry not retired');
  fs.writeFileSync(h.file('proceed'), '');
  await until(() => fs.existsSync(h.file('after-open')), 'launcher did not continue through retirement');
  assert.ok(!fs.existsSync(h.file('ordinary-open')), 'retired adapter fell through to system open');
  fs.writeFileSync(h.file('allow-reap'), '');
  const next = h.run({}, h.verify);
  assert.equal(await first.done, 137, first.output);
  assert.equal(await next.done, 0, next.output);
  assert.ok(!fs.existsSync(h.file('app.pid')), 'late app ran after retirement');
});

test('Swift wrapper preserves app argv/env, relaunch ownership and transport cleanup', async (t) => {
  const h = fixture(t);
  const unrelated = spawn(process.execPath, ['-e', 'setInterval(()=>{},1000)'], { stdio: 'ignore', detached: true });
  t.after(() => unrelated.kill('SIGKILL'));
  const first = h.run({ RELAUNCH: '1' });
  await until(() => fs.existsSync(h.file('ready')) || first.exitCode !== null, 'Swift did not start');
  assert.ok(fs.existsSync(h.file('ready')), first.output);
  await until(() => ['app', 'mock'].every((name) => fs.existsSync(h.file(`all-${name}.pids`)) && fs.readFileSync(h.file(`all-${name}.pids`), 'utf8').trim().split('\n').length === 2), 'relaunch did not start both generations');
  assert.ok(!fs.existsSync(h.file('ordinary-open')), 'perf app went through LaunchServices');
  assert.match(fs.readFileSync(h.file('app.args'), 'utf8'), /argument with spaces\n$/);
  assert.equal(fs.readFileSync(h.file('app.env'), 'utf8'), 'private browser home');
  first.stdin!.end();
  assert.notEqual(await first.done, 0);
  const next = h.run({}, h.verify);
  assert.equal(await next.done, 0, next.output);
  assert.ok(alive(unrelated.pid!), 'cleanup killed an unregistered instance');
  for (const name of ['app', 'mock']) for (const pid of fs.readFileSync(h.file(`all-${name}.pids`), 'utf8').trim().split('\n')) assert.ok(!alive(Number(pid)), `${name} survived transport loss`);
});

test('polling does not grow the registry, explicit Node is used, and successful stop cleans up', async (t) => {
  const h = fixture(t);
  const node = h.file('chosen-node');
  fs.writeFileSync(node, `#!/bin/sh\ntouch "$HOME/selected-node"\nexec '${process.execPath.replaceAll("'", "'\\''")}' "$@"\n`, { mode: 0o755 });
  const first = h.run({ NORMAL_STOP: '1', POLL: '1', THREADBEAR_NODE: node });
  assert.equal(await first.done, 0, first.output);
  const [before, after] = JSON.parse(fs.readFileSync(h.file('counts'), 'utf8'));
  assert.equal(after, before, 'dump calls added dead launcher groups');
  assert.ok(fs.existsSync(h.file('selected-node')), 'explicit Node was ignored');
  const next = h.run({}, h.verify);
  assert.equal(await next.done, 0, next.output);
});

test('without a perf registry Swift still calls the ordinary launcher', async (t) => {
  const h = fixture(t, { perf: false });
  const first = h.run({ NORMAL_STOP: '1', TMPDIR: h.file('new-temp-directory') });
  assert.equal(await first.done, 0, first.output);
  assert.ok(fs.existsSync(h.file('ordinary-open')));
  assert.doesNotMatch(first.output, /direct private-bundle launch/);
});

test('a symlinked Swift instance root still uses the owned launch adapters', async (t) => {
  const h = fixture(t);
  fs.mkdirSync(h.file('real-instances'));
  fs.symlinkSync(h.file('real-instances'), h.file('threadbear-dev-app'));
  const first = h.run({ NORMAL_STOP: '1' });
  assert.equal(await first.done, 0, first.output);
  assert.ok(!fs.existsSync(h.file('ordinary-open')), 'canonical bundle mismatch bypassed the app gate');
  const next = h.run({}, h.verify);
  assert.equal(await next.done, 0, next.output);
});

test('Swift TMPDIR fallback matches the launcher even when TMP points elsewhere', async (t) => {
  const h = fixture(t, { tmpFallback: true });
  const first = h.run({ NORMAL_STOP: '1', TMPDIR: '', TMP: h.home, TEMP: h.home });
  assert.equal(await first.done, 0, first.output);
  assert.ok(!fs.existsSync(h.file('ordinary-open')), 'TMP fallback bypassed the app gate');
  const next = h.run({}, h.verify);
  assert.equal(await next.done, 0, next.output);
});

test('late Swift startup and detached mock exec cannot cross registry retirement', async (t) => {
  const h = fixture(t);
  const registry = h.file('retired-registry');
  fs.mkdirSync(registry);
  fs.writeFileSync(path.join(registry, 'state'), '');
  fs.writeFileSync(path.join(registry, 'gone'), '');
  const helper = path.join(root, 'scripts/perf-swift.pl');
  const started = h.file('unexpected-exec');
  const script = h.file('late-script');
  fs.writeFileSync(script, `#!/bin/sh\ntouch '${started.replaceAll("'", "'\\''")}'\n`, { mode: 0o755 });
  const run = spawnSync('perl', [helper, '--run', registry, process.execPath, h.file('instance'), script]);
  assert.equal(run.status, 129, run.stderr?.toString());
  assert.ok(!fs.existsSync(started));
  const node = h.file('node');
  fs.symlinkSync(helper, node);
  const mock = h.file('late.mjs');
  fs.writeFileSync(mock, `import fs from 'node:fs'; fs.writeFileSync(${JSON.stringify(started)}, '');`);
  const late = spawn(node, [mock, '--data-dir', `${h.file('instance')}/data`], { detached: true, env: { ...process.env, THREADBEAR_PERF_CHILDREN: registry, THREADBEAR_PERF_SWIFT_NODE: process.execPath, THREADBEAR_PERF_SWIFT_INSTANCE: h.file('instance'), THREADBEAR_PERF_SWIFT_MOCK: mock } });
  assert.equal(await new Promise((r) => late.on('exit', r)), 129);
  assert.ok(!fs.existsSync(started));
});
