// The GPUI build folders on a shared host (desktop/scripts/remote.sh). They
// filled the cloud box on 9 Nov, and that box also runs a runner, whose saves
// then failed (THREADBEAR-1R). An age limit alone doesn't help when four
// checkouts are all in use, so low disk has to give room back as well.
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { test, type TestContext } from 'node:test';
import assert from 'node:assert/strict';

const SCRIPTS = path.join(import.meta.dirname, '..', '..', '..', 'desktop', 'scripts');
const TRIM = path.join(SCRIPTS, 'trim-remote-cache.sh');
const HOUR = 3_600_000;

// A df that answers from how many target folders are left, so deleting one
// really does free room. Each is worth 6 GiB.
function host(t: TestContext, freeGib: number, folders: Record<string, number>) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'tb-remote-cache-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const bin = path.join(root, 'bin');
  const base = path.join(root, 'home', '.cache', 'threadbear-check', 'gpui');
  fs.mkdirSync(bin);
  fs.mkdirSync(base, { recursive: true });
  const now = Date.now();
  for (const [name, hoursAgo] of Object.entries(folders)) {
    const p = path.join(base, name);
    fs.mkdirSync(p);
    const when = new Date(now - hoursAgo * HOUR);
    // A negative age: an empty folder, a checkout that has never built.
    if (hoursAgo >= 0 && name.startsWith('target')) {
      fs.mkdirSync(path.join(p, 'debug'));
      fs.writeFileSync(path.join(p, 'debug', 'out'), 'x');
      fs.utimesSync(path.join(p, 'debug', 'out'), when, when);
      fs.utimesSync(path.join(p, 'debug'), when, when);
    }
    fs.utimesSync(p, when, when);
  }
  const targets = Object.keys(folders).filter((n) => /^target(-|$)/.test(n)).length;
  fs.writeFileSync(path.join(bin, 'df'), `#!/bin/bash
n=$(ls -d target target-* 2>/dev/null | wc -l)
echo "Filesystem 1024-blocks Used Available Capacity Mounted"
f=$(( (${freeGib} + (${targets} - n) * 6) * 1048576 ))
[ "$f" -ge 0 ] || f=0
echo "fake 0 0 $f 0% /"
`, { mode: 0o755 });
  // A built target is many GiB and these are tiny, so du goes by whether one
  // has a build in it.
  fs.writeFileSync(path.join(bin, 'du'), '#!/bin/bash\nif [ -f "$2/debug/out" ]; then echo "6000000 $2"; else echo "100 $2"; fi\n', { mode: 0o755 });
  const env = { ...process.env, PATH: `${bin}:${process.env.PATH}`, HOME: path.join(root, 'home') };
  const left = () => fs.readdirSync(base).filter((n) => /^target(-|$)/.test(n)).sort();
  const trim = (own = 'aaa') => spawnSync('/bin/bash', [TRIM, own], { cwd: base, env, encoding: 'utf8', timeout: 20_000 });
  return { root, bin, base, env, left, trim };
}

// An ssh that runs the command here, stdin and all, past remote.sh's -o options.
const FAKE_SSH = `#!/bin/bash\nwhile [ "$1" = -o ]; do shift 2; done\nshift\ncd "$HOME" && eval "$*"\n`;

const WORK = {
  'src-aaa': 0, 'target-aaa': 0, // this checkout
  'src-bbb': 30, 'target-bbb': 30, // idle the longest
  'src-ccc': 10, 'target-ccc': 10, // idle
  'src-ddd': 0.1, 'target-ddd': 0.1, // building now
};

test('a disk with room is left alone', (t) => {
  const h = host(t, 40, WORK);
  assert.equal(h.trim().status, 0);
  assert.deepEqual(h.left(), ['target-aaa', 'target-bbb', 'target-ccc', 'target-ddd']);
});

test('low on disk, the checkout idle longest goes first, and only until there is room', (t) => {
  const h = host(t, 8, WORK);
  const r = h.trim();
  assert.equal(r.status, 0, r.stderr);
  assert.deepEqual(h.left(), ['target-aaa', 'target-ccc', 'target-ddd']);
  assert.match(r.stderr, /target-bbb/);
});

test("this checkout's folder and one in use are never taken, however low the disk", (t) => {
  const h = host(t, 4, WORK);
  assert.equal(h.trim().status, 0);
  assert.deepEqual(h.left(), ['target-aaa', 'target-ddd']);
});

test('the old shared folder goes before any checkout loses its own', (t) => {
  const h = host(t, 8, { ...WORK, target: 5 });
  assert.equal(h.trim().status, 0);
  assert.deepEqual(h.left(), ['target-aaa', 'target-bbb', 'target-ccc', 'target-ddd']);
});

test("what's inside a target says whether it's in use, not the folder's own time", (t) => {
  const h = host(t, 8, { 'target-aaa': 0, 'target-eee': 50 });
  // A long run: the folder itself hasn't changed since it started.
  fs.writeFileSync(path.join(h.base, 'target-eee', '.in-use'), '');
  fs.utimesSync(path.join(h.base, 'target-eee'), new Date(Date.now() - 50 * HOUR), new Date(Date.now() - 50 * HOUR));
  assert.equal(h.trim().status, 0);
  assert.deepEqual(h.left(), ['target-aaa', 'target-eee']);
});

test('a disk still nearly full refuses the run and says why', (t) => {
  const h = host(t, 2, { 'target-aaa': 0, 'target-ddd': 0.1 });
  const r = h.trim();
  assert.equal(r.status, 3);
  assert.match(r.stderr, /2 GiB free/);
  assert.deepEqual(h.left(), ['target-aaa', 'target-ddd']);
});

test('a first build wants more room than one with a target to build on', (t) => {
  const warm = host(t, 5, { 'target-aaa': 0, 'target-ddd': 0.1 });
  assert.equal(warm.trim().status, 0);
  const cold = host(t, 5, { 'target-aaa': -1, 'target-ddd': 0.1 });
  const r = cold.trim();
  assert.equal(r.status, 3);
  assert.match(r.stderr, /wants 8/);
  // A first build that died early left a folder, and still nothing to build on.
  const died = host(t, 5, { 'target-aaa': -1, 'target-ddd': 0.1 });
  fs.mkdirSync(path.join(died.base, 'target-aaa', 'debug'));
  fs.writeFileSync(path.join(died.base, 'target-aaa', 'CACHEDIR.TAG'), '');
  assert.equal(died.trim().status, 3);
});

test('nothing is left behind by a folder given back', (t) => {
  const h = host(t, 8, WORK);
  assert.equal(h.trim().status, 0);
  assert.deepEqual(fs.readdirSync(h.base).filter((n) => n.includes('gone')), []);
});

test('a df reporting less than nothing counts as full, and one it can not read blocks nothing', (t) => {
  const h = host(t, 5, { 'target-aaa': 0 });
  const df = (kb: string) => fs.writeFileSync(path.join(h.bin, 'df'), `#!/bin/bash\necho header\necho "fake 0 0 ${kb} 0% /"\n`, { mode: 0o755 });
  df('-100');
  assert.equal(h.trim().status, 3);
  df('n/a');
  assert.equal(h.trim().status, 0);
});

test('remote.sh stops before copying anything when the host refuses, and marks its target as used', (t) => {
  const h = host(t, 2, {});
  const log = path.join(h.root, 'rsync-ran');
  fs.writeFileSync(path.join(h.bin, 'ssh'), FAKE_SSH, { mode: 0o755 });
  fs.writeFileSync(path.join(h.bin, 'rsync'), `#!/bin/bash\ntouch ${JSON.stringify(log)}\n`, { mode: 0o755 });
  const r = spawnSync('/bin/bash', [path.join(SCRIPTS, 'remote.sh'), 'somehost', '--', 'true'], { env: h.env, encoding: 'utf8', timeout: 20_000 });
  assert.equal(r.status, 3, r.stderr);
  assert.equal(fs.existsSync(log), false, 'nothing copied to a full disk');
  assert.equal(fs.readdirSync(h.base).filter((n) => n.startsWith('target-')).length, 1, 'its target folder exists, so the age limit sees it used');
  const own = fs.readdirSync(h.base).find((n) => n.startsWith('target-'))!;
  assert.ok(Date.now() - fs.statSync(path.join(h.base, own)).mtimeMs < HOUR);
});

test('a run marks its target in use and its copy as used, and ssh does not wait on the marking', (t) => {
  const h = host(t, 40, {});
  fs.writeFileSync(path.join(h.bin, 'ssh'), FAKE_SSH, { mode: 0o755 });
  fs.writeFileSync(path.join(h.bin, 'npm'), '#!/bin/bash\nexit 0\n', { mode: 0o755 });
  // The real rsync sets the copy's time to the Mac's, which is the point here.
  const findNode = path.join(import.meta.dirname, '..', '..', 'scripts', 'find-node.sh');
  fs.writeFileSync(path.join(h.bin, 'rsync'), `#!/bin/bash
dest="$HOME/\${!##*:}"
mkdir -p "$dest/runner/scripts" "$dest/scripts"
cp ${JSON.stringify(findNode)} "$dest/runner/scripts/"
source="\${@: -2:1}"
for helper in perf-lock.pl perf-children.pl hangup-guard.sh remote-guard-lib.sh build-priority.sh perf-build.sh; do
  cp "$source/scripts/$helper" "$dest/scripts/"
done
touch -t 202601010000 "$dest"
# The host was too full for the first mkdir, and the trim then made room.
rm -rf "$HOME"/.cache/threadbear-check/gpui/target-*
`, { mode: 0o755 });
  const r = spawnSync('/bin/bash', [path.join(SCRIPTS, 'remote.sh'), 'somehost', '--', 'true'], { env: h.env, encoding: 'utf8', timeout: 30_000 });
  assert.equal(r.error, undefined, 'it returned rather than waiting for the marker loop');
  assert.equal(r.status, 0, r.stderr);
  const names = fs.readdirSync(h.base);
  const target = names.find((n) => n.startsWith('target-'))!;
  const src = names.find((n) => n.startsWith('src-'))!;
  assert.ok(fs.existsSync(path.join(h.base, target, '.in-use')));
  assert.ok(Date.now() - fs.statSync(path.join(h.base, src)).mtimeMs < HOUR, 'the copy looks used, whatever rsync set');
});

// 12 Nov: seven targets of 5 to 7 GiB on Tinker Desk, grown between runs
// until it had 1.8 GiB free and no check could run there.
const MANY = {
  'src-aaa': 0, 'target-aaa': 0, // this checkout
  'src-bbb': 1, 'target-bbb': 1,
  'src-ccc': 2, 'target-ccc': 2,
  'src-ddd': 3, 'target-ddd': 3,
  'src-eee': 4, 'target-eee': 4,
  'src-fff': 5, 'target-fff': 5,
  'src-ggg': 0.1, 'target-ggg': 0.1, // building now
};

test('past the cap, the idle ones longest unused go even with room to spare, copies and all', (t) => {
  const h = host(t, 400, MANY);
  const r = h.trim();
  assert.equal(r.status, 0, r.stderr);
  assert.deepEqual(h.left(), ['target-aaa', 'target-bbb', 'target-ccc', 'target-ggg']);
  assert.match(r.stderr, /More than 4 build folders/);
  const srcs = fs.readdirSync(h.base).filter((n) => n.startsWith('src-')).sort();
  assert.deepEqual(srcs, ['src-aaa', 'src-bbb', 'src-ccc', 'src-ggg']);
});

test('the cap never takes one in use, so a busy host can go past it', (t) => {
  const h = host(t, 400, MANY);
  // Long runs: the marker's new, the folders' own times as old as they were.
  for (const [k, hours] of [['ddd', 3], ['eee', 4], ['fff', 5]] as const) {
    const p = path.join(h.base, `target-${k}`);
    fs.writeFileSync(path.join(p, '.in-use'), '');
    fs.utimesSync(p, new Date(Date.now() - hours * HOUR), new Date(Date.now() - hours * HOUR));
  }
  assert.equal(h.trim().status, 0);
  assert.equal(h.left().length, 7);
});

test('with no checkout of its own (check-remote.sh) it gives room back and never refuses', (t) => {
  const h = host(t, 1, WORK);
  const r = h.trim('-');
  assert.equal(r.status, 0, r.stderr);
  // No folder is its own, so aaa (idle a moment) is fair game too, but only
  // until there's room: bbb and ccc are enough.
  assert.deepEqual(h.left(), ['target-aaa', 'target-ddd']);
  const full = host(t, 0, { 'target-ggg': 0.1 });
  assert.equal(full.trim('-').status, 0);
});

test('a copy whose target has gone goes too, unless it was touched lately', (t) => {
  const h = host(t, 400, { 'src-aaa': 0, 'target-aaa': 0, 'src-old': 5, 'src-new': 0.1 });
  assert.equal(h.trim().status, 0);
  assert.deepEqual(fs.readdirSync(h.base).filter((n) => n.startsWith('src-')).sort(), ['src-aaa', 'src-new']);
});

test("a target whose copy is being written to stays, though nothing in the target changed (a long rsync)", (t) => {
  const h = host(t, 1, { 'target-aaa': 0, 'src-bbb': 30, 'target-bbb': 30 });
  fs.writeFileSync(path.join(h.base, 'src-bbb', 'just-copied'), '');
  fs.utimesSync(path.join(h.base, 'src-bbb'), new Date(Date.now() - 30 * HOUR), new Date(Date.now() - 30 * HOUR));
  h.trim();
  assert.deepEqual(h.left(), ['target-aaa', 'target-bbb']);
  assert.ok(fs.existsSync(path.join(h.base, 'src-bbb', 'just-copied')));
});

test("old temp files go, but a SwiftPM lock file stays while the copy it's named for is here", (t) => {
  const h = host(t, 400, { 'src-aaa': 0, 'target-aaa': 0 });
  fs.mkdirSync(path.join(h.base, 'tmp'));
  // Only holding a lock file open shows it's in use, not its time.
  const live = '_Users_x_.cache_threadbear-check_gpui_src-aaa_app_.build.lock';
  const dead = '_Users_x_.cache_threadbear-check_gpui_src-zzz_app_.build.lock';
  for (const [name, hoursAgo] of [['old.txt', 72], ['new.txt', 1], [live, 72], [dead, 72]] as const) {
    const p = path.join(h.base, 'tmp', name);
    fs.writeFileSync(p, '');
    const when = new Date(Date.now() - hoursAgo * HOUR);
    fs.utimesSync(p, when, when);
  }
  assert.equal(h.trim().status, 0);
  assert.deepEqual(fs.readdirSync(path.join(h.base, 'tmp')).sort(), [live, 'new.txt']);
});

test("a trim waits for another one on the same host, so a run can't start between its look and its move", (t) => {
  const h = host(t, 40, {});
  // Another trim holding the lock for 3 s. Only the trim's own time counts:
  // the pipe waits for the holder either way.
  const holder = path.join(h.root, 'hold.pl');
  fs.writeFileSync(holder, 'use Fcntl ":flock"; open(my $f, "<", ".") or die; flock($f, LOCK_EX) or die; $| = 1; print "held\\n"; sleep 3;');
  const r = spawnSync('/bin/bash', ['-c', `perl ${JSON.stringify(holder)} | { read -r _; SECONDS=0; bash ${JSON.stringify(TRIM)} aaa; echo "took $SECONDS"; }`], { cwd: h.base, env: h.env, encoding: 'utf8', timeout: 20_000 });
  assert.equal(r.status, 0, r.stderr);
  assert.ok(Number(/took (\d+)/.exec(r.stdout)![1]) >= 2, r.stdout);
});

// 13 Nov: the cloud box filled during a spec run, every scenario failed with
// ENOSPC, and the run still ended 0.
test('a run that leaves the host nearly full fails, whatever its command said, and says why', (t) => {
  const h = host(t, 40, {});
  const full = path.join(h.root, 'full');
  fs.writeFileSync(path.join(h.bin, 'ssh'), FAKE_SSH, { mode: 0o755 });
  fs.writeFileSync(path.join(h.bin, 'npm'), '#!/bin/bash\nexit 0\n', { mode: 0o755 });
  // Room to start, then 300 MiB once the command has run.
  fs.writeFileSync(path.join(h.bin, 'df'), `#!/bin/bash
echo "Filesystem 1024-blocks Used Available Capacity Mounted"
if [ -e ${JSON.stringify(full)} ]; then echo "fake 0 0 307200 0% /"; else echo "fake 0 0 $((40 * 1048576)) 0% /"; fi
`, { mode: 0o755 });
  const findNode = path.join(import.meta.dirname, '..', '..', 'scripts', 'find-node.sh');
  fs.writeFileSync(path.join(h.bin, 'rsync'), `#!/bin/bash
dest="$HOME/\${!##*:}"
mkdir -p "$dest/runner/scripts" "$dest/scripts"
cp ${JSON.stringify(findNode)} "$dest/runner/scripts/"
source="\${@: -2:1}"
for helper in perf-lock.pl perf-children.pl hangup-guard.sh remote-guard-lib.sh build-priority.sh perf-build.sh; do
  cp "$source/scripts/$helper" "$dest/scripts/"
done
`, { mode: 0o755 });
  // A host each: the fake ssh runs here, so a run's in-use marker keeps the
  // last run's turn lock open, which the real ssh would have dropped.
  const run = (where: string, ...cmd: string[]) => spawnSync('/bin/bash', [path.join(SCRIPTS, 'remote.sh'), where, '--', ...cmd], { env: h.env, encoding: 'utf8', timeout: 30_000 });

  // Remote builds keep line numbers, not the full debug info that filled it.
  const fine = run('host1', 'bash', '-c', 'echo "debug=$CARGO_PROFILE_DEV_DEBUG"');
  assert.equal(fine.status, 0, fine.stderr);
  assert.match(fine.stdout, /debug=line-tables-only/);

  const filled = run('host2', 'touch', full);
  assert.equal(filled.status, 1);
  assert.match(filled.stderr, /host2 has only 300 MiB free after this run/);
  // A failure keeps its own code.
  fs.rmSync(full);
  const failed = run('host3', 'bash', '-c', `touch ${JSON.stringify(full)}; exit 7`);
  assert.equal(failed.status, 7);
  assert.match(failed.stderr, /300 MiB free/);
});

test('the shared performance target survives ordinary cache eviction', (t) => {
  const h = host(t, 400, MANY);
  fs.mkdirSync(path.join(h.base, 'perf-target'));
  assert.equal(h.trim().status, 0);
  assert.ok(fs.existsSync(path.join(h.base, 'perf-target')));
});

test('a warm shared performance target needs warm-build space rather than cold-build space', (t) => {
  const h = host(t, 6, { 'src-aaa': 0, 'target-aaa': -1 });
  fs.mkdirSync(path.join(h.base, 'perf-target/debug'), { recursive: true });
  fs.writeFileSync(path.join(h.base, 'perf-target/debug/out'), 'warm');
  const r = spawnSync('/bin/bash', [TRIM, 'aaa', 'perf-target'], { cwd: h.base, env: h.env, encoding: 'utf8' });
  assert.equal(r.status, 0, r.stderr);
});


test('perf mode consumes no per-checkout target slot', (t) => {
  const h = host(t, 400, MANY);
  fs.mkdirSync(path.join(h.base, 'perf-target'));
  const r = spawnSync('/bin/bash', [TRIM, 'perfcheckout', 'perf-target'], { cwd: h.base, env: h.env, encoding: 'utf8' });
  assert.equal(r.status, 0, r.stderr);
  assert.ok(!fs.existsSync(path.join(h.base, 'target-perfcheckout')));
  assert.equal(h.left().length, 4);
});

test('perf mode counts and evicts an old normal target without deleting its active source', (t) => {
  const folders = { 'src-aaa': 20, 'target-aaa': 20, 'src-bbb': 1, 'target-bbb': 1, 'src-ccc': 2, 'target-ccc': 2, 'src-ddd': 3, 'target-ddd': 3, 'src-eee': 4, 'target-eee': 4 };
  const h = host(t, 400, folders);
  fs.mkdirSync(path.join(h.base, 'perf-target'));
  const r = spawnSync('/bin/bash', [TRIM, 'aaa', 'perf-target'], { cwd: h.base, env: h.env, encoding: 'utf8' });
  assert.equal(r.status, 0, r.stderr);
  assert.equal(h.left().length, 4);
  assert.ok(!fs.existsSync(path.join(h.base, 'target-aaa')));
  assert.ok(fs.existsSync(path.join(h.base, 'src-aaa')), 'perf run still needs its source');
});

// 13 Nov: one target per checkout, six of them, 34 GB, filled the cloud box.
// Builds now share a few targets (desktop/scripts/shared-cargo/cargo), each
// under a lock its build holds.
function sharedSlot(h: ReturnType<typeof host>, n: number, built: boolean) {
  const t = path.join(h.base, `shared-${n}`, 'target');
  fs.mkdirSync(path.join(t, 'debug'), { recursive: true });
  if (built) fs.writeFileSync(path.join(t, 'debug', 'out'), 'x');
  fs.writeFileSync(path.join(h.base, `shared-${n}.lock`), '');
  return t;
}

test('too short to build at all, a shared target no build holds goes, but only after every idle checkout', (t) => {
  const h = host(t, 1, { 'target-aaa': 0 });
  const slot = sharedSlot(h, 1, true);
  const r = h.trim();
  assert.equal(r.status, 3, r.stderr); // the fake df only counts per-checkout targets
  assert.equal(fs.existsSync(slot), false);
  assert.match(r.stderr, /shared-1\/target, a shared build folder/);
  assert.equal(fs.existsSync(path.join(h.base, 'shared-1')), true, 'its copy of the source stays');
  assert.deepEqual(fs.readdirSync(h.base).filter((n) => n.includes('gone')), []);
  // An idle checkout's target first: that's room enough, so the warm one stays.
  const idle = host(t, 0, { 'target-aaa': 0, 'src-bbb': 30, 'target-bbb': 30 });
  const kept = sharedSlot(idle, 1, true);
  assert.equal(idle.trim().status, 0);
  assert.equal(fs.existsSync(path.join(idle.base, 'target-bbb')), false);
  assert.equal(fs.existsSync(kept), true);
});

test('a shared target a build holds stays, however low the disk', (t) => {
  const h = host(t, 1, { 'target-aaa': 0 });
  const slot = sharedSlot(h, 1, true);
  // A build holding the slot's lock, as shared-cargo's flock does.
  const holder = path.join(h.root, 'hold.pl');
  fs.writeFileSync(holder, `use Fcntl ":flock"; open(my $f, ">>", "shared-1.lock") or die; flock($f, LOCK_EX) or die; $| = 1; print "held\\n"; sleep 4;`);
  const r = spawnSync('/bin/bash', ['-c', `perl ${JSON.stringify(holder)} | { read -r _; bash ${JSON.stringify(TRIM)} aaa; }`], { cwd: h.base, env: h.env, encoding: 'utf8', timeout: 20_000 });
  assert.match(r.stderr, /won't start/);
  assert.equal(fs.existsSync(path.join(slot, 'debug', 'out')), true);
});

test("a checkout with no build of its own isn't cold when a shared target is warm", (t) => {
  const h = host(t, 5, { 'target-aaa': -1 });
  assert.equal(h.trim().status, 3);
  sharedSlot(h, 2, true);
  const r = h.trim();
  assert.equal(r.status, 0, r.stderr);
});

test("remote.sh puts the shared cargo first and clears a checkout's target from before", (t) => {
  const h = host(t, 40, {});
  fs.writeFileSync(path.join(h.bin, 'ssh'), FAKE_SSH, { mode: 0o755 });
  fs.writeFileSync(path.join(h.bin, 'npm'), '#!/bin/bash\nexit 0\n', { mode: 0o755 });
  const findNode = path.join(import.meta.dirname, '..', '..', 'scripts', 'find-node.sh');
  // The shim's own copy, from the checkout's copy to its slot, is a real one.
  fs.writeFileSync(path.join(h.bin, 'rsync'), `#!/bin/bash
case "\${!#}" in *:*) ;; *) exec /usr/bin/rsync "$@" ;; esac
dest="$HOME/\${!##*:}"
mkdir -p "$dest/runner/scripts" "$dest/scripts"
cp ${JSON.stringify(findNode)} "$dest/runner/scripts/"
source="\${@: -2:1}"
for helper in perf-lock.pl perf-children.pl hangup-guard.sh remote-guard-lib.sh build-priority.sh perf-build.sh; do
  cp "$source/scripts/$helper" "$dest/scripts/"
done
mkdir -p "$dest/desktop/scripts"
cp -R "$source/desktop/scripts/shared-cargo" "$dest/desktop/scripts/"
`, { mode: 0o755 });
  // The first run makes this checkout's folders; then its target gets an old-style build in it.
  // A host each: the fake ssh runs here, so a run's in-use marker keeps the
  // last run's turn lock open.
  const run = (where: string, ...cmd: string[]) => spawnSync('/bin/bash', [path.join(SCRIPTS, 'remote.sh'), where, '--', ...cmd], { env: h.env, encoding: 'utf8', timeout: 30_000 });
  assert.equal(run('host1', 'true').status, 0);
  const target = path.join(h.base, fs.readdirSync(h.base).find((n) => n.startsWith('target-'))!);
  fs.mkdirSync(path.join(target, 'debug', '.fingerprint'), { recursive: true });
  fs.writeFileSync(path.join(target, 'debug', 'big'), 'x');
  const r = run('host2', 'bash', '-c', 'echo "path=$PATH"; echo "base=$THREADBEAR_SHARED_CARGO_BASE src=$THREADBEAR_SHARED_CARGO_SRC target=$CARGO_TARGET_DIR"');
  assert.equal(r.status, 0, r.stderr);
  const first = /path=([^:\n]*)/.exec(r.stdout)![1];
  assert.match(first, /\/src-[0-9a-f]+\/desktop\/scripts\/shared-cargo$/);
  assert.match(r.stdout, new RegExp(`base=${h.base} src=${h.base}/src-[0-9a-f]+ target=${target}`));
  assert.equal(fs.existsSync(path.join(target, 'debug', 'big')), false, 'the old build went');
  assert.equal(fs.existsSync(target), true);
  // And cargo through the whole chain: this, build-priority's, then the real one, once.
  const log = path.join(h.root, 'cargo.log');
  fs.writeFileSync(path.join(h.bin, 'cargo'), `#!/bin/bash\necho "$(pwd -P) $CARGO_TARGET_DIR $*" >>${JSON.stringify(log)}\n`, { mode: 0o755 });
  const built = run('host3', 'bash', '-c', 'cd desktop && cargo build -p x');
  assert.equal(built.status, 0, built.stderr);
  const lines = fs.readFileSync(log, 'utf8').trim().split('\n');
  assert.equal(lines.length, 1, lines.join('\n'));
  assert.match(lines[0], new RegExp(`/shared-1/src/desktop ${h.base}/shared-1/target build -p x$`));
  // The three-day age limit spares a folder whose run is still marking it in use.
  const days = new Date(Date.now() - 4 * 24 * HOUR);
  for (const name of ['target-leased', 'target-stale', 'src-stale']) {
    fs.mkdirSync(path.join(h.base, name), { recursive: true });
    if (name === 'target-leased') fs.writeFileSync(path.join(h.base, name, '.in-use'), '');
    else fs.writeFileSync(path.join(h.base, name, 'old'), '');
    fs.utimesSync(path.join(h.base, name, name === 'target-leased' ? '.in-use' : 'old'), name === 'target-leased' ? new Date() : days, name === 'target-leased' ? new Date() : days);
    fs.utimesSync(path.join(h.base, name), days, days);
  }
  assert.equal(run('host4', 'true').status, 0);
  assert.equal(fs.existsSync(path.join(h.base, 'target-leased')), true, 'a run still in it');
  assert.equal(fs.existsSync(path.join(h.base, 'target-stale')), false);
  assert.equal(fs.existsSync(path.join(h.base, 'src-stale')), false);
});

test("an idle checkout's whole build from before the shared targets goes, even with room", (t) => {
  const h = host(t, 400, { 'target-aaa': 0, 'src-old': 2, 'target-old': 2, 'src-busy': 0.1, 'target-busy': 0.1, 'src-new': 2, 'target-new': 2 });
  for (const k of ['old', 'busy']) fs.mkdirSync(path.join(h.base, `target-${k}`, 'debug', '.fingerprint'));
  // mkdir changed the folders' times: put the idle one's back.
  const when = new Date(Date.now() - 2 * HOUR);
  fs.utimesSync(path.join(h.base, 'target-old', 'debug', '.fingerprint'), when, when);
  fs.utimesSync(path.join(h.base, 'target-old', 'debug'), when, when);
  const r = h.trim();
  assert.equal(r.status, 0, r.stderr);
  assert.deepEqual(h.left(), ['target-aaa', 'target-busy', 'target-new']);
  assert.match(r.stderr, /target-old, a whole build from before/);
});

test("check-remote.sh's trim gives nothing back while a build or measurement holds the host", (t) => {
  const h = host(t, 1, WORK);
  const lock = path.join(h.root, 'home', '.cache', 'threadbear-perf.lock');
  fs.writeFileSync(lock, '');
  const holder = path.join(h.root, 'hold.pl');
  fs.writeFileSync(holder, `use Fcntl ":flock"; open(my $f, ">>", ${JSON.stringify(lock)}) or die; flock($f, LOCK_EX) or die; $| = 1; print "held\\n"; sleep 4;`);
  const r = spawnSync('/bin/bash', ['-c', `perl ${JSON.stringify(holder)} | { read -r _; bash ${JSON.stringify(TRIM)} -; }`], { cwd: h.base, env: h.env, encoding: 'utf8', timeout: 20_000 });
  assert.equal(r.status, 0, r.stderr);
  assert.match(r.stderr, /no build folders are given back now/);
  assert.deepEqual(h.left(), ['target-aaa', 'target-bbb', 'target-ccc', 'target-ddd']);
  // Free again, it does.
  assert.equal(h.trim('-').status, 0);
  assert.deepEqual(h.left(), ['target-aaa', 'target-ddd']);
});
