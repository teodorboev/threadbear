// Where the GPUI app's update is built (desktop/scripts/stage-update.sh).
// What the build host sends back is what the main Mac runs next, so a check
// host, trusted only to run tests, must never become one by being in
// checkHosts: only updateBuildHosts names a builder (Daybreak 108, 12 Nov
// 2026). A Mac with check hosts keeps heavy builds off itself, so with no
// update build host it builds nothing at all rather than building here.
//
// The script runs for real against a tiny checkout whose remote.sh,
// make-bundle.sh and cargo are stand-ins that only note they ran. Hosts are
// .invalid names, so ssh's look at them fails at once and nothing leaves
// this machine.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { tmpDir } from '../helpers/fakes.ts';

const SCRIPT = path.join(import.meta.dirname, '..', '..', '..', 'desktop', 'scripts', 'stage-update.sh');

function write(file: string, text: string, mode = 0o644) {
  fs.mkdirSync(path.dirname(file), { recursive: true });
  fs.writeFileSync(file, text, { mode });
}

function setup(config: Record<string, unknown> | null, remote?: 'busy' | 'legacy') {
  const root = tmpDir();
  const home = path.join(root, 'home');
  const marks = path.join(root, 'marks');
  fs.mkdirSync(marks, { recursive: true });
  const checkout = path.join(root, 'checkout');
  const note = (what: string) => `#!/bin/bash\necho "$*" >> '${marks}/${what}'\n`;
  write(path.join(checkout, 'desktop', 'scripts', 'remote.sh'), `${note('remote')}
cat >/dev/null
echo "try=\${THREADBEAR_REMOTE_TRY-unset}" >> '${marks}/remote'
${remote === 'busy' ? '[ "$1" = free.invalid ] || exit 75' : ''}
${remote === 'legacy' ? '[ "$2" = -- ] || exit 127' : ''}
echo BUILT_IN=/mock-target
`, 0o755);
  write(path.join(checkout, 'desktop', 'scripts', 'make-bundle.sh'), `${note('bundle')}echo "builtOn=\${THREADBEAR_BUILT_ON-unset}" >> '${marks}/bundle'\nmkdir -p "$2"\n`, 0o755);
  // cargo, for a build here: it notes it ran and leaves the two binaries.
  write(path.join(home, '.cargo', 'bin', 'cargo'), `${note('cargo')}mkdir -p "$CARGO_TARGET_DIR/release"\ntouch "$CARGO_TARGET_DIR/release/threadbear-gpui" "$CARGO_TARGET_DIR/release/threadbear-web-helper"\n`, 0o755);
  const git = (...args: string[]) => {
    const r = spawnSync('git', ['-C', checkout, ...args], { encoding: 'utf8', env: { ...process.env, GIT_AUTHOR_NAME: 't', GIT_AUTHOR_EMAIL: 't@t', GIT_COMMITTER_NAME: 't', GIT_COMMITTER_EMAIL: 't@t' } });
    assert.equal(r.status, 0, r.stderr);
  };
  git('init', '-q', '-b', 'main');
  git('add', '-A');
  git('commit', '-q', '-m', 'x');
  const data = path.join(root, 'data');
  fs.mkdirSync(data, { recursive: true });
  if (config) fs.writeFileSync(path.join(data, 'runner.json'), JSON.stringify(config));
  const run = () => {
    const r = spawnSync('bash', [SCRIPT, checkout, '--next', path.join(root, 'next.app')], {
      encoding: 'utf8',
      timeout: 60_000,
      env: { ...process.env, HOME: home, THREADBEAR_HOME: data,
        // Bash imports this stand-in even though stage-update sets a system PATH.
        ...(remote ? { 'BASH_FUNC_ssh%%': '() { cat >/dev/null; echo Darwin; }', 'BASH_FUNC_ioreg%%': '() { :; }', 'BASH_FUNC_scp%%': '() { touch "${@: -1}"; }' } : {}),
      },
    });
    const marked = (what: string) => (fs.existsSync(path.join(marks, what)) ? fs.readFileSync(path.join(marks, what), 'utf8') : '');
    return { status: r.status, out: `${r.stdout}${r.stderr}`, marked, root };
  };
  return run;
}

test('check hosts alone never build the update, and this Mac does not build it either', () => {
  const r = setup({ checkHost: 'checker.invalid', checkHosts: ['checker.invalid', 'cloud.invalid'] })();
  assert.equal(r.status, 5, r.out);
  assert.equal(r.marked('remote'), '', 'nothing sent to a check host to build');
  assert.equal(r.marked('cargo'), '', 'nothing built here');
  assert.equal(r.marked('bundle'), '', 'nothing staged');
  assert.match(r.out, /updateBuildHosts/);
  assert.ok(!fs.existsSync(path.join(r.root, 'next.app')));
});

test('a busy update host defers without staging or marking main as broken', () => {
  const r = setup({ updateBuildHosts: ['builder.invalid'] }, 'busy')();
  assert.equal(r.status, 4, r.out);
  assert.match(r.marked('remote'), /try=1/);
  assert.equal(r.marked('bundle'), '');
  assert.equal(r.marked('cargo'), '');
  assert.match(r.out, /busy|queued/i);
});

test('a busy trusted host leaves the next trusted host a chance to build', () => {
  const r = setup({ updateBuildHosts: ['busy.invalid', 'free.invalid'], checkHosts: ['checker.invalid'] }, 'busy')();
  assert.equal(r.status, 0, r.out);
  assert.match(r.marked('remote'), /busy\.invalid --[^]*free\.invalid --/);
  assert.match(r.marked('bundle'), /^builtOn=free\.invalid$/m);
  assert.doesNotMatch(r.marked('remote'), /checker\.invalid/);
});

test('an installed updater can build main rolled back to a remote.sh from before --try', () => {
  const r = setup({ updateBuildHosts: ['legacy.invalid'] }, 'legacy')();
  assert.equal(r.status, 0, r.out);
  assert.match(r.marked('remote'), /^legacy\.invalid -- bash/m);
  assert.match(r.marked('bundle'), /^builtOn=legacy\.invalid$/m);
});

test('the single checkHost of a runner.json from before the list keeps this Mac from building too', () => {
  const r = setup({ checkHost: 'checker.invalid' })();
  assert.equal(r.status, 5, r.out);
  assert.equal(r.marked('cargo'), '');
  assert.equal(r.marked('remote'), '');
});

test('only the update build hosts are tried, never the check hosts beside them', () => {
  const r = setup({ checkHosts: ['checker.invalid'], updateBuildHosts: ['builder.invalid'] })();
  // builder.invalid doesn't answer: wait and try again later, no build here.
  assert.equal(r.status, 4, r.out);
  assert.match(r.out, /update build hosts \(builder\.invalid \)/);
  assert.doesNotMatch(r.out, /checker\.invalid/);
  assert.equal(r.marked('cargo'), '');
  assert.equal(r.marked('remote'), '');
});

test("a host name ssh would take as an option is never a builder", () => {
  const r = setup({ checkHosts: ['checker.invalid'], updateBuildHosts: ['-oProxyCommand=touch /tmp/tb-stage-pwned', ' spaced.invalid', 7] })();
  assert.equal(r.status, 5, r.out);
  assert.equal(r.marked('cargo'), '');
  assert.equal(r.marked('remote'), '');
});

test("someone else's copy with neither builds here, and the bundle says so", () => {
  for (const config of [null, {}, { updateBuildHosts: [] }]) {
    const r = setup(config)();
    assert.equal(r.status, 0, r.out);
    assert.notEqual(r.marked('cargo'), '', 'built here');
    assert.equal(r.marked('remote'), '');
    assert.match(r.marked('bundle'), /^builtOn=$/m, 'built on this Mac, said as ""');
  }
});

// An installed app's older stage-update.sh runs from its own bundle but takes
// make-bundle.sh from main, and it built on a check host. It says nothing
// about where it built, so main's make-bundle.sh takes its binaries only if
// they're the build it made on this Mac, which it keeps beside them.
const MAKE_BUNDLE = path.join(import.meta.dirname, '..', '..', '..', 'desktop', 'scripts', 'make-bundle.sh');

test("an older updater's binaries from a check host are never made into a bundle", () => {
  const cache = tmpDir();
  const bins = path.join(cache, 'bins');
  for (const bin of ['threadbear-gpui', 'threadbear-web-helper']) write(path.join(bins, bin), 'from the check host', 0o755);
  const make = (env: Record<string, string | undefined>) => {
    const e: Record<string, string | undefined> = { ...process.env, ...env };
    for (const [k, v] of Object.entries(env)) if (v === undefined) delete e[k];
    return spawnSync('bash', [MAKE_BUNDLE, bins, path.join(cache, 'next.app')], { encoding: 'utf8', env: e as NodeJS.ProcessEnv });
  };
  // Nothing built here beside them: refused, and no bundle.
  let r = make({ THREADBEAR_BUILT_ON: undefined });
  assert.equal(r.status, 6, r.stderr);
  assert.ok(!fs.existsSync(path.join(cache, 'next.app')));
  // A different build here: still refused.
  for (const bin of ['threadbear-gpui', 'threadbear-web-helper']) write(path.join(cache, 'target', 'release', bin), 'built here', 0o755);
  r = make({ THREADBEAR_BUILT_ON: undefined });
  assert.equal(r.status, 6, r.stderr);
  // The same bytes as the build here (an older updater building on this Mac,
  // as anyone else's copy does) get past this check, as does a caller that
  // says where it built.
  for (const bin of ['threadbear-gpui', 'threadbear-web-helper']) write(path.join(bins, bin), 'built here', 0o755);
  r = make({ THREADBEAR_BUILT_ON: undefined });
  assert.notEqual(r.status, 6, r.stderr);
  for (const bin of ['threadbear-gpui', 'threadbear-web-helper']) write(path.join(bins, bin), 'from somewhere named', 0o755);
  r = make({ THREADBEAR_BUILT_ON: 'builder' });
  assert.notEqual(r.status, 6, r.stderr);
});
