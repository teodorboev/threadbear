// tb land's queue (docs/thread-worktrees.md, "Landing together"): on 12 Nov
// a dozen landings each waited for a check of their own, 40 minutes and
// more. Now everyone waiting goes into the next check together.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import { spawn } from 'node:child_process';
import path from 'node:path';
import { land, landTargetFromGit, type LandTarget } from '../../src/worktrees.ts';
import { joinQueue, leaveQueue, queueDir, readRunning, setRunning, waiting, waitingLine, writeEntry } from '../../src/land-queue.ts';
import { capture, commit, holdUntil, makeBranch, makeRepo, sh, until, wrapGit, write } from '../helpers/worktrees-harness.ts';

// A check that writes down which of the files it saw, and holds the first
// landing (a.txt alone) until the test lets it go, so the rest queue up.
function slowCheck(root: string, failIf = 'never-there.txt'): string {
  return `ls *.txt | tr '\\n' ' ' >> '${root}/checks'; echo >> '${root}/checks'; ` +
    `if test -f a.txt && ! test -f b.txt; then touch '${root}/checking'; ${holdUntil(`${root}/release`)}; fi; ` +
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
  const common = path.join(main, '.git');
  const running = readRunning(common);
  const holderEntries = waiting(common).filter((e) => e.branch === a.branch).map((e) => e.id);
  const caps = others.map(() => capture());
  const rest = others.map((t, i) => land({ ...t, check }, caps[i].opts));
  try {
    assert.equal(running?.pid, process.pid);
    assert.deepEqual(running?.entries, holderEntries);
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

test('a real group check records every entry id and identifies each member as being checked', async () => {
  const { root, main } = makeRepo();
  const [a, b, c] = branches(main, ['a', 'b', 'c']);
  const common = path.join(main, '.git');
  const check = `${slowCheck(root)}; if test -f b.txt; then touch '${root}/group-checking'; ` +
    `${holdUntil(`${root}/group-release`)}; fi`;
  const first = land({ ...a, check }, capture().opts);
  const caps = [capture(), capture()];
  const rest: Promise<number>[] = [];
  let codes: number[] = [];
  try {
    await until(() => fs.existsSync(path.join(root, 'checking')), "a's check");
    rest.push(...[b, c].map((t, i) => land({ ...t, check }, caps[i].opts)));
    await until(() => caps.every((cap) => cap.out.some((line) => line.startsWith('Waiting for another landing'))), 'both queued');
    write(root, { release: '' });
    await until(() => fs.existsSync(path.join(root, 'group-checking')), 'the real group check');
    const running = readRunning(common);
    const entries = waiting(common).filter((entry) => [b.branch, c.branch].includes(entry.branch));
    assert.deepEqual(running?.branches.slice().sort(), [b.branch, c.branch].sort());
    assert.equal(running?.pid, process.pid);
    assert.equal(entries.length, 2);
    assert.deepEqual(running?.entries?.slice().sort(), entries.map((entry) => entry.id).sort());
    for (const entry of entries) {
      assert.match(waitingLine(common, path.join(common, 'no-times'), entry), /this landing is being checked with 1 other landing/);
    }
  } finally {
    write(root, { release: '', 'group-release': '' });
    codes = await Promise.all([first, ...rest]);
  }
  assert.deepEqual(codes, [0, 0, 0], caps.map((cap) => cap.err.join('\n')).join('\n'));
});

test('a group check that fails is narrowed down, and only the one that broke it is refused', async () => {
  const { root, main } = makeRepo();
  const [a, b, c, d] = branches(main, ['a', 'b', 'c', 'd']);
  commit(c.path, { 'bad.txt': 'bad\n' }, 'Break it');
  const check = `${slowCheck(root)}; if test -f bad.txt; then ` +
    `{ for i in $(seq 1 80); do echo "passing output $i"; done; ` +
    `printf '%12000s\\n' '' | tr ' ' x; ` +
    `echo 'FAIL desktop core tests /retained/group/desktop-core-tests.log'; ` +
    `echo 'open_gate_source: hostname URI format failed'; } >&2; exit 3; fi`;
  const { first, codes, caps } = await queuedBehindA(root, main, [b, c, d], a, check);
  assert.equal(first, 0);
  assert.deepEqual(codes, [0, 1, 0], caps.map((x) => x.out.concat(x.err).join('\n')).join('\n---\n'));
  assert.match(caps[1].err.join('\n'), /^The check failed \(exit 3\), so main wasn't touched/);
  assert.ok(!fs.existsSync(path.join(main, 'bad.txt')));
  for (const n of ['a', 'b', 'd']) assert.ok(fs.existsSync(path.join(main, `${n}.txt`)));
  assert.ok(caps.some((x) => x.out.some((l) => /Checking them in smaller groups/.test(l))));
  // The group holder reports what failed before starting a smaller check,
  // while each landing still gets its own eventual result.
  assert.ok(caps.some((cap) => cap.out.some((line) => line.startsWith('The check failed with'))), 'the holder prints the grouped failure diagnostic');
  for (const cap of caps) {
    for (const [i, line] of cap.out.entries()) {
      if (!line.startsWith('The check failed with')) continue;
      const retry = cap.out.findIndex((l, j) => j > i && l.startsWith('Running the check'));
      assert.ok(retry > i, 'the holder retries after reporting the grouped failure');
      const beforeRetry = cap.out.slice(i, retry).join('\n');
      assert.match(beforeRetry, /exit 3/);
      assert.match(beforeRetry, /FAIL desktop core tests \/retained\/group\/desktop-core-tests.log/);
      assert.match(beforeRetry, /open_gate_source: hostname URI format failed/);
      assert.doesNotMatch(beforeRetry, /passing output 1\n/);
      assert.ok(beforeRetry.length < 8500, 'long lines cannot flood the grouped diagnostic');
      assert.ok(line.split('\n').length <= 43, 'header, edit precaution, blank line, and at most 40 output lines');
      assert.match(beforeRetry, /Wait for tb land to finish before editing this worktree/);
    }
  }
  // Every check that passed had no bad.txt in it: main only moved to those.
  // At most 7: a's, the group's, then halves. Which half goes first is
  // timing: if d lands from [c, d] first, main moves and the limits lapse,
  // so b and c are checked together again before c alone (a, abcd, cd, d,
  // bc, c, b). The slower cloud box took that order (14 Nov).
  const checks = fs.readFileSync(path.join(root, 'checks'), 'utf8').trim().split('\n');
  assert.ok(checks.length <= 7, checks.join('\n'));
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

test('the waiting line reads a completed result from disk before describing another check', () => {
  const { main } = makeRepo();
  const common = path.join(main, '.git');
  const times = path.join(common, 'threadbear-land-times.log');
  fs.writeFileSync(times, JSON.stringify({ ok: true, seconds: 360 }) + '\n');
  const now = Date.now();
  const me = joinQueue(common, { branch: 'thread/me', path: '/me', base: 'b', commit: 'c' });
  const completed = { ...me, result: { landed: 'landed-tip', commits: 'Add me', together: ['thread/holder'] } };
  writeEntry(common, completed);
  assert.equal(me.result, undefined, 'the caller still has its original queue entry');
  const expected = ': already landed with its group; waiting for the landing lock to finish up.';
  setRunning(common, { startedAt: now - 180_000, branches: ['thread/unrelated'], pid: process.pid, entries: ['another-entry'] });
  assert.equal(waitingLine(common, times, me, now), expected);
  setRunning(common, undefined);
  assert.equal(waitingLine(common, times, me, now), expected);
  leaveQueue(common, me.id);
  assert.equal(waitingLine(common, times, completed, now), expected, 'an already-read result also works');
});

test('the waiting line recognises a landing already in the running group', () => {
  const { main } = makeRepo();
  const common = path.join(main, '.git');
  const times = path.join(common, 'threadbear-land-times.log');
  fs.writeFileSync(times, JSON.stringify({ ok: true, seconds: 360 }) + '\n');
  const now = Date.now();
  joinQueue(common, { branch: 'thread/peer', path: '/peer', base: 'b', commit: 'c' });
  const me = joinQueue(common, { branch: 'thread/me', path: '/me', base: 'b', commit: 'c' });
  // The holder and any position in its group use the same remaining check.
  for (const branches of [['thread/me', 'thread/peer'], ['thread/peer', 'thread/me']]) {
    setRunning(common, { startedAt: now - 120_000, branches });
    assert.equal(waitingLine(common, times, me, now), ': this landing is being checked with 1 other landing; the check has been running 2 min, so roughly 4 min to go (a check takes about 6 min here).');
  }
  setRunning(common, { startedAt: now - 180_000, branches: ['thread/holder', me.branch, 'thread/peer', 'thread/another'] });
  assert.equal(waitingLine(common, times, me, now), ': this landing is being checked with 3 other landings; the check has been running 3 min, so roughly 3 min to go (a check takes about 6 min here).');
  setRunning(common, { startedAt: now - 600_000, branches: [me.branch] });
  assert.equal(waitingLine(common, times, me, now), ': this landing is being checked; the check has been running 10 min, longer than usual (a check takes about 6 min here).');
  fs.rmSync(times);
  assert.equal(waitingLine(common, times, me, now), ': this landing is being checked; the check has been running 10 min.');
});

test('the waiting line counts only landings outside the running group as ahead', () => {
  const { main } = makeRepo();
  const common = path.join(main, '.git');
  const times = path.join(common, 'threadbear-land-times.log');
  fs.writeFileSync(times, JSON.stringify({ ok: true, seconds: 360 }) + '\n');
  const now = Date.now();
  const active = joinQueue(common, { branch: 'thread/active', path: '/active', base: 'b', commit: 'c' });
  const ahead = joinQueue(common, { branch: 'thread/ahead', path: '/ahead', base: 'b', commit: 'c' });
  const me = joinQueue(common, { branch: 'thread/me', path: '/me', base: 'b', commit: 'c' });
  // Fix the ordering independently of the clock and random queue ids.
  me.at = Math.max(active.at, ahead.at, me.at) + 1;
  setRunning(common, { startedAt: now - 120_000, branches: ['thread/holder', active.branch] });
  assert.equal(waitingLine(common, times, me, now), ': the check for thread/holder and 1 more has been running 2 min; 1 more waiting ahead of this one, all checked together with it next, so roughly 10 min to go (a check takes about 6 min here).');
  // A caller without an entry (such as a dry run) is outside this check.
  assert.equal(waitingLine(common, times, undefined, now), ': the check for thread/holder and 1 more has been running 2 min; 2 more waiting ahead of this one, all checked together with it next, so roughly 10 min to go (a check takes about 6 min here).');
});

test('the waiting line distinguishes a new entry on a branch already being checked', () => {
  const { main } = makeRepo();
  const common = path.join(main, '.git');
  const times = path.join(common, 'threadbear-land-times.log');
  fs.writeFileSync(times, JSON.stringify({ ok: true, seconds: 360 }) + '\n');
  const now = Date.now();
  const me = joinQueue(common, { branch: 'thread/me', path: '/me', base: 'b', commit: 'new' });
  setRunning(common, { startedAt: now - 120_000, branches: ['thread/holder', me.branch], pid: process.pid, entries: ['holder', 'old-entry'] });
  assert.equal(waitingLine(common, times, me, now), ': the check for thread/holder and 1 more has been running 2 min; this one is checked next, so roughly 10 min to go (a check takes about 6 min here).');
  setRunning(common, { startedAt: now - 120_000, branches: ['thread/holder', me.branch], pid: process.pid, entries: ['holder', me.id] });
  assert.equal(waitingLine(common, times, me, now), ': this landing is being checked with 1 other landing; the check has been running 2 min, so roughly 4 min to go (a check takes about 6 min here).');
});

test('the waiting line ignores a running record left by a dead holder', () => {
  const { main } = makeRepo();
  const common = path.join(main, '.git');
  const me = joinQueue(common, { branch: 'thread/me', path: '/me', base: 'b', commit: 'c' });
  setRunning(common, { startedAt: Date.now() - 45 * 60_000, branches: ['thread/dead', me.branch], pid: 2 ** 22 + 12345, entries: [me.id] });
  assert.equal(waitingLine(common, path.join(common, 'no-times'), me), '...');
  assert.ok(fs.existsSync(path.join(common, 'threadbear-land-running.json')), 'reading status does not change the record');
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
  fs.writeFileSync(path.join(gitDir, 'threadbear-land-checking.json'), JSON.stringify({ branch: a.branch, own, top: sh(main, 'git rev-parse main') }));
  sh(a.path, 'git checkout -q --detach main');
  const c = capture();
  assert.equal(await land({ ...a, check: 'true' }, c.opts), 0, c.err.join('\n'));
  assert.match(c.out[0], /checked out again as it was/);
  assert.equal(sh(main, 'git rev-parse main'), own);
  assert.ok(!fs.existsSync(path.join(gitDir, 'threadbear-land-checking.json')));
});

// 13 Nov: an agent committed its fix on the detached group while its tb
// land still had it, stopped that tb and ran it again, and the checkout of
// its branch left the fix on no branch, silently.
function stackedOn(a: LandTarget): { own: string; top: string; gitDir: string } {
  const own = sh(a.path, 'git rev-parse HEAD');
  const gitDir = sh(a.path, 'git rev-parse --absolute-git-dir');
  // Another's unlanded work on top, as a group check stacks it.
  sh(a.path, 'git checkout -q --detach');
  commit(a.path, { 'other.txt': 'theirs\n' }, "Another's work");
  const top = sh(a.path, 'git rev-parse HEAD');
  fs.writeFileSync(path.join(gitDir, 'threadbear-land-checking.json'), JSON.stringify({ branch: a.branch, own, top }));
  return { own, top, gitDir };
}

test('commits made on the group a killed tb land left detached are carried onto the branch and land', async () => {
  const { main } = makeRepo();
  const [a] = branches(main, ['a']);
  const { gitDir } = stackedOn(a);
  commit(a.path, { 'a.txt': 'fixed\n' }, 'Fix a');
  const fix = sh(a.path, 'git rev-parse --short HEAD');
  const c = capture();
  assert.equal(await land({ ...a, check: 'true' }, c.opts), 0, c.err.join('\n'));
  assert.match(c.out.join('\n'), new RegExp(`moved onto ${a.branch}[^]*${fix} Fix a`));
  assert.equal(fs.readFileSync(path.join(main, 'a.txt'), 'utf8'), 'fixed\n');
  assert.ok(!fs.existsSync(path.join(main, 'other.txt')), "the other's work isn't landed with it");
  assert.equal(sh(a.path, 'git rev-parse --abbrev-ref HEAD'), a.branch);
  assert.ok(!fs.existsSync(path.join(gitDir, 'threadbear-land-checking.json')));
});

test("commits on the detached group that won't go onto the branch stop tb land, with how to keep them", async () => {
  const { main } = makeRepo();
  const [a] = branches(main, ['a']);
  const { own, top, gitDir } = stackedOn(a);
  // Changes a file only the other's work has.
  commit(a.path, { 'other.txt': 'mine\n' }, 'Fix theirs');
  const head = sh(a.path, 'git rev-parse HEAD');
  const before = sh(main, 'git rev-parse main');
  const c = capture();
  assert.equal(await land({ ...a, check: 'true' }, c.opts), 1);
  const said = c.err.join('\n');
  assert.match(said, /Fix theirs/);
  assert.match(said, new RegExp(`git cherry-pick ${top.slice(0, 9)}\\.\\.${head.slice(0, 9)}`));
  assert.equal(sh(a.path, 'git rev-parse HEAD'), head, 'HEAD stays on the commits');
  assert.equal(sh(a.path, `git rev-parse ${a.branch}`), own);
  assert.equal(sh(main, 'git rev-parse main'), before);
  assert.ok(fs.existsSync(path.join(gitDir, 'threadbear-land-checking.json')), 'kept, so the next tb land asks again');
});

test('a commit made in the holder while its group check runs is carried onto its branch, not dropped', async () => {
  const { root, main } = makeRepo();
  const [a, b, c] = branches(main, ['a', 'b', 'c']);
  // The group (b and c together) fails, after saying where it runs.
  const check = `${slowCheck(root)}; if test -f b.txt && test -f c.txt; then pwd > '${root}/holder'; ` +
    `${holdUntil(`${root}/group-release`)}; exit 2; fi`;
  const first = land({ ...a, check }, capture().opts);
  await until(() => fs.existsSync(path.join(root, 'checking')), "a's check");
  const caps = [capture(), capture()];
  const rest = [b, c].map((t, i) => land({ ...t, check }, caps[i].opts));
  let codes: number[];
  try {
    await until(() => caps.every((x) => x.out.some((l) => l.startsWith('Waiting for another landing'))), 'b and c waiting');
    write(root, { release: '' });
    assert.equal(await first, 0);
    await until(() => fs.existsSync(path.join(root, 'holder')), 'the group check');
    const holder = fs.realpathSync(fs.readFileSync(path.join(root, 'holder'), 'utf8').trim());
    commit(holder, { 'fix.txt': 'fix\n' }, 'Fix during the check');
  } finally {
    write(root, { 'group-release': '' });
    codes = await Promise.all(rest);
  }
  const i = [b, c].findIndex((t) => sh(t.path, 'git log --format=%s -1') === 'Fix during the check');
  assert.ok(i >= 0, 'the fix is on the holder\'s branch');
  const h = [b, c][i];
  assert.equal(codes[i], 1);
  assert.match(caps[i].err.join('\n'), /Commits were made in .* while it was being checked, so nothing landed; they're on/);
  assert.equal(sh(h.path, 'git rev-parse --abbrev-ref HEAD'), h.branch);
  assert.equal(sh(h.path, `git log --format=%s main..${h.branch}`).split('\n').at(-1), `Add ${i ? 'c' : 'b'}`, 'only its own commits under the fix');
  assert.ok(!fs.existsSync(path.join(main, 'fix.txt')));
});

test('a commit made while the carried commits are picked stops the put-back rather than drop it', async () => {
  const { main } = makeRepo();
  const [a] = branches(main, ['a']);
  stackedOn(a);
  commit(a.path, { 'a.txt': 'fixed\n' }, 'Fix a');
  // The second fix lands on HEAD as the first is being carried.
  const once = path.join(a.path, '..', `once-${path.basename(a.path)}`);
  const restore = wrapGit('commit-tree', `if [ ! -e '${once}' ]; then touch '${once}'; echo two > '${a.path}/two.txt'; "$REAL" -C '${a.path}' add two.txt; "$REAL" -C '${a.path}' commit -qm 'Fix two'; fi`);
  const c = capture();
  try {
    assert.equal(await land({ ...a, check: 'true' }, c.opts), 1);
  } finally {
    restore();
  }
  assert.match(c.err.join('\n'), /moved to .* while tb land was putting/);
  assert.equal(sh(a.path, 'git log --format=%s -1 HEAD'), 'Fix two', 'HEAD left on the new commit');
  assert.equal(sh(a.path, `git log --format=%s -1 ${a.branch}`), 'Fix a', 'the first fix is on the branch');
  // The next tb land carries the second one too, and lands both.
  const again = capture();
  assert.equal(await land({ ...a, check: 'true' }, again.opts), 0, again.err.join('\n'));
  assert.match(again.out.join('\n'), /Fix two/);
  assert.equal(sh(main, 'git log --format=%s -3 main'), 'Fix two\nFix a\nAdd a');
  assert.ok(!fs.existsSync(path.join(main, 'other.txt')));
});

test('a marker from before the stack was written down never checks out over HEAD', async () => {
  const { main } = makeRepo();
  const [a] = branches(main, ['a']);
  const { own, gitDir } = stackedOn(a);
  fs.writeFileSync(path.join(gitDir, 'threadbear-land-checking.json'), JSON.stringify({ branch: a.branch, own }));
  commit(a.path, { 'a.txt': 'fixed\n' }, 'Fix a');
  const head = sh(a.path, 'git rev-parse HEAD');
  const c = capture();
  assert.equal(await land({ ...a, check: 'true' }, c.opts), 1);
  assert.match(c.err.join('\n'), /can't tell whether commits were made on it/);
  assert.equal(sh(a.path, 'git rev-parse HEAD'), head);
  assert.ok(fs.existsSync(path.join(gitDir, 'threadbear-land-checking.json')));
});

test('a commit made in the holder after its group check passed, before main moves, is kept on its branch', async () => {
  const { root, main } = makeRepo();
  const [a, b, c] = branches(main, ['a', 'b', 'c']);
  const check = `${slowCheck(root)}; if test -f b.txt && test -f c.txt; then touch '${root}/group-passed'; fi`;
  // Asked once more after the check: the agent commits just then.
  let committed = '';
  const hold = async (cwd: string) => {
    if (!committed && fs.existsSync(path.join(root, 'group-passed'))) {
      commit(cwd, { 'late.txt': 'late\n' }, 'Late fix');
      committed = cwd;
    }
    return undefined;
  };
  const first = land({ ...a, check }, capture().opts);
  await until(() => fs.existsSync(path.join(root, 'checking')), "a's check");
  const caps = [capture(), capture()];
  const rest = [b, c].map((t, i) => land({ ...t, check }, { ...caps[i].opts, hold }));
  try {
    await until(() => caps.every((x) => x.out.some((l) => l.startsWith('Waiting for another landing'))), 'b and c waiting');
  } finally {
    write(root, { release: '' });
  }
  assert.equal(await first, 0);
  assert.deepEqual(await Promise.all(rest), [0, 0], caps.map((x) => x.err.join('\n')).join('\n'));
  assert.ok(committed, 'the hold ran after the group check');
  const h = [b, c].find((t) => fs.realpathSync(t.path) === fs.realpathSync(committed))!;
  for (const n of ['b', 'c']) assert.ok(fs.existsSync(path.join(main, `${n}.txt`)));
  assert.ok(!fs.existsSync(path.join(main, 'late.txt')));
  assert.equal(sh(h.path, 'git rev-parse --abbrev-ref HEAD'), h.branch);
  assert.equal(sh(h.path, `git log --format=%s main..${h.branch}`), 'Late fix');
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
// c is first in the stack, so whether it lands as new commits or as it was
// came down to the clock: rebased within the same second, the commits are the
// same ones. Both ways are run, with git's commit date pinned, and moved on
// a second before the landing for the first. (13 Nov: a note from c's commit
// to itself made c's next tb land say it moved the branch, about half the
// runs on GitHub's Linux.)
for (const sameSecond of [false, true]) test(`a landing whose tb is killed during the group check still lands, if its branch is where it was${sameSecond ? ' (landed as it was)' : ''}`, async () => {
  const pinned = process.env.GIT_COMMITTER_DATE;
  process.env.GIT_COMMITTER_DATE = '2026-11-13T00:00:00Z';
  try {
    await killedDuringGroupCheck(sameSecond);
  } finally {
    if (pinned === undefined) delete process.env.GIT_COMMITTER_DATE;
    else process.env.GIT_COMMITTER_DATE = pinned;
  }
});

async function killedDuringGroupCheck(sameSecond: boolean) {
  const { root, main } = makeRepo();
  const [b, c] = branches(main, ['b', 'c']);
  const common = path.join(main, '.git');
  // c's tb, waiting: an entry, and its lock held by a stand-in that goes
  // once the check has started.
  const e = joinQueue(common, { branch: c.branch, path: c.path, base: sh(main, 'git rev-parse main'), commit: 'later', check: 'CHECK', checkFrom: '' });
  // An empty commit on purpose too: once landed for it, its branch has to
  // be moved, or its next tb land would land that one again.
  sh(c.path, 'git commit -q --allow-empty -m "Empty on c"');
  const cmd = `touch '${root}/held'; ${holdUntil(`${root}/drop`)}`;
  const lockFile = path.join(queueDir(common), `${e.id}.lock`);
  // Says it's gone once the lock is let go, which the check waits for.
  const wrapper = process.platform === 'darwin'
    ? `/usr/bin/lockf -k '${lockFile}' /bin/sh -c "${cmd}"; touch '${root}/released'`
    : `flock -x '${lockFile}' /bin/sh -c "${cmd}"; touch '${root}/released'`;
  const holder = spawn('/bin/sh', ['-c', wrapper], { stdio: 'ignore' });
  holder.on('error', () => {});
  const gone = new Promise((r) => holder.once('close', r));
  await until(() => fs.existsSync(path.join(root, 'held')), "c's stand-in holding its lock");
  const check = `if test -f c.txt; then touch '${root}/drop'; ${holdUntil(`${root}/released`)}; fi`;
  // The entry asks for the same check as b's, for c's commits as they are.
  fs.writeFileSync(path.join(queueDir(common), `${e.id}.json`), JSON.stringify({ ...e, commit: sh(c.path, 'git rev-parse HEAD'), check }));
  const asked = sh(c.path, 'git rev-parse HEAD');
  if (!sameSecond) process.env.GIT_COMMITTER_DATE = '2026-11-13T00:00:01Z';
  const out = capture();
  assert.equal(await land({ ...b, check }, out.opts), 0, out.err.join('\n'));
  await gone;
  assert.ok(fs.existsSync(path.join(main, 'b.txt')) && fs.existsSync(path.join(main, 'c.txt')), out.out.join('\n'));
  assert.ok(out.out.some((l) => /In the same check, and landed with it: thread\/c0000000/.test(l)));
  // c's branch is on what landed, so its next tb land has nothing to land.
  // Whether b's landing moves c's branch there or this one does depends on
  // when c's stand-in lets go of its lock, so this one may say it moved it
  // first (tomredf's fork, 12 Nov).
  const again = capture();
  assert.equal(await land({ ...c, check: 'true' }, again.opts), 0);
  const moved = `An earlier tb land of ${c.branch} landed while its tb was stopped; the branch is moved to what landed.`;
  assert.deepEqual(again.out.filter((l) => l !== moved), [`Nothing to land: ${c.branch} has no commits that aren't on main.`]);
  assert.equal(sh(main, 'git log --format=%s main | grep -c "Empty on c"'), '1');
  // Each case really happened: c's commits landed as they were (nothing to
  // move, so no note), or as new ones.
  const asIs = sh(main, `git merge-base --is-ancestor ${asked} main && echo yes || echo no`);
  assert.equal(asIs, sameSecond ? 'yes' : 'no');
  const notes = path.join(common, 'threadbear-land-done');
  if (sameSecond) assert.deepEqual(fs.existsSync(notes) ? fs.readdirSync(notes) : [], [], 'no note from a commit to itself');
}

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
  // branch leaves it): its commits on main, after another's, so they're
  // new commits and not, within the same second, the very same ones.
  sh(main, 'git commit -q --allow-empty -m "Landed before c"');
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

test("a group's check hears the hosts each landing in it asked for, the holder's first", async () => {
  // On 13 Nov a landing that asked for the cloud box was checked on Tinker
  // Desk in another thread's group, as the check ran with the holder's
  // environment only.
  const { root, main } = makeRepo();
  const [a, b, c, d] = branches(main, ['a', 'b', 'c', 'd']);
  const check = `${slowCheck(root)}; echo "prefer=\${THREADBEAR_CHECK_PREFER-unset} group=\${THREADBEAR_CHECK_GROUP-unset}" >> '${root}/prefers'`;
  const first = land({ ...a, check }, { ...capture().opts, checkHost: 'tinker-desk' });
  await until(() => fs.existsSync(path.join(root, 'checking')), "a's check");
  const caps = [capture(), capture(), capture()];
  // b asks for cloud, c for nothing, d for something no ssh host is called.
  const rest = [
    land({ ...b, check }, { ...caps[0].opts, checkHost: 'cloud' }),
    land({ ...c, check }, { ...caps[1].opts, checkHost: '' }),
    land({ ...d, check }, { ...caps[2].opts, checkHost: '-oProxyCommand=x' }),
  ];
  try {
    await until(() => caps.every((cap) => cap.out.some((l) => l.startsWith('Waiting for another landing'))), 'the others waiting');
  } finally {
    write(root, { release: '' });
  }
  assert.equal(await first, 0);
  assert.deepEqual(await Promise.all(rest), [0, 0, 0], caps.map((cap) => cap.err.join('\n')).join('\n'));
  // a by itself hears nothing; the group's holder (b, c or d) passes on
  // cloud, and its own, if it's b, only once.
  const lines = fs.readFileSync(path.join(root, 'prefers'), 'utf8').trim().split('\n');
  assert.deepEqual(lines, ['prefer=unset group=unset', 'prefer=cloud group=1']);
});

// 13 Nov: main was broken (a compile error only checked when its folder
// changed), so a group was halved down three checks, each failing on main's
// own error, while the one-line repair (tb land --despite-red) waited for a
// group of its own.
async function brokenMainQueue(fixBreaks: boolean) {
  const { root, main } = makeRepo();
  const [a, b, c, f] = branches(main, ['a', 'b', 'c', 'f']);
  // f's commit is the repair: fix.txt. Without it, any change to b or c
  // fails on main's own error.
  commit(f.path, { 'fix.txt': 'fixed\n' }, 'Repair main');
  // a's own check fails too, so main stays where the limits below were set.
  const check = slowCheck(root) + `; if (test -f b.txt || test -f c.txt) && ! test -f fix.txt; then exit 1; fi` +
    `; if test -f a.txt && ! test -f b.txt && ! test -f c.txt && ! test -f fix.txt; then exit 1; fi` +
    (fixBreaks ? '; ! test -f fix.txt' : '');
  const first = land({ ...a, check }, capture().opts);
  await until(() => fs.existsSync(path.join(root, 'checking')), "a's check");
  const caps = [capture(), capture(), capture()];
  const rest = [land({ ...b, check }, caps[0].opts), land({ ...c, check }, caps[1].opts)];
  await until(() => caps.slice(0, 2).every((x) => x.out.some((l) => l.startsWith('Waiting for another landing'))), 'b and c waiting');
  // As after earlier failed group checks: each to be checked alone.
  const common = path.join(main, '.git');
  const at = sh(main, 'git rev-parse main');
  for (const e of waiting(common)) fs.writeFileSync(path.join(queueDir(common), `${e.id}.json`), JSON.stringify({ ...e, limit: 1, limitAt: at }));
  rest.push(land({ ...f, check }, { ...caps[2].opts, despiteRed: true }));
  try {
    await until(() => caps[2].out.some((l) => l.startsWith('Waiting for another landing')), 'the fix waiting');
  } finally {
    write(root, { release: '' });
  }
  assert.equal(await first, 1);
  return { main, root, codes: await Promise.all(rest), caps };
}

test('a repair landed despite red goes into the next check with everyone held back by halving, so they land with it', async () => {
  const { main, codes, caps } = await brokenMainQueue(false);
  assert.deepEqual(codes, [0, 0, 0], caps.map((x) => x.out.concat(x.err).join('\n')).join('\n---\n'));
  for (const n of ['b', 'c', 'fix']) assert.ok(fs.existsSync(path.join(main, `${n}.txt`)), n);
});

test('a "fix" that itself breaks the check is left out in the end, and the rest land', async () => {
  const { main, codes, caps } = await brokenMainQueue(true);
  // b and c fail on main's error once the fix is out (it's broken too), and
  // the fix fails its own check: nothing lands, and nothing loops for ever.
  assert.equal(codes[2], 1, caps[2].out.concat(caps[2].err).join('\n'));
  assert.ok(!fs.existsSync(path.join(main, 'fix.txt')));
  assert.deepEqual(codes.slice(0, 2), [1, 1]);
});

test("an ordinary landing doesn't get past the red hold by riding with a fix that moved its file into a held folder", async () => {
  const { root, main } = makeRepo();
  commit(main, { 'shared.txt': 'one\ntwo\nthree\n' }, 'Shared');
  const [a, n, f] = branches(main, ['a', 'n', 'f']);
  commit(n.path, { 'shared.txt': 'one\nTWO by n\nthree\n' }, 'Edit shared');
  sh(f.path, 'mkdir -p guard && git mv shared.txt guard/shared.txt && git commit -qm "Move shared into guard"');
  const hold = async (_r: string, files: string[]) => (files.some((x) => x.startsWith('guard/')) ? 'Held: guard is red on main.' : undefined);
  const check = slowCheck(root);
  const first = land({ ...a, check }, capture().opts);
  await until(() => fs.existsSync(path.join(root, 'checking')), "a's check");
  const cn = capture();
  const cf = capture();
  const rest = [land({ ...n, check }, { ...cn.opts, hold }), land({ ...f, check }, { ...cf.opts, hold, despiteRed: true })];
  try {
    await until(() => [cn, cf].every((x) => x.out.some((l) => l.startsWith('Waiting for another landing'))), 'both waiting');
  } finally {
    write(root, { release: '' });
  }
  assert.equal(await first, 0);
  const [nCode, fCode] = await Promise.all(rest);
  assert.equal(fCode, 0, cf.out.concat(cf.err).join('\n'));
  if (nCode === 0) {
    // Landed, then only as an edit outside the held folder.
    assert.match(sh(main, 'git show --name-only --format= "main^{/Edit shared}"'), /^shared\.txt$/);
  } else {
    assert.match(cn.err.join('\n'), /Held: guard is red on main\./);
  }
});

test('limits from a failed group check lapse once main has moved', async () => {
  const { root, main } = makeRepo();
  const [a, b, c] = branches(main, ['a', 'b', 'c']);
  const check = slowCheck(root);
  const first = land({ ...a, check }, capture().opts);
  await until(() => fs.existsSync(path.join(root, 'checking')), "a's check");
  const caps = [capture(), capture()];
  const rest = [land({ ...b, check }, caps[0].opts), land({ ...c, check }, caps[1].opts)];
  await until(() => caps.every((x) => x.out.some((l) => l.startsWith('Waiting for another landing'))), 'b and c waiting');
  // Set on the main before a lands: a lands, so they no longer hold.
  const common = path.join(main, '.git');
  const at = sh(main, 'git rev-parse main');
  for (const e of waiting(common)) fs.writeFileSync(path.join(queueDir(common), `${e.id}.json`), JSON.stringify({ ...e, limit: 1, limitAt: at }));
  write(root, { release: '' });
  assert.equal(await first, 0);
  assert.deepEqual(await Promise.all(rest), [0, 0]);
  assert.equal(fs.readFileSync(path.join(root, 'checks'), 'utf8').trim().split('\n').length, 2, 'b and c in one check');
});
