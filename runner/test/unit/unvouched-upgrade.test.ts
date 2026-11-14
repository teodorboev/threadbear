// Worktrees a move held from the always-trust, across upgrades (security
// review, 11 Nov 2026, findings 90 and 91). The list of them moved twice on
// 10 Nov 2026 (a mark on the thread's worktree record, then an array in
// patch-trust.json, then patch-trust-unvouched.json) without bringing its
// old forms, so an upgrade trusted again a moved worktree's patches, which
// a compromised Mac had written, and imported them in the runner.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { execFileSync } from 'node:child_process';
import { makeHarness, tick, tmpDir } from '../helpers/fakes.ts';
import { PatchTrust } from '../../src/patch-trust.ts';
import { eventsSayMovedIn } from '../../src/runner/moves.ts';

const g = (cwd: string, ...a: string[]) => execFileSync('git', ['-C', cwd, '-c', 'user.name=t', '-c', 'user.email=t@t', ...a], { encoding: 'utf8' }).trim();

// Threadbear's own checkout (always trusted) with a linked worktree whose
// patch is another Mac's.
function checkout() {
  const root = fs.realpathSync(tmpDir('tb-unvouched-'));
  const repo = path.join(root, 'threadbear');
  fs.mkdirSync(repo);
  g(repo, 'init', '-q', '-b', 'main');
  fs.writeFileSync(path.join(repo, 'a.txt'), 'a\n');
  g(repo, 'add', '-A');
  g(repo, 'commit', '-qm', 'a');
  const wt = path.join(root, 'slot-1');
  g(repo, 'worktree', 'add', '-q', '-b', 'thread/abcd1234', wt, 'main');
  const patch = path.join(wt, '.threadbear', 'patches', 'evil.ts');
  fs.mkdirSync(path.dirname(patch), { recursive: true });
  fs.writeFileSync(patch, "import fs from 'node:fs'; fs.writeFileSync('/tmp/x', 'ran'); export default {};\n");
  const home = path.join(root, 'home');
  fs.mkdirSync(home);
  const worktree = { repo: 'threadbear', mainCheckout: repo, path: wt, branch: 'thread/abcd1234', base: g(repo, 'rev-parse', 'main') };
  return { root, repo, wt, patch, home, worktree, file: path.join(home, 'patch-trust.json'), side: path.join(home, 'patch-trust-unvouched.json') };
}

// A runner with a thread in the worktree, its record then edited as an
// older runner left it, and started again over the trust files.
async function runnerWith(c: ReturnType<typeof checkout>, edit: (rec: any) => void, events?: (file: string) => void) {
  const first = makeHarness(c.home);
  const { thread } = first.runner.create({ text: 'hi', accountId: 'claude-3', cwd: c.wt });
  await tick();
  first.adapter.last.init();
  first.adapter.last.result('completed');
  await tick();
  const recFile = path.join(c.home, 'threads', thread.id, 'thread.json');
  const rec = JSON.parse(fs.readFileSync(recFile, 'utf8'));
  edit(rec);
  fs.writeFileSync(recFile, JSON.stringify(rec, null, 2));
  events?.(path.join(c.home, 'threads', thread.id, 'events.ndjson'));
  const trust = new PatchTrust({ file: c.file, always: [c.repo] });
  const h = makeHarness(c.home, {}, { patchTrust: trust });
  return { h, trust, id: thread.id };
}

const sideJson = (c: ReturnType<typeof checkout>) => JSON.parse(fs.readFileSync(c.side, 'utf8'));

test("90: a worktree an older runner marked on the thread's record stays held after the upgrade", async () => {
  const c = checkout();
  // A trust file from then, without any list.
  fs.writeFileSync(c.file, JSON.stringify({ version: 3, folders: {} }));
  const { trust } = await runnerWith(c, (rec) => (rec.worktree = { ...c.worktree, unvouched: true }));
  assert.equal((await trust.check(c.patch, 0)).trusted, false, 'its patch waits for trust');
  assert.deepEqual(sideJson(c), { version: 2, unvouched: [c.wt] });
  assert.equal((await trust.check(path.join(c.repo, '.threadbear', 'patches', 'w.ts'), 0)).trusted, true, 'the checkout itself still is');
  // And after the next start, from the file alone.
  assert.equal((await new PatchTrust({ file: c.file, always: [c.repo] }).check(c.patch, 0)).trusted, false);
});

test("91: patch-trust.json's own list (the next form) is taken in, before anything is checked", async () => {
  const c = checkout();
  fs.writeFileSync(c.file, JSON.stringify({ version: 3, folders: {}, unvouched: [c.wt] }));
  const trust = new PatchTrust({ file: c.file, always: [c.repo] });
  assert.equal((await trust.check(c.patch, 0)).trusted, false);
  assert.ok(trust.isUnvouched(c.patch));
  assert.deepEqual(sideJson(c).unvouched, [c.wt]);
  // Still held once the trust file is written without it, as it now is.
  trust.save();
  assert.equal(JSON.parse(fs.readFileSync(c.file, 'utf8')).unvouched, undefined);
  assert.equal((await new PatchTrust({ file: c.file, always: [c.repo] }).check(c.patch, 0)).trusted, false, 'after a restart');
  // A worktree made there afresh (vouch) clears it from both, for good.
  fs.writeFileSync(c.file, JSON.stringify({ version: 3, folders: {}, unvouched: [c.wt] }));
  const again = new PatchTrust({ file: c.file, always: [c.repo] });
  again.vouch(c.wt);
  assert.equal((await again.check(c.patch, 0)).trusted, true);
  assert.equal((await new PatchTrust({ file: c.file, always: [c.repo] }).check(c.patch, 0)).trusted, true, 'and stays so');
});

test('90/91: a move finished before any mark is held, as nothing can say what it brought', async () => {
  const c = checkout();
  fs.writeFileSync(c.file, JSON.stringify({ version: 3, folders: {} }));
  const { trust } = await runnerWith(
    c,
    (rec) => (rec.worktree = { ...c.worktree }),
    (file) => {
      const n = fs.readFileSync(file, 'utf8').split('\n').filter(Boolean).length;
      fs.appendFileSync(file, JSON.stringify({ seq: n + 1, at: new Date().toISOString(), type: 'system.note', text: 'Moved from Bruce to M5, in /x.' }) + '\n');
    },
  );
  assert.equal((await trust.check(c.patch, 0)).trusted, false);
});

test("90/91: this runner's own mark of a moved worktree brings it back if the list is lost", async () => {
  const c = checkout();
  fs.writeFileSync(c.file, JSON.stringify({ version: 3, folders: {} }));
  fs.writeFileSync(c.side, JSON.stringify({ version: 2, unvouched: [] }));
  // Known good: a clean move isn't held.
  const known = await runnerWith(c, (rec) => (rec.worktree = { ...c.worktree, movedIn: true }));
  assert.equal((await known.trust.check(c.patch, 0)).trusted, true, 'a clean move, with the list known');
  // Lost: held until trusted.
  fs.rmSync(c.side);
  const trust = new PatchTrust({ file: c.file, always: [c.repo] });
  makeHarness(c.home, {}, { patchTrust: trust });
  assert.equal((await trust.check(c.patch, 0)).trusted, false, 'the list lost');
});

test('90/91: a list that can not be read holds every worktree of an always-trusted checkout, not the checkout', async () => {
  const c = checkout();
  fs.writeFileSync(c.file, JSON.stringify({ version: 3, folders: {} }));
  fs.writeFileSync(c.side, '{ not json');
  const trust = new PatchTrust({ file: c.file, always: [c.repo] });
  assert.equal(trust.unvouchedKnown(), false);
  assert.equal((await trust.check(c.patch, 0)).trusted, false);
  assert.equal((await trust.check(path.join(c.repo, '.threadbear', 'patches', 'w.ts'), 0)).trusted, true);
});

test("90/91: an ordinary worktree of this runner's keeps the always-trust through the upgrade", async () => {
  const c = checkout();
  fs.writeFileSync(c.file, JSON.stringify({ version: 3, folders: {} }));
  const { trust } = await runnerWith(c, (rec) => (rec.worktree = { ...c.worktree }));
  assert.equal((await trust.check(c.patch, 0)).trusted, true);
  assert.deepEqual(sideJson(c), { version: 2, unvouched: [] });
});

test("90/91: a first start's seeding doesn't trust a held worktree's patches", async () => {
  const c = checkout();
  // No trust file at all: the first start seeds from what threads run.
  const { trust } = await runnerWith(c, (rec) => (rec.worktree = { ...c.worktree, unvouched: true }));
  assert.equal((await trust.check(c.patch, 0)).trusted, false);
});

test('a log says a thread was moved here only with the note adoptMoved writes', () => {
  const dir = tmpDir();
  assert.equal(eventsSayMovedIn(dir), false, 'no log');
  const file = path.join(dir, 'events.ndjson');
  fs.writeFileSync(file, JSON.stringify({ seq: 1, type: 'user.message', text: 'Moved to /x' }) + '\n');
  assert.equal(eventsSayMovedIn(dir), false);
  // Past a read's piece, across its edge.
  fs.appendFileSync(file, 'x'.repeat((1 << 20) - 10) + '\n' + JSON.stringify({ seq: 2, type: 'system.note', text: 'Moved from Bruce to M5.' }) + '\n');
  assert.equal(eventsSayMovedIn(dir), true);
});

// Finding 96's other half: whatever trips one thread's recovery at start
// costs that thread its recovery, not the runner.
test("one thread whose recovery throws doesn't stop the runner starting, and is said", async () => {
  const home = tmpDir();
  const h = makeHarness(home);
  const a = h.runner.create({ text: 'a', accountId: 'claude-3', cwd: tmpDir() }).thread.id;
  const b = h.runner.create({ text: 'b', accountId: 'claude-3', cwd: tmpDir() }).thread.id;
  await tick();
  const { Runner } = await import('../../src/runner.ts');
  const recover = Runner.prototype.recover;
  Runner.prototype.recover = function (this: any, st: any) {
    if (st.rec.id === a) throw new TypeError('object is not iterable');
    return recover.call(this, st);
  };
  try {
    const again = makeHarness(home);
    assert.ok(again.runner.threads.has(a) && again.runner.threads.has(b));
    assert.ok(again.runner.startupNotices.some((n) => n.level === 'warning' && n.message.includes("couldn't be picked up")));
  } finally {
    Runner.prototype.recover = recover;
  }
});

// Muse's review of the fix: a thread handed over while the new runner is
// still connecting to the old one is adopted before start, so before the
// list was worked out; and a record that can't be read was rebuilt without
// its worktree before the list was worked out, which then lost it for good.
test('90/91: a handover before start, and a record that can not be read, still hold a moved worktree', async () => {
  {
    const c = checkout();
    fs.writeFileSync(c.file, JSON.stringify({ version: 3, folders: {} }));
    const { h, trust, id } = await runnerWith(c, (rec) => (rec.worktree = { ...c.worktree }));
    assert.equal((await trust.check(c.patch, 0)).trusted, true, 'not moved: trusted');
    // Another thread, moved here before any mark, on disk but not loaded,
    // and the list lost: as a runner adopting it mid-connect would find it.
    const other = '0123abcd-0000-4000-8000-000000000001';
    fs.cpSync(path.join(c.home, 'threads', id), path.join(c.home, 'threads', other), { recursive: true });
    const recFile = path.join(c.home, 'threads', other, 'thread.json');
    fs.writeFileSync(recFile, JSON.stringify({ ...JSON.parse(fs.readFileSync(recFile, 'utf8')), id: other }));
    const n = fs.readFileSync(path.join(c.home, 'threads', other, 'events.ndjson'), 'utf8').split('\n').filter(Boolean).length;
    fs.appendFileSync(path.join(c.home, 'threads', other, 'events.ndjson'), JSON.stringify({ seq: n + 1, type: 'system.note', text: `Moved from Bruce to M5, in ${c.wt}.` }) + '\n');
    fs.rmSync(c.side);
    const fresh = new PatchTrust({ file: c.file, always: [c.repo] });
    (h.runner as any).patchTrust = fresh;
    h.runner.unvouchedSettled = false;
    h.runner.adopt(other);
    assert.equal((await fresh.check(c.patch, 0)).trusted, false, 'held before the adopted thread could load anything');
  }
  {
    const c = checkout();
    fs.writeFileSync(c.file, JSON.stringify({ version: 3, folders: {} }));
    const { id } = await runnerWith(c, (rec) => (rec.worktree = { ...c.worktree }));
    const dir = path.join(c.home, 'threads', id);
    const n = fs.readFileSync(path.join(dir, 'events.ndjson'), 'utf8').split('\n').filter(Boolean).length;
    fs.appendFileSync(path.join(dir, 'events.ndjson'), JSON.stringify({ seq: n + 1, type: 'system.note', text: `Moved from Bruce to M5, in ${path.join(c.wt, 'sub')}. Its worktree's work changes this repo's patches.` }) + '\n');
    fs.writeFileSync(path.join(dir, 'thread.json'), '{ broken');
    fs.rmSync(c.side);
    const trust = new PatchTrust({ file: c.file, always: [c.repo] });
    makeHarness(c.home, {}, { patchTrust: trust });
    assert.equal((await trust.check(c.patch, 0)).trusted, false, 'held from its log, though its record was rebuilt');
    assert.equal((await trust.check(path.join(c.repo, '.threadbear', 'patches', 'w.ts'), 0)).trusted, true, 'a note can only hold a worktree, never the checkout');
  }
});

// Muse's second look: adopt rebuilt an unreadable record before working
// the list out, and the note's folder could be hidden by the other
// runner's name (its own word) or by a ". " in the folder's path.
test("90/91: a note's folder is found whatever the names around it say, and only a worktree is held", async () => {
  const { movedInWorktrees } = await import('../../src/runner/moves.ts');
  const c = checkout();
  const odd = path.join(c.root, 'My App. backup');
  fs.mkdirSync(odd);
  g(c.repo, 'worktree', 'add', '-q', '-b', 'thread/odd', path.join(odd, 'slot-2'), 'main');
  const dir = tmpDir();
  const lines = [
    { type: 'system.note', text: `Moved from Bruce, in /nonexistent. to M5, in ${c.wt}. Its worktree's work changes this repo's patches.` },
    { type: 'system.note', text: `Moved from Bruce to M5, in ${path.join(odd, 'slot-2', 'sub')}.` },
    { type: 'system.note', text: `Moved from Bruce to M5, in ${c.repo}.` },
    { type: 'user.message', text: `Moved from Bruce to M5, in ${c.root}.` },
  ];
  fs.mkdirSync(path.join(c.wt, 'sub'), { recursive: true });
  fs.mkdirSync(path.join(odd, 'slot-2', 'sub'), { recursive: true });
  fs.writeFileSync(path.join(dir, 'events.ndjson'), lines.map((l, i) => JSON.stringify({ seq: i + 1, ...l })).join('\n') + '\n');
  assert.deepEqual(movedInWorktrees(dir).sort(), [c.wt, path.join(odd, 'slot-2')].sort());
});

test('90/91: a handover of an unreadable moved record before start still holds its worktree', async () => {
  const c = checkout();
  fs.writeFileSync(c.file, JSON.stringify({ version: 3, folders: {} }));
  const { h, id } = await runnerWith(c, (rec) => (rec.worktree = { ...c.worktree }));
  const other = '0123abcd-0000-4000-8000-000000000002';
  fs.cpSync(path.join(c.home, 'threads', id), path.join(c.home, 'threads', other), { recursive: true });
  const n = fs.readFileSync(path.join(c.home, 'threads', other, 'events.ndjson'), 'utf8').split('\n').filter(Boolean).length;
  fs.appendFileSync(path.join(c.home, 'threads', other, 'events.ndjson'), JSON.stringify({ seq: n + 1, type: 'system.note', text: `Moved from Bruce to M5, in ${c.wt}.` }) + '\n');
  fs.writeFileSync(path.join(c.home, 'threads', other, 'thread.json'), '{ broken');
  fs.rmSync(c.side);
  const fresh = new PatchTrust({ file: c.file, always: [c.repo] });
  (h.runner as any).patchTrust = fresh;
  h.runner.unvouchedSettled = false;
  h.runner.adopt(other);
  assert.equal((await fresh.check(c.patch, 0)).trusted, false);
});

// Muse's third look: the note's folders were tried at every "." after each
// ", in /", walking up each one, so a crafted note held the start for
// minutes; and a record rebuilt from a log of nulls threw at every start.
test('90/91: a crafted note is read in well under a second of CPU, and a long real one still holds', async () => {
  const { movedInWorktrees } = await import('../../src/runner/moves.ts');
  const c = checkout();
  const dir = tmpDir();
  const evil = { seq: 1, type: 'system.note', text: 'Moved from x, in /' + 'a/'.repeat(20_000) + '.'.repeat(20_000) + ', in /'.repeat(5000) };
  const long = { seq: 2, type: 'system.note', text: `Moved from Bruce to M5, in ${c.wt}. Left behind: ${'some/file.ts, '.repeat(100_000)}` };
  fs.writeFileSync(path.join(dir, 'events.ndjson'), [evil, long].map((l) => JSON.stringify(l)).join('\n') + '\n');
  const before = process.cpuUsage();
  const held = movedInWorktrees(dir);
  const used = process.cpuUsage(before);
  const ms = (used.user + used.system) / 1000;
  // About 0.2 s on a slow Linux box under a full check (14 Nov); the bug
  // took minutes, so a second still catches it without failing on load.
  assert.ok(ms < 1000, `took ${ms} ms of CPU`);
  assert.deepEqual(held, [c.wt]);
});

test('96: a record rebuilt from a log that holds no events starts the runner anyway', () => {
  const home = tmpDir();
  makeHarness(home);
  for (const [i, log] of ['null\n', '7\n"x"\n', 'not json\n', '{"type":3}\n'].entries()) {
    const dir = path.join(home, 'threads', `0123abcd-0000-4000-8000-00000000010${i}`);
    fs.mkdirSync(dir, { recursive: true });
    fs.writeFileSync(path.join(dir, 'thread.json'), '{ broken');
    fs.writeFileSync(path.join(dir, 'events.ndjson'), log);
  }
  const h = makeHarness(home);
  assert.ok(h.runner);
});
