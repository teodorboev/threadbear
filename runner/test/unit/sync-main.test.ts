// scripts/sync-main.sh (docs/thread-worktrees.md, "Syncing main with
// GitHub"). On 13 Nov it waited 30 minutes and more for the landing lock
// behind queued landings, just to push a main that was only ahead of
// GitHub's, so GitHub's lagged. Only ahead now pushes straight away.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { execFileSync, spawn } from 'node:child_process';
import { tmpDir } from '../helpers/fakes.ts';
import { holdUntil } from '../helpers/patience.ts';

const SCRIPT = path.join(import.meta.dirname, '..', '..', '..', 'scripts', 'sync-main.sh');
const env = { ...process.env, GIT_CONFIG_GLOBAL: '/dev/null', GIT_CONFIG_NOSYSTEM: '1', GIT_AUTHOR_NAME: 'T', GIT_AUTHOR_EMAIL: 't@t', GIT_COMMITTER_NAME: 'T', GIT_COMMITTER_EMAIL: 't@t' };
const sh = (cwd: string, cmd: string) => execFileSync('/bin/sh', ['-c', cmd], { cwd, env, encoding: 'utf8' }).trim();

function repos() {
  const root = fs.realpathSync(tmpDir('sync-main-'));
  sh(root, 'git init -q --bare -b main origin.git && git clone -q origin.git main 2>/dev/null');
  const main = path.join(root, 'main');
  sh(main, 'git checkout -q -B main && mkdir scripts && echo a > a && git add a && git commit -qm a && git push -q origin main');
  fs.copyFileSync(SCRIPT, path.join(main, 'scripts', 'sync-main.sh'));
  fs.chmodSync(path.join(main, 'scripts', 'sync-main.sh'), 0o755);
  sh(main, 'echo scripts/ > .git/info/exclude');
  return { root, main };
}

// Holds the landing lock, as a landing's check would, until `release` exists.
function holdLock(root: string, main: string) {
  const lock = path.join(main, '.git', 'threadbear-land.lock');
  const wait = `touch '${root}/held'; ${holdUntil(`${root}/release`)}`;
  const p = process.platform === 'darwin'
    ? spawn('/usr/bin/lockf', ['-k', lock, '/bin/sh', '-c', wait], { stdio: 'ignore' })
    : spawn('flock', ['-x', lock, '/bin/sh', '-c', wait], { stdio: 'ignore' });
  p.on('error', () => {});
  return new Promise<void>((r) => p.once('close', () => r()));
}

function run(main: string) {
  const p = spawn(path.join(main, 'scripts', 'sync-main.sh'), [], { cwd: main, env });
  let out = '';
  p.stdout.on('data', (d) => (out += d));
  p.stderr.on('data', (d) => (out += d));
  return { done: new Promise<{ code: number | null; out: string }>((r) => p.once('close', (code) => r({ code, out }))), output: () => out };
}

async function until(check: () => boolean, what: string) {
  const end = Date.now() + 20_000;
  while (!check()) {
    if (Date.now() > end) throw new Error(`timed out: ${what}`);
    await new Promise((r) => setTimeout(r, 20));
  }
}

test('main only ahead of GitHub is pushed straight away, while a landing holds the lock', async () => {
  const { root, main } = repos();
  sh(main, 'echo b > b && git add b && git commit -qm b');
  const held = holdLock(root, main);
  await until(() => fs.existsSync(path.join(root, 'held')), 'lock held');
  try {
    const r = await run(main).done;
    assert.equal(r.code, 0, r.out);
    assert.match(r.out, /Pushed main to GitHub \(1 commits\)/);
    assert.doesNotMatch(r.out, /Taking the landing lock/);
    assert.equal(sh(root, 'git --git-dir=origin.git rev-parse main'), sh(main, 'git rev-parse main'));
  } finally {
    fs.writeFileSync(path.join(root, 'release'), '');
    await held;
  }
  const same = await run(main).done;
  assert.match(same.out, /the same here and on GitHub/);
});

test('when GitHub has moved, it still waits for the landing lock before moving main', async () => {
  const { root, main } = repos();
  sh(root, 'git clone -q origin.git other 2>/dev/null && cd other && git checkout -q main && echo c > c && git add c && git commit -qm c && git push -q origin main');
  const held = holdLock(root, main);
  await until(() => fs.existsSync(path.join(root, 'held')), 'lock held');
  const before = sh(main, 'git rev-parse main');
  const r = run(main);
  await until(() => r.output().includes('Taking the landing lock'), 'waiting for the lock');
  await new Promise((res) => setTimeout(res, 300));
  assert.equal(sh(main, 'git rev-parse main'), before, 'main untouched while a landing holds the lock');
  fs.writeFileSync(path.join(root, 'release'), '');
  await held;
  const done = await r.done;
  assert.equal(done.code, 0, done.out);
  assert.match(done.out, /fast-forwarded to GitHub's/);
  assert.equal(sh(main, 'git rev-parse main'), sh(root, 'git --git-dir=origin.git rev-parse main'));
});
