// check.sh runs the Swift tests that read the shared mock runner
// (MockRunnerTests and the others swift-freeze.sh --mock-tests names), by
// themselves and --quick included, when the mock or those tests changed. On
// 15 Nov the mock's demo data was renamed, three MockRunnerTests broke, and
// main stayed red for check.sh --app, as --quick never ran them.
//
// This runs the real check.sh --local in a throwaway checkout whose other
// steps are stand-ins that pass at once, and a stand-in swift that only
// says what it was asked, so only what check.sh picks to run is under test.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../../..');

function checkout() {
  const root = fs.mkdtempSync(path.join(fs.realpathSync(os.tmpdir()), 'tb-check-mock-'));
  const put = (rel: string, text: string) => {
    fs.mkdirSync(path.dirname(path.join(root, rel)), { recursive: true });
    fs.writeFileSync(path.join(root, rel), text);
  };
  const copy = (rel: string) => put(rel, fs.readFileSync(path.join(repo, rel), 'utf8'));
  copy('check.sh');
  fs.chmodSync(path.join(root, 'check.sh'), 0o755);
  copy('scripts/swift-freeze.sh');
  fs.chmodSync(path.join(root, 'scripts/swift-freeze.sh'), 0o755);
  copy('runner/scripts/find-node.sh');
  copy('scripts/limit-step.mjs');
  copy('runner/test/helpers/failed-files-reporter.mjs');
  // Every other step check.sh --quick runs, passing.
  put('runner/package.json', JSON.stringify({ scripts: { typecheck: 'true' } }));
  put('runner/node_modules/.keep', '');
  put('runner/test/helpers/clean-env.ts', '');
  put('runner/test/unit/ok.test.ts', "import { test } from 'node:test';\ntest('ok', () => {});\n");
  put('package.json', JSON.stringify({ scripts: { 'convex:typecheck': 'true', 'web:typecheck': 'true', 'web:test': 'true' } }));
  put('node_modules/.keep', '');
  put('app/Tools/ok.mjs', '');
  put('app/Tools/mock-runner/ok.mjs', '');
  put('app/Tools/mock-runner/node_modules/.keep', '');
  put('desktop/.keep', '');
  // swift and cargo that only record what they were asked.
  const log = path.join(root, 'calls.log');
  put('bin/swift', `#!/bin/bash\necho "swift $*" >>'${log}'\n`);
  put('bin/cargo', `#!/bin/bash\necho "cargo $*" >>'${log}'\n`);
  fs.chmodSync(path.join(root, 'bin/swift'), 0o755);
  fs.chmodSync(path.join(root, 'bin/cargo'), 0o755);
  return { root, log };
}

function run(c: { root: string; log: string }, flags: string[], changed: string[], extra: NodeJS.ProcessEnv = {}) {
  fs.rmSync(c.log, { force: true });
  const list = path.join(c.root, '.changed');
  fs.writeFileSync(list, changed.join('\n') + (changed.length ? '\n' : ''));
  const env: NodeJS.ProcessEnv = {
    ...process.env,
    PATH: `${path.join(c.root, 'bin')}:${process.env.PATH}`,
    THREADBEAR_CHECK_CHANGED_FILE: list,
    THREADBEAR_NODE: process.execPath,
    TMPDIR: c.root,
    // As if Xcode is there, so the swift test steps run rather than skip.
    DEVELOPER_DIR: '/Applications/Xcode.app/Contents/Developer',
    ...extra,
  };
  for (const k of Object.keys(env)) if (/^THREAD(BEAR|BARE)_CHECK_HOST$/.test(k)) delete env[k];
  const r = spawnSync('bash', [path.join(c.root, 'check.sh'), ...flags, '--local'], { env, encoding: 'utf8', timeout: 120_000 });
  const calls = fs.existsSync(c.log) ? fs.readFileSync(c.log, 'utf8').trim().split('\n') : [];
  return { status: r.status, out: (r.stdout ?? '') + (r.stderr ?? ''), swift: calls.filter((l) => l.startsWith('swift ')) };
}

const FILTERED = 'swift test --disable-keychain --filter MockRunnerTests|RunnerAuthTests|GoldenTranscriptTests';

test('--quick runs just the mock runner Swift tests when the mock or they changed', () => {
  const c = checkout();
  try {
    for (const changed of ['app/Tools/mock-runner/server.mjs', 'app/Tools/mock-runner/fixtures/demo-x/thread.json', 'app/Tests/ThreadbearCoreTests/MockRunnerTests.swift', 'app/Tests/ThreadbearCoreTests/GoldenTranscriptTests.swift']) {
      const r = run(c, ['--quick'], [changed]);
      assert.equal(r.status, 0, `${changed}: ${r.out}`);
      assert.match(r.out, /^ok    app mock tests/m, `${changed}: ${r.out}`);
      // (A change to a Swift test file also builds the app, as any app/ change does.)
      assert.deepEqual(r.swift.filter((l) => l.startsWith('swift test')), [FILTERED], changed);
    }
  } finally {
    fs.rmSync(c.root, { recursive: true, force: true });
  }
});

test('--quick runs no Swift tests for anything else, nor on main with nothing changed', () => {
  const c = checkout();
  try {
    for (const changed of [['runner/src/x.ts'], ['app/Tests/Fixtures/x.json'], ['desktop/core/src/x.rs'], []]) {
      const r = run(c, ['--quick'], changed);
      assert.doesNotMatch(r.out, /app mock tests/, `${changed}: ${r.out}`);
      assert.deepEqual(r.swift.filter((l) => l.startsWith('swift test')), [], `${changed}`);
    }
  } finally {
    fs.rmSync(c.root, { recursive: true, force: true });
  }
});

test('--runner never does, and the full check runs the whole suite instead when the app changed', () => {
  const c = checkout();
  try {
    const runner = run(c, ['--runner'], ['app/Tools/mock-runner/server.mjs']);
    assert.deepEqual(runner.swift, [], runner.out);
    // A mock change alone: the full check has no Swift app build, so it
    // runs the filter as --quick does.
    const mock = run(c, [], ['app/Tools/mock-runner/server.mjs']);
    assert.ok(mock.swift.includes(FILTERED), mock.out);
    // With the Swift app changed too, its whole suite covers them: no second run.
    const both = run(c, ['--app'], ['app/Tools/mock-runner/server.mjs', 'app/Sources/Threadbear/X.swift']);
    assert.ok(both.swift.includes('swift test --disable-keychain'), both.out);
    assert.ok(!both.swift.includes(FILTERED), both.out);
  } finally {
    fs.rmSync(c.root, { recursive: true, force: true });
  }
});

test('with no check host to take it (the fallback on this Mac) they still run, rather than land unchecked', () => {
  const c = checkout();
  try {
    const r = run(c, ['--quick'], ['app/Tools/mock-runner/server.mjs'], { THREADBEAR_CHECK_FALLBACK: '1' });
    assert.ok(r.swift.includes(FILTERED), r.out);
  } finally {
    fs.rmSync(c.root, { recursive: true, force: true });
  }
});
