// desktop/scripts/dev-app.ts's state folder (Daybreak, 13 Nov, finding 116). On a TMPDIR other
// accounts can write to (Linux's /tmp) another account could make threadbear-gpui-dev-app first
// and leave a seed/instance.json in it whose id was "../../<a folder of yours>"; `status` and
// `start` reap stale instances, and that recursive delete followed the id outside. Now the folder
// is this account's own (0700, not a symlink) or refused, an instance's id is its folder's name,
// and every delete is of a direct child.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { tmpDir } from '../helpers/fakes.ts';
import { readInstance, stateRoot, validInstance } from '../../../desktop/scripts/dev-app.ts';

const DEV_APP = path.join(import.meta.dirname, '..', '..', '..', 'desktop', 'scripts', 'dev-app.sh');
const POSIX = process.platform !== 'win32';

function status(tmp: string, ...args: string[]) {
  const r = spawnSync('bash', [DEV_APP, ...(args.length ? args : ['status'])], {
    env: { ...process.env, TMPDIR: tmp, THREADBEAR_THREAD_ID: 'victim-thread' },
    encoding: 'utf8',
    timeout: 60_000,
  });
  return { status: r.status, out: `${r.stdout}${r.stderr}` };
}

function forged(id: string) {
  return { id, thread: 'attacker', checkout: '/elsewhere', created: 0, mock: true, fixtures: [], home: '/tmp/x', live: false, binary: '/bin/app' };
}

test('a stale instance whose instance.json names a path outside is never followed by the reap', { skip: !POSIX }, () => {
  const tmp = fs.realpathSync(tmpDir('gpui-state-trav-'));
  const temp = path.join(tmp, 'tmp');
  fs.mkdirSync(temp, { mode: 0o700 });
  const victim = path.join(tmp, 'victim-parent', 'target');
  fs.mkdirSync(victim, { recursive: true });
  fs.writeFileSync(path.join(victim, 'sentinel'), 'keep');
  // Planted as the write-up has it, as if the folder were someone else's (here it can't be, so
  // this plays what happened past the old folder check).
  const root = path.join(temp, 'threadbear-gpui-dev-app');
  fs.mkdirSync(path.join(root, 'seed'), { recursive: true, mode: 0o700 });
  const rel = path.relative(root, victim);
  fs.writeFileSync(path.join(root, 'seed', 'instance.json'), JSON.stringify(forged(rel)));
  // And one that's "mine" (this thread and checkout), which the reap removes at once.
  fs.mkdirSync(path.join(root, 'seed2'), { mode: 0o700 });
  fs.writeFileSync(path.join(root, 'seed2', 'instance.json'), JSON.stringify({ ...forged(rel), thread: 'victim-thread', checkout: fs.realpathSync(path.join(import.meta.dirname, '..', '..', '..')) }));
  // start reaps too, before anything else: these arguments are refused just after it, so no build.
  const s = status(temp, 'start', '--mock', '--home', path.join(tmp, 'nowhere'));
  assert.match(s.out, /--mock or --home, not both/);
  assert.equal(fs.readFileSync(path.join(victim, 'sentinel'), 'utf8'), 'keep');
  const r = status(temp);
  assert.equal(r.status, 0, r.out);
  assert.equal(fs.readFileSync(path.join(victim, 'sentinel'), 'utf8'), 'keep');
  // The forged records aren't instances at all.
  assert.doesNotMatch(r.out, /attacker/);
});

test('an instance is the folder it was read from, with the fields start writes', () => {
  assert.ok(validInstance(forged('g1'), 'g1'));
  assert.ok(!validInstance(forged('../../x'), '../../x'));
  assert.ok(!validInstance(forged('other'), 'g1'));
  assert.ok(!validInstance({ ...forged('g1'), home: 7 }, 'g1'));
  assert.ok(!validInstance({ ...forged('g1'), supervisor: -1 }, 'g1'));
  assert.ok(!validInstance({ ...forged('g1'), fixtures: [{}] }, 'g1'));
  assert.ok(!validInstance(null, 'g1'));
  assert.equal(readInstance('../x'), null);
  assert.equal(readInstance('a/b'), null);
});

test('on a TMPDIR anyone can write to, the folder has the uid in its name and is refused unless it is ours alone', { skip: !POSIX }, () => {
  const tmp = fs.realpathSync(tmpDir('gpui-state-shared-'));
  const shared = path.join(tmp, 'shared');
  fs.mkdirSync(shared);
  fs.chmodSync(shared, 0o1777);
  const mineName = `threadbear-gpui-dev-app-${process.getuid!()}`;
  const dir = stateRoot(shared);
  assert.equal(dir, path.join(shared, mineName));
  assert.equal(fs.statSync(dir).mode & 0o777, 0o700);
  // Again: the same folder.
  assert.equal(stateRoot(shared), dir);
  // One others can get into is never used, nor repaired.
  fs.chmodSync(dir, 0o777);
  assert.throws(() => stateRoot(shared), /isn't a folder of this account's own/);
  assert.equal(fs.statSync(dir).mode & 0o777, 0o777);
  fs.rmSync(dir, { recursive: true });
  // Nor a symlink to a folder, even a private one.
  const elsewhere = path.join(tmp, 'elsewhere');
  fs.mkdirSync(elsewhere, { mode: 0o700 });
  fs.symlinkSync(elsewhere, dir);
  assert.throws(() => stateRoot(shared), /isn't a folder of this account's own/);
  fs.rmSync(dir);
  // Nor a file.
  fs.writeFileSync(dir, '');
  assert.throws(() => stateRoot(shared), /isn't a folder of this account's own/);
  fs.rmSync(dir);
  // A shared TMPDIR without the sticky bit lets anyone rename the folder away.
  fs.chmodSync(shared, 0o777);
  assert.throws(() => stateRoot(shared), /isn't sticky/);
  // And the command says so rather than going on.
  const r = status(shared);
  assert.notEqual(r.status, 0);
  assert.match(r.out, /isn't sticky/);
});

test('a private TMPDIR (macOS) keeps the plain name, so instances from before still show', { skip: !POSIX }, () => {
  const tmp = fs.realpathSync(tmpDir('gpui-state-private-'));
  assert.equal(stateRoot(tmp), path.join(tmp, 'threadbear-gpui-dev-app'));
  fs.chmodSync(path.join(tmp, 'threadbear-gpui-dev-app'), 0o755);
  assert.throws(() => stateRoot(tmp), /isn't a folder of this account's own/);
});
