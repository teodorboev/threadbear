// Starts the real runner entry point (no CLI is ever spawned: claudePath
// points nowhere) to check what happens when two start at once.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { spawn, type ChildProcess } from 'node:child_process';
import { makeHarness, tick, tmpDir } from '../helpers/fakes.ts';
import { groupAlive, killGroup, processInfo } from '../../src/proc.ts';
import { readToken } from '../../src/config.ts';
import { RpcServer } from '../../src/rpc.ts';
import { RunnerClient } from '../../src/client.ts';
import { writeDrainInfo } from '../../src/handover.ts';
import { nullLogger } from '../../src/log.ts';
import { PatchTrust } from '../../src/patch-trust.ts';
import { checkStateFile } from '../../src/schedule-check.ts';
import { Scheduler } from '../../src/schedules.ts';
import { patiently } from '../helpers/patience.ts';

const RUNNER_DIR = path.join(import.meta.dirname, '..', '..');

function startRunner(home: string, port = 0): { proc: ChildProcess; out: () => string; listening: Promise<number>; exited: Promise<number | null> } {
  let out = '';
  const proc = spawn(process.execPath, ['src/main.ts'], {
    cwd: RUNNER_DIR,
    env: {
      ...process.env,
      THREADBEAR_HOME: home,
      THREADBEAR_PORT: String(port),
      THREADBEAR_LOG: path.join(home, 'runner.log'),
      THREADBEAR_CLAUDE_PATH: '/nonexistent/claude',
      THREADBEAR_ACCOUNTS_DIR: path.join(home, 'no-accounts'),
      THREADBEAR_CLAUDE_MAIN_DIR: path.join(home, 'no-main'),
      THREADBEAR_CLAUDE_MAIN_JSON: path.join(home, 'no-main.json'),
      // Never the real ~/.codex (its config.toml) or Token Stats' limits.json.
      THREADBEAR_CODEX_MAIN_DIR: path.join(home, 'no-codex-main'),
      THREADBEAR_TOKEN_STATS_LIMITS: path.join(home, 'no-limits.json'),
      THREADBEAR_CLI_UPDATES: 'off',
    },
    stdio: ['ignore', 'pipe', 'pipe'],
  });
  proc.stdout!.on('data', (d) => (out += d));
  proc.stderr!.on('data', (d) => (out += d));
  const exited = new Promise<number | null>((r) => proc.once('exit', (code) => r(code)));
  const listening = new Promise<number>((resolve, reject) => {
    proc.stdout!.on('data', () => {
      const m = /listening on (\d+)/.exec(out);
      if (m) resolve(Number(m[1]));
    });
    void exited.then((code) => reject(new Error(`exited ${code}: ${out}`)));
  });
  listening.catch(() => {});
  return { proc, out: () => out, listening, exited };
}

test('a second runner on the same data dir refuses to start and touches nothing', async () => {
  const home = tmpDir();
  const a = startRunner(home);
  const port = await a.listening;
  // A process group the first runner owns, as children.json would list it.
  const owned = spawn('/bin/sleep', ['60'], { detached: true, stdio: 'ignore' });
  owned.unref();
  try {
    await new Promise((r) => setTimeout(r, 100));
    fs.writeFileSync(
      path.join(home, 'run', 'children.json'),
      JSON.stringify([{ pid: owned.pid, command: '/bin/sleep', startedAt: new Date().toISOString(), lstart: processInfo(owned.pid!)!.lstart, groups: [] }]),
    );
    const b = startRunner(home);
    assert.equal(await b.exited, 1);
    assert.match(b.out(), /Another runner \(pid \d+\) is already using/);
    assert.equal(groupAlive(owned.pid!), true, "the first runner's CLI was not swept");

    // A runner on another data dir but the same port says so and exits.
    const c = startRunner(tmpDir(), port);
    assert.equal(await c.exited, 1);
    assert.match(c.out(), /port \d+ is already in use/);
    assert.doesNotMatch(c.out(), /Unhandled 'error' event/);

    // Run from a checkout on a temp data dir: no Sentry (sentry.ts).
    assert.match(fs.readFileSync(path.join(home, 'runner.log'), 'utf8'), /runner starting .*"sentry":false/);

    a.proc.kill('SIGTERM');
    assert.equal(await a.exited, 0);
    assert.equal(fs.existsSync(path.join(home, 'run', 'runner.pid')), false, 'the lock is released on a clean stop');
  } finally {
    killGroup(owned.pid!, 'SIGKILL');
    a.proc.kill('SIGKILL');
  }
});

for (const createdAway of [false, true]) test(`the production scheduler runs a draining thread's check in its approved folder (created ${createdAway ? 'after' : 'before'} handover)`, async () => {
  const home = tmpDir();
  const cwd = fs.realpathSync(tmpDir());
  const trust = new PatchTrust({ file: path.join(home, 'patch-trust.json') });
  const h = makeHarness(home, {}, { patchTrust: trust });
  const { thread } = h.runner.create({ text: 'Inspect', cwd, accountId: 'claude-3' });
  h.adapter.last.init();
  h.adapter.last.out({ kind: 'tasks', tasks: [{ id: 'shell', type: 'local_bash', kind: 'shell', description: 'holding the old runner' }] });
  h.adapter.last.result('completed');
  await tick();
  const check = 'pwd > "$THREADBEAR_CHECK_STATE"';
  const params = { threadId: thread.id, text: 'Inspect', at: new Date(Date.now() - 1000).toISOString(), check };
  const oldSched = new Scheduler({ runner: h.runner, home, clock: h.clock, log: nullLogger });
  oldSched.start();
  let schedule = createdAway ? undefined : await oldSched.create(params);
  oldSched.stop();
  await h.runner.beginDrain([thread.id], { onHandedOver: () => assert.fail('the shell keeps the thread on the old runner') });
  h.runner.flush();
  const token = readToken(home);
  const oldServer = new RpcServer({
    host: '127.0.0.1', port: 0, token, runner: h.runner, log: nullLogger, onlyOwn: true,
    methods: { 'drain.hello': () => ({ threads: [thread.id], version: 'old' }) },
  });
  const { port: oldPort } = await oldServer.listen();
  // Peers checks that a recorded PID still runs a main.ts command. The
  // fake server lives here; this idle child supplies its process lifetime.
  const oldProcess = spawn(process.execPath, ['-e', 'const parent = process.ppid; setInterval(() => { try { process.kill(parent, 0); } catch { process.exit(); } }, 1000)', 'fake-main.ts'], { stdio: 'ignore' });
  const oldExited = new Promise((resolve) => oldProcess.once('exit', resolve));
  writeDrainInfo(home, { pid: oldProcess.pid!, port: oldPort, version: 'old', since: new Date().toISOString(), threads: [thread.id], auth: 'challenge' });
  // Boot main.ts itself: a test that passes its own folderElsewhere to
  // Scheduler would miss a dropped or broken production connection to Peers.
  const primary = startRunner(home);
  let client: RunnerClient | undefined;
  try {
    const port = await primary.listening;
    client = await RunnerClient.connect(`ws://127.0.0.1:${port}/rpc`, token);
    const { threads } = await client.call('threads.list', { include: ['inbox'] });
    assert.equal(threads.find((t: { id: string }) => t.id === thread.id)?.onOldRunner, true, fs.readFileSync(path.join(home, 'runner.log'), 'utf8'));
    schedule ??= (await client.call('schedules.create', params)).schedule;
    assert.ok(schedule, 'the primary returned the schedule');
    const state = checkStateFile(home, schedule.id);
    await patiently(() => fs.existsSync(state));
    assert.equal(fs.readFileSync(state, 'utf8').trim(), cwd, primary.out());
    const saved = new PatchTrust({ file: path.join(home, 'patch-trust.json') });
    assert.equal(saved.checkApproved(check, cwd).approved, true);
    assert.equal(saved.checkApproved(check, home).approved, false);
    assert.equal(h.runner.has(thread.id), true, 'the check ran before the old runner handed over its thread');
  } finally {
    client?.close();
    primary.proc.kill('SIGTERM');
    await primary.exited;
    await oldServer.close();
    oldProcess.kill('SIGTERM');
    await oldExited;
  }
});
