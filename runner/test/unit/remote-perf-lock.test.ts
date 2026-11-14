// Execute remote.sh against a local fake ssh host: its real copy/prelude/run
// paths and real locks, without any build, GUI, network or installed runner.
import { spawn } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import assert from 'node:assert/strict';
import { test } from 'node:test';

const root = path.resolve(import.meta.dirname, '../../..');

for (const mode of ['ordinary', 'perf', 'try', 'try-env', 'busy-copy', 'busy-run']) test(`remote.sh host leases: ${mode}`, async (t) => {
  const perf = mode === 'perf';
  const tryOnly = mode === 'try' || mode.startsWith('busy-');
  const work = fs.mkdtempSync(path.join(os.tmpdir(), 'tb-remote-perf-'));
  t.after(() => fs.rmSync(work, { recursive: true, force: true }));
  const home = path.join(work, 'home');
  const bin = path.join(work, 'bin');
  fs.mkdirSync(home);
  fs.mkdirSync(bin);
  // The occupied phase uses a real lock belonging to this test process.
  // A lock taken during rsync tests the gap between trim and the final run.
  let holder: ReturnType<typeof spawn> | undefined;
  async function hold() {
    fs.mkdirSync(path.join(home, '.cache'), { recursive: true });
    holder = spawn('perl', ['-MFcntl=:flock', '-e', 'open(my $f, ">>", $ARGV[0]) or die $!; flock($f, LOCK_EX) or die $!; $|=1; print "held\\n"; sleep 60', path.join(home, '.cache/threadbear-perf.lock')]);
    t.after(() => holder?.kill());
    await new Promise((r) => holder!.stdout!.once('data', r));
  }
  if (mode === 'busy-copy') await hold();
  fs.writeFileSync(path.join(bin, 'ssh'), '#!/bin/bash\ncd "$HOME"\nexec /bin/bash -c "${@: -1}"\n', { mode: 0o755 });
  fs.writeFileSync(path.join(bin, 'rsync'), `#!/bin/bash
src="\${@: -2:1}"
dest="$HOME/\${!#}"
dest="\${dest/host:/}"
mkdir -p "$dest/scripts" "$dest/runner/scripts" "$dest/desktop"
for name in perf-lock.pl perf-children.pl hangup-guard.sh remote-guard-lib.sh build-priority.sh perf-build.sh; do
  [ ! -f "$src/scripts/$name" ] || cp "$src/scripts/$name" "$dest/scripts/"
done
cp "$src/runner/scripts/find-node.sh" "$dest/runner/scripts/"
for dir in features app/Tools/mock-runner runner; do mkdir -p "$dest/$dir/node_modules"; done
${mode === 'busy-run' ? 'touch "$HOME/start-holder"\nwhile [ ! -f "$HOME/holder-ready" ]; do sleep 0.02; done' : ''}
`, { mode: 0o755 });
  // df is called during heavy cache trimming and again after the command.
  // Prove those phases own both locks too, not just the benchmark body.
  fs.writeFileSync(path.join(bin, 'df'), `#!/bin/bash
perl -MFcntl=:flock -e 'for my $p (@ARGV) { open(my $f, ">>", $p) or die $!; if (flock($f, LOCK_EX | LOCK_NB)) { open(my $bad, ">", "$ENV{HOME}/unlocked"); print {$bad} $p; } }' "$HOME/.cache/threadbear-perf.lock" "$HOME/.cache/threadbear-check/lock"
echo 'Filesystem 1024-blocks Used Available Capacity Mounted'
echo 'fake 0 0 41943040 0% /'
`, { mode: 0o755 });
  const entry = path.join(home, 'measurement.mjs');
  fs.writeFileSync(entry, `import { ensurePerfLock } from ${JSON.stringify(path.join(root, 'features/tools/perf-lock.ts'))}; await ensurePerfLock(); console.log('nested measurement');`);
  const probe = 'use Fcntl ":flock"; for my $p (@ARGV) { open(my $f, ">>", $p) or die $!; if (flock($f, LOCK_EX | LOCK_NB)) { die "host lock not held: $p\\n"; } } print "both locked\\n";';
  const child = spawn('/bin/bash', [path.join(root, 'desktop/scripts/remote.sh'), 'host', ...(perf ? ['--perf'] : []), ...(tryOnly ? ['--try'] : []), '--', 'bash', '-c', 'perl -e "$1" "$HOME/.cache/threadbear-perf.lock" "$HOME/.cache/threadbear-check/lock" && "$2" "$3" && echo "target=$CARGO_TARGET_DIR"', 'probe', probe, process.execPath, entry], {
    env: { ...process.env, HOME: home, PATH: `${bin}:${process.env.PATH}`, THREADBEAR_NODE: process.execPath, THREADBEAR_PERF_LOCK_OWNER: '', THREADBEAR_REMOTE_TRY: mode === 'try-env' ? '1' : '' },
    stdio: ['ignore', 'pipe', 'pipe'],
  });
  let output = '';
  child.stdout!.on('data', (s) => { output += s; });
  child.stderr!.on('data', (s) => { output += s; });
  const done = new Promise((r) => child.on('exit', r));
  const timeout = setTimeout(() => child.kill('SIGTERM'), 15_000);
  t.after(() => clearTimeout(timeout));
  if (mode === 'busy-run') {
    const end = Date.now() + 10_000;
    while (!fs.existsSync(path.join(home, 'start-holder'))) {
      assert.ok(Date.now() < end, output);
      await new Promise((r) => setTimeout(r, 20));
    }
    await hold();
    fs.writeFileSync(path.join(home, 'holder-ready'), '');
  }
  const code = await done;
  if (mode.startsWith('busy-')) {
    assert.equal(code, 75, output);
    assert.doesNotMatch(output, /both locked|nested measurement/);
    const base = path.join(home, '.cache/threadbear-check/gpui');
    assert.ok(!fs.readdirSync(base).some((name) => name.startsWith('run-')), 'a deferred run gives back its tools and command');
    return;
  }
  assert.equal(code, 0, output);
  assert.ok(!fs.existsSync(path.join(home, 'unlocked')), 'cache trim and postlude hold both host locks');
  assert.match(output, /both locked\nnested measurement/);
  assert.match(output, perf ? /target=.*\/perf-target/ : /target=.*\/target-[a-f0-9]+/);
  const base = path.join(home, '.cache/threadbear-check/gpui');
  const src = fs.readdirSync(base).find((name) => name.startsWith('src-'))!;
  assert.ok(fs.existsSync(path.join(base, src, '.in-use')), 'source lease starts before lock waiting');
});
