// desktop/scripts/shared-cargo/cargo: every checkout's GPUI builds on a host
// share a few warm cargo targets, taking turns, instead of one target each
// (six of them, 34 GB, filled the cloud box on 13 Nov). The fake cargo here
// decides freshness as the real one does, by file times: nothing newer than
// its last build, nothing built. That's what makes a shared target dangerous
// (a checkout's edit copied over before another checkout's build looks
// older than it) and what these check stays safe.
import { spawn, spawnSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { test, type TestContext } from 'node:test';
import assert from 'node:assert/strict';

const SHIM_DIR = path.join(import.meta.dirname, '..', '..', '..', 'desktop', 'scripts', 'shared-cargo');

const FAKE_CARGO = `#!/bin/bash
# Logs where it ran, then builds desktop/app from desktop/src/main.txt into
# $CARGO_TARGET_DIR/debug, unless the build there is newer than every source.
# A held build gives up once the test process has gone, so a failed test
# can't leave it polling for ever.
echo "$$ $(pwd -P) $CARGO_TARGET_DIR \${THREADBEAR_SHARED_CARGO_HELD:-} $*" >>"$FAKE_LOG"
[ -z "\${FAKE_HOLD:-}" ] || { touch "$FAKE_HOLD.started"; while [ ! -e "$FAKE_HOLD.go" ] && kill -0 ${process.pid} 2>/dev/null; do sleep 0.05; done; }
[ "\${1:-}" = build ] || exit "\${FAKE_EXIT:-0}"
d="$CARGO_TARGET_DIR/debug"
mkdir -p "$d/deps"
if [ -f "$d/app" ] && [ -z "$(find src -newer "$d/app" -type f)" ]; then echo fresh >&2; exit "\${FAKE_EXIT:-0}"; fi
echo compiling >&2
sleep 1.1 # file times on some filesystems are to the second
{ echo '#!/bin/sh'; echo "echo $(cat src/main.txt)"; } >"$d/app.tmp"
chmod +x "$d/app.tmp"; mv "$d/app.tmp" "$d/app"
echo rlib >"$d/libapp.rlib"; echo "$d/app: $(pwd -P)/src/main.txt" >"$d/app.d"
exit "\${FAKE_EXIT:-0}"
`;

function host(t: TestContext) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'tb-shared-cargo-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const bin = path.join(root, 'bin');
  const base = path.join(root, 'gpui');
  fs.mkdirSync(bin);
  fs.mkdirSync(base);
  fs.writeFileSync(path.join(bin, 'cargo'), FAKE_CARGO, { mode: 0o755 });
  const log = path.join(root, 'cargo.log');
  // A checkout's copy on the host, as remote.sh leaves it, with its main.txt
  // written `ageSecs` ago.
  const checkout = (key: string, text: string, ageSecs = 0) => {
    const src = path.join(base, `src-${key}`);
    fs.mkdirSync(path.join(src, 'desktop', 'src'), { recursive: true });
    fs.mkdirSync(path.join(src, 'desktop', 'node_modules'), { recursive: true });
    const f = path.join(src, 'desktop', 'src', 'main.txt');
    fs.writeFileSync(f, text);
    const when = new Date(Date.now() - ageSecs * 1000);
    fs.utimesSync(f, when, when);
    return src;
  };
  const envFor = (key: string, extra: Record<string, string> = {}) => ({
    ...process.env,
    PATH: `${SHIM_DIR}:${bin}:${process.env.PATH}`,
    FAKE_LOG: log,
    THREADBEAR_SHARED_CARGO_BASE: base,
    THREADBEAR_SHARED_CARGO_SRC: path.join(base, `src-${key}`),
    THREADBEAR_SHARED_CARGO_KEY: key,
    CARGO_TARGET_DIR: path.join(base, `target-${key}`),
    ...extra,
  });
  const cargo = (key: string, args: string[] = ['build'], extra: Record<string, string> = {}) =>
    spawnSync('cargo', args, { cwd: path.join(base, `src-${key}`, 'desktop'), env: envFor(key, extra), encoding: 'utf8', timeout: 30_000 });
  // What the checkout's own target folder hands to whatever runs next.
  const built = (key: string) => spawnSync(path.join(base, `target-${key}`, 'debug', 'app'), { encoding: 'utf8' }).stdout.trim();
  const logLines = () => (fs.existsSync(log) ? fs.readFileSync(log, 'utf8').trim().split('\n') : []);
  return { root, base, bin, log, checkout, envFor, cargo, built, logLines };
}

test('cargo builds in a shared slot and the checkout gets the program in its own target', (t) => {
  const h = host(t);
  h.checkout('aaa', 'one');
  const r = h.cargo('aaa');
  assert.equal(r.status, 0, r.stderr);
  const [line] = h.logLines();
  const [, cwd, target, held] = line.split(' ');
  assert.equal(cwd, fs.realpathSync(path.join(h.base, 'shared-1', 'src', 'desktop')));
  assert.equal(target, path.join(h.base, 'shared-1', 'target'));
  assert.equal(held, '1');
  assert.equal(h.built('aaa'), 'one');
  // Only the programs and libraries come across, not what cargo keeps for itself.
  assert.deepEqual(fs.readdirSync(path.join(h.base, 'target-aaa', 'debug')).sort(), ['app']);
  // node_modules isn't copied, only linked.
  assert.ok(fs.lstatSync(path.join(h.base, 'shared-1', 'src', 'desktop', 'node_modules')).isSymbolicLink());
});

test("one checkout never gets another's build, whatever the file times say", (t) => {
  const h = host(t);
  // b's copy is older than a's build will be: in its own folder, cargo would call it fresh.
  h.checkout('aaa', 'from a', 60);
  h.checkout('bbb', 'from b', 30);
  const env = { THREADBEAR_REMOTE_SLOTS: '1' };
  assert.equal(h.cargo('aaa', ['build'], env).status, 0);
  assert.equal(h.built('aaa'), 'from a');
  assert.equal(h.cargo('bbb', ['build'], env).status, 0);
  assert.equal(h.built('bbb'), 'from b');
  // And back: a's unchanged, old file still differs from what the slot last built.
  const again = h.cargo('aaa', ['build'], env);
  assert.equal(h.built('aaa'), 'from a');
  assert.match(again.stderr, /compiling/);
  // b's own copy of its build wasn't touched by a's.
  assert.equal(h.built('bbb'), 'from b');
});

test('a run with nothing changed builds nothing and copies nothing', (t) => {
  const h = host(t);
  h.checkout('aaa', 'same', 60);
  assert.equal(h.cargo('aaa').status, 0);
  const app = path.join(h.base, 'target-aaa', 'debug', 'app');
  const ino = fs.statSync(app).ino;
  const r = h.cargo('aaa');
  assert.match(r.stderr, /fresh/);
  assert.equal(fs.statSync(app).ino, ino);
});

test('a fresh build made for another checkout still reaches this one', (t) => {
  const h = host(t);
  h.checkout('aaa', 'shared text', 60);
  h.checkout('bbb', 'shared text', 60);
  const env = { THREADBEAR_REMOTE_SLOTS: '1' };
  assert.equal(h.cargo('aaa', ['build'], env).status, 0);
  const r = h.cargo('bbb', ['build'], env);
  assert.match(r.stderr, /fresh/);
  assert.equal(h.built('bbb'), 'shared text');
});

test("cargo's exit code comes back, and the program is still copied", (t) => {
  const h = host(t);
  h.checkout('aaa', 'x');
  const r = h.cargo('aaa', ['build'], { FAKE_EXIT: '101' });
  assert.equal(r.status, 101);
  assert.equal(h.built('aaa'), 'x');
});

test('paths into the checkout point at the slot', (t) => {
  const h = host(t);
  const src = h.checkout('aaa', 'x');
  assert.equal(h.cargo('aaa', ['test', '--manifest-path', path.join(src, 'desktop', 'Cargo.toml'), `--config=${src}/c.toml`]).status, 0);
  const slot = path.join(h.base, 'shared-1', 'src');
  assert.ok(h.logLines()[0].endsWith(`test --manifest-path ${slot}/desktop/Cargo.toml --config=${slot}/c.toml`), h.logLines()[0]);
});

test('outside a remote run, or outside the copy, it is plain cargo', (t) => {
  const h = host(t);
  h.checkout('aaa', 'x');
  const env = h.envFor('aaa');
  delete (env as Record<string, string | undefined>).THREADBEAR_SHARED_CARGO_BASE;
  spawnSync('cargo', ['build'], { cwd: path.join(h.base, 'src-aaa', 'desktop'), env, encoding: 'utf8' });
  spawnSync('cargo', ['build'], { cwd: h.root, env: h.envFor('aaa'), encoding: 'utf8' });
  spawnSync('cargo', ['--version'], { cwd: path.join(h.base, 'src-aaa', 'desktop'), env: h.envFor('aaa'), encoding: 'utf8' });
  assert.equal(h.logLines().length, 3);
  assert.ok(h.logLines().every((l) => !/shared-\d/.test(l)), h.logLines().join('\n'));
  assert.equal(fs.existsSync(path.join(h.base, 'shared-1')), false);
});

function started(file: string) {
  return new Promise<void>((resolve, reject) => {
    const until = Date.now() + 15_000;
    const tick = () => (fs.existsSync(file) ? resolve() : Date.now() > until ? reject(new Error(`never started: ${file}`)) : setTimeout(tick, 50));
    tick();
  });
}

function run(h: ReturnType<typeof host>, key: string, extra: Record<string, string>) {
  const p = spawn('cargo', ['build'], { cwd: path.join(h.base, `src-${key}`, 'desktop'), env: h.envFor(key, extra), detached: true });
  let stderr = '';
  p.stderr.on('data', (d) => (stderr += d));
  const done = new Promise<number | null>((resolve) => p.on('exit', (code) => resolve(code)));
  return { p, done, stderr: () => stderr };
}

test('builds take turns for a slot, and with two slots two run at once', async (t) => {
  const h = host(t);
  h.checkout('aaa', 'a');
  h.checkout('bbb', 'b');
  h.checkout('ccc', 'c');
  const hold = path.join(h.root, 'hold');
  const first = run(h, 'aaa', { THREADBEAR_REMOTE_SLOTS: '2', FAKE_HOLD: hold });
  await started(`${hold}.started`);
  // A second slot is free: b doesn't wait.
  const second = run(h, 'bbb', { THREADBEAR_REMOTE_SLOTS: '2', FAKE_HOLD: `${hold}-b` });
  await started(`${hold}-b.started`);
  // Both taken: c waits, and says so.
  const third = run(h, 'ccc', { THREADBEAR_REMOTE_SLOTS: '2' });
  await new Promise((r) => setTimeout(r, 1500));
  assert.match(third.stderr(), /Waiting for one of this host's 2 shared build folders/);
  // Only a's and b's cargo have started.
  assert.equal(h.logLines().length, 2);
  fs.writeFileSync(`${hold}.go`, '');
  fs.writeFileSync(`${hold}-b.go`, '');
  assert.deepEqual(await Promise.all([first.done, second.done, third.done]), [0, 0, 0]);
  assert.equal(h.built('aaa'), 'a');
  assert.equal(h.built('bbb'), 'b');
  assert.equal(h.built('ccc'), 'c');
});

test('a checkout goes back to the slot it used last', (t) => {
  const h = host(t);
  h.checkout('aaa', 'a');
  h.checkout('bbb', 'b');
  fs.mkdirSync(path.join(h.base, 'shared-2'), { recursive: true });
  fs.writeFileSync(path.join(h.base, 'shared-2', '.last-key'), 'bbb\n');
  assert.equal(h.cargo('bbb', ['build'], { THREADBEAR_REMOTE_SLOTS: '2' }).status, 0);
  assert.ok(h.logLines()[0].includes('/shared-2/'), h.logLines()[0]);
});

test("a build that's killed leaves its slot free", async (t) => {
  const h = host(t);
  h.checkout('aaa', 'a');
  h.checkout('bbb', 'b');
  const hold = path.join(h.root, 'hold');
  const env = { THREADBEAR_REMOTE_SLOTS: '1' };
  const first = run(h, 'aaa', { ...env, FAKE_HOLD: hold });
  await started(`${hold}.started`);
  // As remote.sh's guard does when the ssh goes: the whole process group.
  process.kill(-first.p.pid!, 'SIGKILL');
  await first.done;
  const r = h.cargo('bbb', ['build'], env);
  assert.equal(r.status, 0, r.stderr);
  assert.doesNotMatch(r.stderr, /Waiting/);
  assert.equal(h.built('bbb'), 'b');
});

test('a program still running from the checkout target is replaced, not rewritten', (t) => {
  const h = host(t);
  h.checkout('aaa', 'old', 60);
  assert.equal(h.cargo('aaa').status, 0);
  const app = path.join(h.base, 'target-aaa', 'debug', 'app');
  const fd = fs.openSync(app, 'r');
  t.after(() => fs.closeSync(fd));
  const before = fs.fstatSync(fd).ino;
  h.checkout('aaa', 'new');
  assert.equal(h.cargo('aaa').status, 0);
  assert.equal(h.built('aaa'), 'new');
  // The open file is the old one, whole.
  assert.equal(fs.fstatSync(fd).ino, before);
  assert.match(fs.readFileSync(fd, 'utf8'), /echo old/);
});

test("a cargo that didn't build the app doesn't hand back another checkout's", (t) => {
  const h = host(t);
  h.checkout('aaa', 'from a', 60);
  h.checkout('bbb', 'from b', 30);
  const env = { THREADBEAR_REMOTE_SLOTS: '1' };
  assert.equal(h.cargo('aaa', ['build'], env).status, 0);
  // `cargo test -p threadbear-core`, say: the app's not built, and the slot's is a's.
  assert.equal(h.cargo('bbb', ['test'], env).status, 0);
  assert.equal(fs.existsSync(path.join(h.base, 'target-bbb', 'debug', 'app')), false);
  // And a's own still comes back to a, whose files the slot then has again.
  fs.rmSync(path.join(h.base, 'target-aaa'), { recursive: true });
  h.checkout('aaa', 'from a', 60);
  assert.equal(h.cargo('aaa', ['test'], env).status, 0);
  assert.equal(fs.existsSync(path.join(h.base, 'target-aaa', 'debug', 'app')), false, "a's files changed back in the slot, so its build there is from another generation");
});

test("the copy's node_modules are there in the slot, for the mock runner the client's tests start", (t) => {
  const h = host(t);
  const src = h.checkout('aaa', 'x');
  fs.mkdirSync(path.join(src, 'app', 'Tools', 'mock-runner', 'node_modules', 'ws'), { recursive: true });
  assert.equal(h.cargo('aaa', ['test']).status, 0);
  const slot = path.join(h.base, 'shared-1', 'src');
  assert.ok(fs.existsSync(path.join(slot, 'app', 'Tools', 'mock-runner', 'node_modules', 'ws')));
  assert.equal(fs.realpathSync(path.join(slot, 'desktop', 'node_modules')), fs.realpathSync(path.join(src, 'desktop', 'node_modules')));
  // Another checkout's run points them at its own.
  const other = h.checkout('bbb', 'y');
  assert.equal(h.cargo('bbb', ['test'], { THREADBEAR_REMOTE_SLOTS: '1' }).status, 0);
  assert.equal(fs.realpathSync(path.join(slot, 'desktop', 'node_modules')), fs.realpathSync(path.join(other, 'desktop', 'node_modules')));
  assert.equal(fs.existsSync(path.join(slot, 'app', 'Tools', 'mock-runner', 'node_modules')), false);
});

test("a program cargo's own list says is up to date comes back, though other files changed", (t) => {
  const h = host(t);
  const a = h.checkout('aaa', 'same app', 60);
  const b = h.checkout('bbb', 'same app', 60);
  // b differs only in a file the app isn't built from (another program's, a README).
  fs.writeFileSync(path.join(b, 'desktop', 'README.md'), 'b');
  fs.writeFileSync(path.join(a, 'desktop', 'README.md'), 'a');
  const env = { THREADBEAR_REMOTE_SLOTS: '1' };
  assert.equal(h.cargo('aaa', ['build'], env).status, 0);
  const r = h.cargo('bbb', ['build'], env);
  assert.match(r.stderr, /fresh/);
  assert.equal(h.built('bbb'), 'same app');
});

// 14 Nov: --perf builds went straight from each checkout's copy into
// perf-target, so every change of checkout rebuilt the vendored GPUI kits.
test('a --perf build uses a source copy of its own and builds straight into perf-target', (t) => {
  const h = host(t);
  h.checkout('aaa', 'same', 60);
  h.checkout('bbb', 'same', 30);
  const perfTarget = path.join(h.base, 'perf-target');
  const perf = (key: string) => h.cargo(key, ['build'], { THREADBEAR_PERF_BUILD_KEY: key, CARGO_TARGET_DIR: perfTarget });
  assert.equal(perf('aaa').status, 0);
  const [line] = h.logLines();
  assert.ok(line.includes(`${fs.realpathSync(h.base)}/perf-slot/src/desktop ${perfTarget} 1 build`), line);
  assert.equal(spawnSync(path.join(perfTarget, 'debug', 'app'), { encoding: 'utf8' }).stdout.trim(), 'same');
  // Another checkout with the same files: nothing rebuilds.
  const r = perf('bbb');
  assert.match(r.stderr, /fresh/);
  // One with a change: that rebuilds, whatever its file's time.
  h.checkout('aaa', 'changed', 3600);
  assert.match(perf('aaa').stderr, /compiling/);
  assert.equal(spawnSync(path.join(perfTarget, 'debug', 'app'), { encoding: 'utf8' }).stdout.trim(), 'changed');
  assert.equal(fs.existsSync(path.join(h.base, 'shared-1')), false, 'ordinary slots untouched');
});
