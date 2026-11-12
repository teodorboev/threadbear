import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { execFileSync } from 'node:child_process';
import { makeHarness, tmpDir } from '../helpers/fakes.ts';
import { inBackground, jobPriority, TASKPOLICY } from '../../src/background.ts';
import { runCommand } from '../../src/patches.ts';
import { land } from '../../src/worktrees.ts';

// Builds and tests Threadbear spawns itself run below normal priority (a
// land's check held Mike's M5 above load 24 on 6 Nov): how far is
// runner.json's jobPriority, nice by default, as background priority made a
// landing on someone else's Mac take four hours (11 Nov).

test('inBackground: taskpolicy -b for background on a Mac that has it, nice for low, as it is for normal', () => {
  const sh = ['/bin/sh', ['-c', 'x']] as const;
  const nice = ['/usr/bin/nice', ['-n', '10', '/bin/sh', '-c', 'x']];
  const run = (priority: 'background' | 'low' | 'normal', platform: NodeJS.Platform, exists: (p: string) => boolean) =>
    inBackground(sh[0], [...sh[1]], { platform, exists, priority });
  assert.deepEqual(run('background', 'darwin', () => true), [TASKPOLICY, ['-b', '/bin/sh', '-c', 'x']]);
  // No taskpolicy (Linux, or a Mac without it): background is low.
  assert.deepEqual(run('background', 'darwin', (p) => p !== TASKPOLICY), nice);
  assert.deepEqual(run('background', 'linux', () => true), nice);
  assert.deepEqual(run('low', 'darwin', () => true), nice);
  assert.deepEqual(run('low', 'linux', () => false), ['nice', ['-n', '10', '/bin/sh', '-c', 'x']]);
  assert.deepEqual(run('normal', 'darwin', () => true), ['/bin/sh', ['-c', 'x']]);
  assert.deepEqual(run('low', 'win32', () => true), ['/bin/sh', ['-c', 'x']]);
});

test("jobPriority: THREADBEAR_JOB_PRIORITY, then runner.json's jobPriority, then low", () => {
  const home = tmpDir('tb-pri-');
  assert.equal(jobPriority({ THREADBEAR_HOME: home }), 'low');
  fs.writeFileSync(path.join(home, 'runner.json'), JSON.stringify({ jobPriority: 'background' }));
  assert.equal(jobPriority({ THREADBEAR_HOME: home }), 'background');
  assert.equal(jobPriority({ THREADBEAR_HOME: home, THREADBEAR_JOB_PRIORITY: 'normal' }), 'normal');
  // Nonsense in either is passed over.
  assert.equal(jobPriority({ THREADBEAR_HOME: home, THREADBEAR_JOB_PRIORITY: 'fast' }), 'background');
  fs.writeFileSync(path.join(home, 'runner.json'), JSON.stringify({ jobPriority: 'idle' }));
  assert.equal(jobPriority({ THREADBEAR_HOME: home }), 'low');
});

// The runtime tests below say which they want, whatever runner.json this
// suite's environment would find.
function withPriority(p: string): () => void {
  const was = process.env.THREADBEAR_JOB_PRIORITY;
  process.env.THREADBEAR_JOB_PRIORITY = p;
  return () => {
    if (was === undefined) delete process.env.THREADBEAR_JOB_PRIORITY;
    else process.env.THREADBEAR_JOB_PRIORITY = was;
  };
}

// A darwin-background process reports scheduling priority 4 in ps (20 is
// normal). Only the background side is checked: the suite itself may run
// under taskpolicy -b, and then everything it starts is 4 anyway.
const onMac = process.platform === 'darwin' && fs.existsSync(TASKPOLICY);
const PRI = 'ps -o pri= -p $$';

test('runCommand: background runs at background priority', { skip: !onMac }, async (t) => {
  t.after(withPriority('background'));
  const r = await runCommand(PRI, { cwd: tmpDir(), env: { PATH: '/usr/bin:/bin' }, shell: ['/bin/sh', '-c'], background: true });
  assert.equal(r.ok, true, r.text);
  assert.equal(r.text.trim(), '4');
});

async function landPriority(command: string): Promise<string> {
  const root = fs.realpathSync(tmpDir('tb-bg-'));
  const main = path.join(root, 'repo');
  const git = (cwd: string, ...args: string[]) => execFileSync('git', args, { cwd, encoding: 'utf8' }).trim();
  fs.mkdirSync(main);
  git(main, 'init', '-q', '-b', 'main');
  git(main, 'config', 'user.email', 't@example.com');
  git(main, 'config', 'user.name', 'T');
  fs.writeFileSync(path.join(main, 'a.txt'), 'a\n');
  git(main, 'add', '.');
  git(main, 'commit', '-q', '-m', 'init');
  const wt = path.join(root, 'wt');
  git(main, 'worktree', 'add', '-q', '-b', 'thread/bg', wt);
  fs.writeFileSync(path.join(wt, 'b.txt'), 'b\n');
  git(wt, 'add', '.');
  git(wt, 'commit', '-q', '-m', 'b');
  const pri = path.join(root, 'pri.txt');
  const code = await land(
    { path: wt, branch: 'thread/bg', mainCheckout: main, check: `${command} > '${pri}'` },
    { out: () => {}, err: () => {}, shell: ['/bin/sh', '-c'] },
  );
  assert.equal(code, 0);
  return fs.readFileSync(pri, 'utf8').trim();
}

test('land: the check runs at background priority when runner.json says so', { skip: !onMac }, async (t) => {
  t.after(withPriority('background'));
  assert.equal(await landPriority(PRI), '4');
});

// Ten nicer than this suite (which may itself be niced on a check host),
// up to the most there is.
test('land: the check runs niced by default', async (t) => {
  t.after(withPriority('low'));
  const own = Number(execFileSync('/bin/ps', ['-o', 'nice=', '-p', String(process.pid)], { encoding: 'utf8' }).trim());
  const got = Number(await landPriority('ps -o nice= -p $$'));
  assert.equal(got, Math.min(own + 10, process.platform === 'darwin' ? 20 : 19));
});

test('runner: a turn hook runs at background priority unless the runner is told otherwise', { skip: !onMac }, async (t) => {
  t.after(withPriority('background'));
  const pri = async (backgroundJobs: boolean): Promise<string | undefined> => {
    const root = tmpDir();
    const project = path.join(root, 'app');
    fs.mkdirSync(path.join(project, '.git'), { recursive: true });
    fs.mkdirSync(path.join(project, '.threadbear', 'patches'), { recursive: true });
    fs.writeFileSync(path.join(project, '.threadbear', 'patches', 'hooks.ts'), `export default { hooks: [{ title: "Pri", on: "turn.end", run: "${PRI}", tell: "never" }] };\n`);
    const h = makeHarness(undefined, {}, { panelsRoot: root, actionShell: ['/bin/sh', '-c'], actionTimeoutMs: 20_000, backgroundJobs });
    const { thread } = h.runner.create({ text: 'hi', accountId: 'claude-3', cwd: project });
    await new Promise((r) => setImmediate(r));
    await h.runner.patchesIdle();
    const s = h.adapter.last;
    s.init();
    s.result('completed');
    const runs = () => h.events(thread.id).filter((e) => e.type === 'action.run');
    for (let i = 0; i < 2000 && !runs().length; i++) await new Promise((r) => setTimeout(r, 10));
    return (runs()[0] as { text?: string } | undefined)?.text?.trim();
  };
  assert.equal(await pri(true), '4');
  // Off, it runs as the runner does: at this suite's own priority, which
  // depends on how it was started (20 under a GUI login, 31 over ssh, as
  // when ./check.sh sends it to a check host, 4 under taskpolicy -b).
  const own = execFileSync('/bin/ps', ['-o', 'pri=', '-p', String(process.pid)], { encoding: 'utf8' }).trim();
  assert.equal((await pri(false)) ?? '', own);
});
