// tb land's queue (docs/thread-worktrees.md, "Landing together"): on 12 Nov
// a dozen landings each waited for a check of their own, 40 minutes and
// more. Now everyone waiting goes into the next check together.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import { spawn } from 'node:child_process';
import path from 'node:path';
import { land, landTargetFromGit, type LandTarget } from '../../src/worktrees.ts';
import { joinQueue, queueDir, setRunning, waiting, waitingLine } from '../../src/land-queue.ts';
import { capture, commit, makeBranch, makeRepo, sh, until, write } from '../helpers/worktrees-harness.ts';

// A check that writes down which of the files it saw, and holds the first
// landing (a.txt alone) until the test lets it go, so the rest queue up.
function slowCheck(root: string, failIf = 'never-there.txt'): string {
  return `ls *.txt | tr '\\n' ' ' >> '${root}/checks'; echo >> '${root}/checks'; ` +
    `if test -f a.txt && ! test -f b.txt; then touch '${root}/checking'; while ! test -f '${root}/release'; do sleep 0.05; done; fi; ` +
    `! test -f ${failIf}`;
}

function branches(main: string, names: string[]): LandTarget[] {
  return names.map((n) => {
    const t = makeBranch(main, `${n}0000000`);
    commit(t.path, { [`${n}.txt`]: `${n}\n` }, `Add ${n}`);
    return t;
  });
}

async function queuedBehindA(root: string, main: string, others: LandTarget[], a: LandTarget, check: string) {
  const first = land({ ...a, check }, capture().opts);
  await until(() => fs.existsSync(path.join(root, 'checking')), "a's check");
  const caps = others.map(() => capture());
  const rest = others.map((t, i) => land({ ...t, check }, caps[i].opts));
  try {
    await until(() => caps.every((c) => c.out.some((l) => l.startsWith('Waiting for another landing'))), 'the others waiting');
  } finally {
    write(root, { release: '' });
  }
  return { first: await first, codes: await Promise.all(rest), caps };
}

test('everyone waiting lands in one check, each branch moved to what landed', async () => {
  const { root, main } = makeRepo();
  const [a, b, c, d] = branches(main, ['a', 'b', 'c', 'd']);
  const check = slowCheck(root);
  const { first, codes, caps } = await queuedBehindA(root, main, [b, c, d], a, check);
  assert.equal(first, 0);
  assert.deepEqual(codes, [0, 0, 0], caps.map((c) => c.err.join('\n')).join('\n'));
  // a's check, then one for the other three.
  assert.deepEqual(fs.readFileSync(path.join(root, 'checks'), 'utf8').trim().split('\n').map((l) => l.trim()), ['a.txt', 'a.txt b.txt c.txt d.txt']);
  for (const n of ['a', 'b', 'c', 'd']) assert.ok(fs.existsSync(path.join(main, `${n}.txt`)));
  assert.equal(sh(main, 'git log --format=%s main | head -4 | sort | tr "\\n" " "'), 'Add a Add b Add c Add d');
  // Each branch is now on main, so its next tb land lands only what's new.
  for (const t of [b, c, d]) {
    assert.equal(sh(t.path, `git log --oneline main..${t.branch}`), '');
    assert.equal(sh(t.path, 'git rev-parse --abbrev-ref HEAD'), t.branch);
    assert.equal(sh(t.path, 'git status --porcelain'), '');
  }
  const all = caps.map((c) => c.out.join('\n'));
  assert.equal(all.filter((o) => /In the same check, and landed with it: /.test(o)).length, 1, all.join('\n---\n'));
  assert.equal(all.filter((o) => /, in one check with /.test(o)).length, 2, all.join('\n---\n'));
  // Each says what was its own.
  for (const [i, n] of ['b', 'c', 'd'].entries()) assert.match(all[i], new RegExp(`Add ${n}`));
  assert.ok(all.some((o) => /the check for thread\/a0000000 has been running/.test(o)), all.join('\n---\n'));
  assert.deepEqual(waiting(path.join(main, '.git')), [], 'nobody left in the line');
});

test('a group check that fails is narrowed down, and only the one that broke it is refused', async () => {
  const { root, main } = makeRepo();
  const [a, b, c, d] = branches(main, ['a', 'b', 'c', 'd']);
  commit(c.path, { 'bad.txt': 'bad\n' }, 'Break it');
  const check = slowCheck(root, 'bad.txt');
  const { first, codes, caps } = await queuedBehindA(root, main, [b, c, d], a, check);
  assert.equal(first, 0);
  assert.deepEqual(codes, [0, 1, 0], caps.map((x) => x.out.concat(x.err).join('\n')).join('\n---\n'));
  assert.match(caps[1].err.join('\n'), /^The check failed \(exit 1\), so main wasn't touched/);
  assert.ok(!fs.existsSync(path.join(main, 'bad.txt')));
  for (const n of ['a', 'b', 'd']) assert.ok(fs.existsSync(path.join(main, `${n}.txt`)));
  assert.ok(caps.some((x) => x.out.some((l) => /Checking them in smaller groups/.test(l))));
  // Every check that passed had no bad.txt in it: main only moved to those.
  const checks = fs.readFileSync(path.join(root, 'checks'), 'utf8').trim().split('\n');
  assert.ok(checks.length <= 6, checks.join('\n'));
  assert.equal(sh(c.path, 'git rev-parse --abbrev-ref HEAD'), c.branch, 'the failed one is back on its branch');
});

test("a queued landing whose tb has gone is dropped from the line, not landed", async () => {
  const { main } = makeRepo();
  const [a, b] = branches(main, ['a', 'b']);
  const common = path.join(main, '.git');
  const gone = joinQueue(common, { branch: b.branch, path: b.path, base: sh(main, 'git rev-parse main'), commit: sh(b.path, 'git rev-parse HEAD') });
  const file = path.join(queueDir(common), `${gone.id}.json`);
  fs.writeFileSync(file, JSON.stringify({ ...gone, pid: 2 ** 22 + 12345 }));
  assert.equal(await land({ ...a, check: 'true' }, capture().opts), 0);
  assert.ok(!fs.existsSync(path.join(main, 'b.txt')));
  assert.ok(!fs.existsSync(file));
});

test('the waiting line says where it is in the line and roughly how long', () => {
  const { main } = makeRepo();
  const common = path.join(main, '.git');
  const times = path.join(common, 'threadbear-land-times.log');
  fs.writeFileSync(times, [300, 360, 420].map((s) => JSON.stringify({ ok: true, seconds: s })).join('\n') + '\n');
  const now = Date.now();
  setRunning(common, { startedAt: now - 120_000, branches: ['thread/x', 'thread/y'] });
  const ahead = joinQueue(common, { branch: 'thread/z', path: '/z', base: 'b', commit: 'c' });
  const me = joinQueue(common, { branch: 'thread/me', path: '/me', base: 'b', commit: 'c' });
  if (me.at === ahead.at) me.at += 1;
  const line = waitingLine(common, times, me, now);
  assert.equal(line, ': the check for thread/x and 1 more has been running 2 min; 1 more waiting ahead of this one, all checked together with it next, so roughly 10 min to go (a check takes about 6 min here).');
  setRunning(common, undefined);
  assert.equal(waitingLine(common, times, { ...ahead, at: ahead.at - 1 }, now), '...');
});

test('a landing that asks for a different check is never put in another one', async () => {
  const { root, main } = makeRepo();
  const [a, b, c] = branches(main, ['a', 'b', 'c']);
  const check = slowCheck(root);
  const first = land({ ...a, check }, capture().opts);
  await until(() => fs.existsSync(path.join(root, 'checking')), "a's check");
  const cb = capture();
  const cc = capture();
  const rest = [land({ ...b, check: `${check}; true` }, cb.opts), land({ ...c, check }, cc.opts)];
  try {
    await until(() => [cb, cc].every((x) => x.out.some((l) => l.startsWith('Waiting for another landing'))), 'both waiting');
  } finally {
    write(root, { release: '' });
  }
  assert.equal(await first, 0);
  assert.deepEqual(await Promise.all(rest), [0, 0]);
  assert.equal(fs.readFileSync(path.join(root, 'checks'), 'utf8').trim().split('\n').length, 3, 'each checked with its own check');
});

test('two tb lands in one worktree take turns rather than abort each other', async () => {
  const { main } = makeRepo();
  const [a] = branches(main, ['a']);
  commit(main, { 'other.txt': 'x\n' }, 'Main moved');
  const one = capture();
  const two = capture();
  const codes = await Promise.all([land({ ...a, check: 'sleep 0.3' }, one.opts), land({ ...a, check: 'sleep 0.3' }, two.opts)]);
  assert.deepEqual(codes, [0, 0], one.err.concat(two.err).join('\n'));
  assert.ok(one.out.concat(two.out).some((l) => /Another tb land is running in this worktree/.test(l)));
  assert.ok(fs.existsSync(path.join(main, 'a.txt')));
});

test('a worktree left detached by a tb killed during a group check is put back by the next tb land', async () => {
  const { main } = makeRepo();
  const [a] = branches(main, ['a']);
  const own = sh(a.path, 'git rev-parse HEAD');
  const gitDir = sh(a.path, 'git rev-parse --absolute-git-dir');
  fs.writeFileSync(path.join(gitDir, 'threadbear-land-checking.json'), JSON.stringify({ branch: a.branch, own }));
  sh(a.path, 'git checkout -q --detach main');
  const c = capture();
  assert.equal(await land({ ...a, check: 'true' }, c.opts), 0, c.err.join('\n'));
  assert.match(c.out[0], /checked out again as it was/);
  assert.equal(sh(main, 'git rev-parse main'), own);
  assert.ok(!fs.existsSync(path.join(gitDir, 'threadbear-land-checking.json')));
});

test('a commit made empty on purpose lands with its group, as a rebase would keep it', async () => {
  const { root, main } = makeRepo();
  const [a, b, c] = branches(main, ['a', 'b', 'c']);
  // main will have an empty commit too, which --cherry-pick would match.
  sh(a.path, 'git commit -q --allow-empty -m "Empty on a"');
  sh(b.path, 'git commit -q --allow-empty -m "Empty on b"');
  const check = slowCheck(root);
  const { first, codes } = await queuedBehindA(root, main, [b, c], a, check);
  assert.equal(first, 0);
  assert.deepEqual(codes, [0, 0]);
  assert.match(sh(main, 'git log --format=%s main'), /Empty on b/);
  assert.equal(fs.readFileSync(path.join(root, 'checks'), 'utf8').trim().split('\n').length, 2, 'b and c in one check');
});

test('tb land outside a thread finds the branch of a worktree a killed group check left detached', async () => {
  const { main } = makeRepo();
  const [a] = branches(main, ['a']);
  const gitDir = sh(a.path, 'git rev-parse --absolute-git-dir');
  fs.writeFileSync(path.join(gitDir, 'threadbear-land-checking.json'), JSON.stringify({ branch: a.branch, own: sh(a.path, 'git rev-parse HEAD') }));
  sh(a.path, 'git checkout -q --detach main');
  const found = await landTargetFromGit(a.path);
  assert.equal(found.target?.branch, a.branch, found.reason ?? "");
  // A failed put-back keeps the marker for next time.
  write(a.path, { 'a.txt': 'local edit in the way\n' });
  assert.equal(await land({ ...a, check: 'true' }, capture().opts), 1);
  assert.ok(fs.existsSync(path.join(gitDir, 'threadbear-land-checking.json')));
});

test("a workflow red on GitHub for another's files leaves that one out of the group, and it hears why", async () => {
  const { root, main } = makeRepo();
  const [a, b, c] = branches(main, ['a', 'b', 'c']);
  const check = slowCheck(root);
  const hold = async (_root: string, files: string[]) =>
    fs.existsSync(path.join(root, 'red')) && files.includes('c.txt') ? 'Held: the workflow for c.txt is red on main.' : undefined;
  const first = land({ ...a, check }, capture().opts);
  await until(() => fs.existsSync(path.join(root, 'checking')), "a's check");
  const cb = capture();
  const cc = capture();
  const rest = [land({ ...b, check }, { ...cb.opts, hold }), land({ ...c, check }, { ...cc.opts, hold })];
  try {
    await until(() => [cb, cc].every((x) => x.out.some((l) => l.startsWith('Waiting for another landing'))), 'both waiting');
    write(root, { red: '' });
  } finally {
    write(root, { release: '' });
  }
  assert.equal(await first, 0);
  assert.deepEqual(await Promise.all(rest), [0, 1]);
  assert.match(cc.err.join('\n'), /Held: the workflow for c\.txt is red on main\./);
  assert.ok(fs.existsSync(path.join(main, 'b.txt')) && !fs.existsSync(path.join(main, 'c.txt')));
});

// 12 Nov: a hand-run `git rebase origin/main` in the main checkout had HEAD
// detached while a landing fast-forwarded, so the landing moved the detached
// HEAD, and the rebase's end set main without it.
test('a landing refuses while the main checkout is in the middle of a rebase, and main keeps nothing half moved', async () => {
  const { main } = makeRepo();
  const [a] = branches(main, ['a']);
  // A rebase in main that stops on a conflict: HEAD detached, rebase-merge there.
  sh(main, 'git checkout -q -b side HEAD && echo side > README.md && git commit -qam side && git checkout -q main && echo mine > README.md && git commit -qam mine');
  assert.throws(() => sh(main, 'git rebase side'));
  const before = sh(main, 'git rev-parse refs/heads/main');
  const c = capture();
  assert.equal(await land({ ...a, check: 'true' }, c.opts), 1);
  assert.match(c.err.join('\n'), /is in the middle of a rebase .*rebase-merge.*nothing landed/);
  assert.equal(sh(main, 'git rev-parse refs/heads/main'), before);
  sh(main, 'git rebase --abort');
  assert.equal(await land({ ...a, check: 'true' }, capture().opts), 0);
  assert.ok(fs.existsSync(path.join(main, 'a.txt')));
});

test('a rebase started in the main checkout while the check runs stops the landing before main moves', async () => {
  const { main } = makeRepo();
  const [a] = branches(main, ['a']);
  sh(main, 'git checkout -q -b side HEAD && echo side > README.md && git commit -qam side && git checkout -q main && echo mine > README.md && git commit -qam mine');
  const before = sh(main, 'git rev-parse refs/heads/main');
  const c = capture();
  // The check starts the rebase in main, as a sync by hand would mid-check.
  const check = `cd '${main}' && git rebase side >/dev/null 2>&1; true`;
  assert.equal(await land({ ...a, check }, c.opts), 1);
  assert.match(c.err.join('\n'), /in the middle of a rebase/);
  assert.equal(sh(main, 'git rev-parse refs/heads/main'), before);
  assert.doesNotMatch(sh(main, 'git log --format=%s HEAD'), /Add a/, 'nor did the detached HEAD get it');
  sh(main, 'git rebase --abort');
});

test('the main checkout on a detached HEAD is refused by name', async () => {
  const { main } = makeRepo();
  const [a] = branches(main, ['a']);
  sh(main, 'git checkout -q --detach main');
  const c = capture();
  assert.equal(await land({ ...a, check: 'true' }, c.opts), 1);
  assert.match(c.err.join('\n'), /is on a detached HEAD, not main, so tb land can't move main there/);
});

// 12 Nov: session restarts killed waiting tb lands, and the group was
// checked again without each one, so two passing checks landed nothing.
test('a landing whose tb is killed during the group check still lands, if its branch is where it was', async () => {
  const { root, main } = makeRepo();
  const [b, c] = branches(main, ['b', 'c']);
  const common = path.join(main, '.git');
  // c's tb, waiting: an entry, and its lock held by a stand-in that goes
  // once the check has started.
  const e = joinQueue(common, { branch: c.branch, path: c.path, base: sh(main, 'git rev-parse main'), commit: 'later', check: 'CHECK', checkFrom: '' });
  // An empty commit on purpose too: once landed for it, its branch has to
  // be moved, or its next tb land would land that one again.
  sh(c.path, 'git commit -q --allow-empty -m "Empty on c"');
  const cmd = `touch '${root}/held'; while ! test -f '${root}/drop'; do sleep 0.05; done`;
  const lockFile = path.join(queueDir(common), `${e.id}.lock`);
  // Says it's gone once the lock is let go, which the check waits for.
  const wrapper = process.platform === 'darwin'
    ? `/usr/bin/lockf -k '${lockFile}' /bin/sh -c "${cmd}"; touch '${root}/released'`
    : `flock -x '${lockFile}' /bin/sh -c "${cmd}"; touch '${root}/released'`;
  const holder = spawn('/bin/sh', ['-c', wrapper], { stdio: 'ignore' });
  holder.on('error', () => {});
  const gone = new Promise((r) => holder.once('close', r));
  await until(() => fs.existsSync(path.join(root, 'held')), "c's stand-in holding its lock");
  const check = `if test -f c.txt; then touch '${root}/drop'; while ! test -f '${root}/released'; do sleep 0.05; done; fi`;
  // The entry asks for the same check as b's, for c's commits as they are.
  fs.writeFileSync(path.join(queueDir(common), `${e.id}.json`), JSON.stringify({ ...e, commit: sh(c.path, 'git rev-parse HEAD'), check }));
  const out = capture();
  assert.equal(await land({ ...b, check }, out.opts), 0, out.err.join('\n'));
  await gone;
  assert.ok(fs.existsSync(path.join(main, 'b.txt')) && fs.existsSync(path.join(main, 'c.txt')), out.out.join('\n'));
  assert.ok(out.out.some((l) => /In the same check, and landed with it: thread\/c0000000/.test(l)));
  // c's branch is on what landed, so its next tb land has nothing to land.
  const again = capture();
  assert.equal(await land({ ...c, check: 'true' }, again.opts), 0);
  assert.deepEqual(again.out, [`Nothing to land: ${c.branch} has no commits that aren't on main.`]);
  assert.equal(sh(main, 'git log --format=%s main | grep -c "Empty on c"'), '1');
});

test('a cherry-pick of several, stopped and then committed by hand, still holds a landing', async () => {
  const { main } = makeRepo();
  const [a] = branches(main, ['a']);
  sh(main, 'git checkout -q -b side HEAD && echo one > README.md && git commit -qam one && echo two > README.md && git commit -qam two && git checkout -q main && echo mine > README.md && git commit -qam mine');
  assert.throws(() => sh(main, 'git cherry-pick side~1 side'));
  sh(main, 'echo fixed > README.md && git add README.md && git -c core.editor=true commit -q --no-edit');
  const c = capture();
  assert.equal(await land({ ...a, check: 'true' }, c.opts), 1);
  assert.match(c.err.join('\n'), /in the middle of a cherry-pick or revert .*sequencer/);
  sh(main, 'git cherry-pick --quit');
});

test("a branch that landed while its tb was stopped is moved to what landed by its next tb land, so nothing lands twice", async () => {
  const { main } = makeRepo();
  const [c] = branches(main, ['c']);
  sh(c.path, 'git commit -q --allow-empty -m "Empty on c"');
  const asked = sh(c.path, 'git rev-parse HEAD');
  // What a group check landed for it (as a holder that couldn't move its
  // branch leaves it): its commits on main.
  sh(main, `git cherry-pick --allow-empty ${asked}~1 ${asked}`);
  const landed = sh(main, 'git rev-parse main');
  const done = path.join(main, '.git', 'threadbear-land-done');
  fs.mkdirSync(done, { recursive: true });
  fs.writeFileSync(path.join(done, asked), landed);
  const out = capture();
  assert.equal(await land({ ...c, check: 'true' }, out.opts), 0, out.err.join('\n'));
  assert.match(out.out[0], /landed while its tb was stopped/);
  assert.equal(out.out[1], `Nothing to land: ${c.branch} has no commits that aren't on main.`);
  assert.equal(sh(main, 'git log --format=%s main | grep -c "Empty on c"'), '1');
  assert.ok(!fs.existsSync(path.join(done, asked)));
});
