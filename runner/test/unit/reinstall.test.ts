// The runner reinstalling its own LaunchAgent (src/reinstall.ts): when it
// starts install-runner.sh, what the Updates panel is told, and what it
// does with the script's exit.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { FakeClock } from '../../src/clock.ts';
import { nullLogger } from '../../src/log.ts';
import { BUSY_EXIT, RUNNING_LIMIT_MS, Reinstaller, installArgs, jobEnv, jobLabel, jobPlist, launcherKey, reinstallDir, reinstallLogPath, type JobSpec } from '../../src/reinstall.ts';

type Setup = { behind?: { reason: string; key: string }; idle?: boolean; auto?: boolean; spawnError?: string; alive?: boolean | undefined };

function setup(s: Setup = {}) {
  const home = fs.mkdtempSync(path.join(os.tmpdir(), 'tb-reinstall-'));
  const clock = new FakeClock();
  const state = { behind: s.behind ?? { reason: "Threadbear Runner.app was built from older launcher sources than main's.", key: 'k1' } as { reason: string; key: string } | undefined, idle: s.idle ?? true, auto: s.auto ?? false, alive: ('alive' in s ? s.alive : true) as boolean | undefined };
  const jobs: JobSpec[] = [];
  let changes = 0;
  const make = () =>
    new Reinstaller({
      home,
      checkout: '/Users/me/dev/threadbear',
      clock,
      log: nullLogger,
      behind: () => state.behind,
      idle: () => state.idle,
      auto: () => state.auto,
      onChange: () => changes++,
      env: { HOME: '/Users/me', PATH: '/usr/bin:/bin', XPC_SERVICE_NAME: 'com.mikerosoft.threadbear.runner', THREADBEAR_THREAD_ID: 'nope', THREADBEAR_HOME: '/elsewhere' },
      platform: 'darwin',
      // The job is going until it writes its result (finish below).
      jobAlive: () => (fs.existsSync(path.join(reinstallDir(home), 'result')) ? false : state.alive),
      spawnJob: (job) => {
        jobs.push(job);
        return s.spawnError;
      },
    });
  // The job finishing: its exit code where the wrapper writes it, and a line of output.
  const finish = (code: number, said = 'some output') => {
    fs.appendFileSync(reinstallLogPath(home), `${said}\n`);
    fs.writeFileSync(path.join(reinstallDir(home), 'result'), `${code}\n`);
  };
  return { home, clock, state, jobs, make, finish, changes: () => changes };
}

test('without the experiment, nothing starts until Reinstall now; the panel shows the button', () => {
  const t = setup();
  const r = t.make();
  r.consider();
  assert.equal(t.jobs.length, 0);
  assert.equal(r.status(), undefined, 'no reinstall field: the app offers Reinstall now');
  const s = r.request();
  assert.equal(t.jobs.length, 1);
  assert.equal(s?.state, 'running');
  assert.equal(t.jobs[0].label, 'com.mikerosoft.threadbear.runner.reinstall');
  assert.equal(t.jobs[0].args[0], '/Users/me/dev/threadbear/runner/scripts/install-runner.sh');
});

test('Reinstall now with a thread busy waits, then starts once nothing is', () => {
  const t = setup({ idle: false });
  const r = t.make();
  assert.equal(r.request()?.state, 'waiting');
  assert.equal(t.jobs.length, 0);
  t.clock.advance(60_000);
  assert.equal(t.jobs.length, 0, 'still busy');
  t.state.idle = true;
  t.clock.advance(60_000);
  assert.equal(t.jobs.length, 1);
  assert.equal(r.status()?.state, 'running');
});

test('with the experiment on, it starts by itself once idle, and says it will before then', () => {
  const t = setup({ auto: true, idle: false });
  const r = t.make();
  r.consider();
  assert.deepEqual(r.status(), { state: 'waiting', auto: true });
  t.state.idle = true;
  t.clock.advance(60_000);
  assert.equal(t.jobs.length, 1);
});

test('a success clears it; the runner it started reads the result after coming up', () => {
  const t = setup({ auto: true });
  t.make().consider();
  assert.equal(t.jobs.length, 1);
  // The old runner is stopped by the script. The new one starts with no
  // updater check yet (behind unknown) and the job still waiting on it.
  t.state.behind = undefined;
  const next = t.make();
  next.consider();
  assert.equal(next.status(), undefined);
  t.finish(0, 'Installed com.mikerosoft.threadbear.runner');
  t.clock.advance(5_000);
  const saved = JSON.parse(fs.readFileSync(path.join(reinstallDir(t.home), 'state.json'), 'utf8'));
  assert.equal(saved.finished.code, 0);
  assert.equal(t.jobs.length, 1, 'not started again');
});

test('a failure shows why, is not retried by itself for the same main, and Try again runs it once more', () => {
  const t = setup({ auto: true });
  const r = t.make();
  r.consider();
  t.finish(1, "The old plist didn't load either.\nBack as it was. The log above says what went wrong; fix it and run this again.");
  t.clock.advance(5_000);
  const s = r.status();
  assert.equal(s?.state, 'failed');
  assert.match(s?.reason ?? '', /Back as it was/);
  assert.equal(s?.logFile, reinstallLogPath(t.home));
  t.clock.advance(10 * 60_000);
  r.consider();
  assert.equal(t.jobs.length, 1, 'not tried again by itself');
  // Main's launcher changes: the experiment tries the new one.
  t.state.behind = { reason: 'launch.mjs has changed on main.', key: 'k2' };
  r.consider();
  assert.equal(t.jobs.length, 2);
  t.finish(1);
  t.clock.advance(5_000);
  assert.equal(r.status()?.state, 'failed');
  // Try again.
  assert.equal(r.request()?.state, 'running');
  assert.equal(t.jobs.length, 3);
  t.finish(1);
  t.clock.advance(5_000);
  assert.equal(r.status()?.state, 'failed', 'a failed Try again is not tried over and over');
  t.clock.advance(10 * 60_000);
  assert.equal(t.jobs.length, 3);
});

test('busy by the time the script checked: waits and goes again, not a failure', () => {
  const t = setup();
  const r = t.make();
  r.request();
  t.state.idle = false;
  t.finish(BUSY_EXIT, "Not replacing the running com.mikerosoft.threadbear.runner while threads are busy: they'd be cut short.");
  t.clock.advance(5_000);
  assert.equal(r.status()?.state, 'waiting');
  t.state.idle = true;
  t.clock.advance(60_000);
  assert.equal(t.jobs.length, 2);
});

test('finished but still behind is a failure, saying what is still behind', () => {
  const t = setup();
  const r = t.make();
  r.request();
  t.finish(0);
  t.clock.advance(5_000);
  const s = r.status();
  assert.equal(s?.state, 'failed');
  assert.match(s?.reason ?? '', /^install-runner\.sh finished, but threadbear Runner\.app was built/i);
});

test('a job still going stays running however long it takes; one that died without a result fails', () => {
  const t = setup();
  const r = t.make();
  r.request();
  t.clock.advance(3 * 60 * 60_000);
  assert.equal(r.status()?.state, 'running', 'a check host can take hours');
  assert.equal(r.request()?.state, 'running', 'Try again leaves a live job alone');
  assert.equal(t.jobs.length, 1);
  t.state.alive = false;
  t.clock.advance(5_000);
  const s = r.status();
  assert.equal(s?.state, 'failed');
  assert.match(s?.reason ?? '', /stopped without finishing/);
});

test("when whether the job is going can't be asked, it's called dead after the limit", () => {
  const t = setup({ alive: undefined });
  const r = t.make();
  r.request();
  t.clock.advance(60_000);
  assert.equal(r.status()?.state, 'running');
  t.clock.advance(RUNNING_LIMIT_MS);
  assert.equal(r.status()?.state, 'failed');
});

test("a job launchd won't take is a failure straight away", () => {
  const t = setup({ spawnError: 'Bootstrap failed: 5: Input/output error' });
  const r = t.make();
  r.request();
  const s = r.status();
  assert.equal(s?.state, 'failed');
  assert.match(s?.reason ?? '', /couldn't start install-runner\.sh: Bootstrap failed/);
});

test('nothing to do when the LaunchAgent is up to date, or the runner runs from a checkout', () => {
  const t = setup({ auto: true });
  t.state.behind = undefined;
  const r = t.make();
  r.consider();
  assert.equal(r.status(), undefined);
  assert.equal(r.request(), undefined);
  assert.equal(t.jobs.length, 0);
});

test("the job's arguments and environment: a sandbox names its home and label; nothing of a thread's goes in", () => {
  const env = { HOME: '/Users/me', PATH: '/usr/bin', XPC_SERVICE_NAME: 'com.mikerosoft.threadbear.runner', THREADBEAR_RUNNER_APP: '/Users/me/Applications/Threadbear Runner.app', THREADBEAR_THREAD_ID: 't', THREADBEAR_HOME: '/x', THREADBEAR_RUNNER_HOME: '/y' };
  assert.deepEqual(installArgs('/c', '/Users/me/Library/Application Support/Threadbear', env, 'darwin'), ['/c/runner/scripts/install-runner.sh', '--runner-app', '/Users/me/Applications/Threadbear Runner.app']);
  const sandbox = { ...env, XPC_SERVICE_NAME: 'com.mikerosoft.threadbear.runner.devtest', THREADBEAR_PORT: '0' };
  assert.deepEqual(installArgs('/c', '/tmp/sb', sandbox, 'darwin'), ['/c/runner/scripts/install-runner.sh', '--home', '/tmp/sb', '--label', 'com.mikerosoft.threadbear.runner.devtest', '--runner-app', '/Users/me/Applications/Threadbear Runner.app', '--port', '0']);
  assert.equal(jobLabel({ XPC_SERVICE_NAME: '0' }, 'darwin'), 'com.mikerosoft.threadbear.runner', 'a process launchd didn\'t start');
  assert.equal(jobLabel({ XPC_SERVICE_NAME: 'x.y' }, 'linux', () => ''), 'com.mikerosoft.threadbear.runner');
  // Linux: the unit it runs in, so a custom --label goes with its --home.
  assert.equal(jobLabel({}, 'linux', () => '0::/user.slice/user-1000.slice/user@1000.service/app.slice/com.example.runner.service\n'), 'com.example.runner');
  const e = jobEnv({ ...env, THREADBEAR_NODE: '/opt/homebrew/Cellar/node/24.1.0/bin/node' }, '/r');
  assert.equal(e.TB_REINSTALL_RESULT, '/r');
  assert.equal(e.THREADBEAR_INSTALL_QUIET, '1');
  // Not the running Node's real path (a Cellar folder brew may have
  // deleted): install-runner.sh finds the pinned one first on PATH.
  assert.equal(e.THREADBEAR_NODE, undefined);
  for (const k of ['THREADBEAR_THREAD_ID', 'THREADBEAR_HOME', 'THREADBEAR_RUNNER_HOME']) assert.equal(e[k], undefined, k);
});

test('the plist is valid, runs once, and its wrapper writes the exit code', { skip: process.platform !== 'darwin' }, () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'tb-reinstall-plist-'));
  const job: JobSpec = { label: 'x.reinstall', args: ['/a b/install-runner.sh', '--home', '/h & <i>'], env: { PATH: '/usr/bin:/bin', TB_REINSTALL_RESULT: path.join(dir, 'result') }, logFile: path.join(dir, 'log'), resultFile: path.join(dir, 'result'), dir };
  const file = path.join(dir, 'job.plist');
  fs.writeFileSync(file, jobPlist(job));
  const lint = spawnSync('plutil', ['-lint', file], { encoding: 'utf8' });
  assert.equal(lint.status, 0, lint.stdout + lint.stderr);
  const parsed = JSON.parse(spawnSync('plutil', ['-convert', 'json', '-o', '-', file], { encoding: 'utf8' }).stdout);
  assert.equal(parsed.KeepAlive, false);
  assert.equal(parsed.RunAtLoad, true);
  assert.deepEqual(parsed.ProgramArguments.slice(4), ['/bin/bash', '/a b/install-runner.sh', '--home', '/h & <i>']);
  // Run the program as launchd would, with a script that exits 3.
  const script = path.join(dir, 'fake.sh');
  fs.writeFileSync(script, 'echo hi; exit 3\n');
  const [cmd, ...rest] = [...parsed.ProgramArguments.slice(0, 5), script];
  const run = spawnSync(cmd, rest, { env: parsed.EnvironmentVariables, encoding: 'utf8' });
  assert.equal(run.status, 0);
  assert.equal(fs.readFileSync(job.resultFile, 'utf8').trim(), '3');
});

test("launcherKey changes with main's launch.mjs or launcher sources", () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'tb-reinstall-key-'));
  fs.mkdirSync(path.join(dir, 'launcher'));
  fs.writeFileSync(path.join(dir, 'launch.mjs'), 'a');
  fs.writeFileSync(path.join(dir, 'launcher', 'main.swift'), 'b');
  const k1 = launcherKey(dir);
  assert.equal(launcherKey(dir), k1);
  fs.writeFileSync(path.join(dir, 'launcher', 'main.swift'), 'c');
  assert.notEqual(launcherKey(dir), k1);
});
