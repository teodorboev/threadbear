// Loose ends on the done card (experiment looseEnds; loose-ends.ts and
// runner/loose-ends.ts): what's still open on a finished thread, checked
// by the runner so a plain Archive is safe without asking the agent.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { execFileSync } from 'node:child_process';
import { makeHarness, tmpDir } from '../helpers/fakes.ts';
import { checkSummary } from '../helpers/protocol-check.ts';
import { PATIENCE_MS } from '../helpers/patience.ts';
import { anyOpen, childLooseEnds, editedLooseEnds, githubRepo, runningLooseEnds, scheduleLooseEnds, worktreeLooseEnds, type Pr, type PrLookup } from '../../src/loose-ends.ts';
import { droppedOpenLines, needsFromTool } from '../../src/needs.ts';
import { buildThreadbearTools } from '../../src/threadbear-mcp.ts';
import { Scrubber } from '../../src/scrub.ts';
import { useScrubForCuts } from '../../src/safe-cut.ts';
import { redactSecrets, scrubText } from '../../src/redaction.ts';
import { EXPERIMENTS } from '../../src/experiments.ts';
import type { Schedule } from '../../src/protocol.ts';
import type { WorktreeRecord } from '../../src/worktrees.ts';

Object.assign(process.env, {
  GIT_CONFIG_GLOBAL: '/dev/null',
  GIT_CONFIG_NOSYSTEM: '1',
  GIT_AUTHOR_NAME: 'Test',
  GIT_AUTHOR_EMAIL: 'test@example.com',
  GIT_COMMITTER_NAME: 'Test',
  GIT_COMMITTER_EMAIL: 'test@example.com',
});

const END_TURN = 'mcp__threadbear__end_turn';

function sh(cwd: string, cmd: string): string {
  return execFileSync('/bin/sh', ['-c', cmd], { cwd, encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] }).trim();
}

// A repo on main with a thread's worktree beside it, on its own branch.
function repoWithWorktree(): WorktreeRecord {
  const root = fs.realpathSync(tmpDir('tb-loose-'));
  const main = path.join(root, 'proj');
  fs.mkdirSync(main);
  sh(main, 'git init -q -b main && printf "node_modules\\n" > .gitignore && echo one > a.txt && git add -A && git commit -qm init');
  const wt = path.join(root, 'slot-1');
  sh(main, `git worktree add -q -b thread/abcd1234 ${wt} main`);
  return { repo: 'proj', mainCheckout: main, path: wt, branch: 'thread/abcd1234', base: sh(main, 'git rev-parse main') };
}

async function until(check: () => boolean, what: string): Promise<void> {
  const end = Date.now() + PATIENCE_MS;
  while (!check()) {
    if (Date.now() > end) throw new Error(`Timed out waiting for ${what}`);
    await new Promise((r) => setTimeout(r, 10));
  }
}

function setup(on = true) {
  const home = tmpDir();
  if (on) fs.writeFileSync(path.join(home, 'runner.json'), JSON.stringify({ experiments: { looseEnds: true } }));
  return makeHarness(home);
}

function finish(h: ReturnType<typeof setup>, id: string, input: Record<string, unknown> = { needs: 'nothing' }) {
  const s = h.adapter.sessions.filter((x) => x.opts.threadId === id).at(-1)!;
  s.init();
  s.out({ kind: 'event', event: { type: 'tool.call', itemId: `e${Math.random()}`, name: END_TURN, kind: 'mcp', title: 'end_turn', input } });
  s.text(`m${Math.random()}`, 'Done.');
  s.result('completed');
}

test('looseEnds: off by default, with a check-in booked', () => {
  const x = EXPERIMENTS.find((e) => e.id === 'looseEnds');
  assert.equal(x?.on, false);
  assert.match(x?.reminder ?? '', /^[0-9a-f]{8}$/);
});

test('looseEnds: a worktree with nothing left says nothing; commits off main and uncommitted files are named', async () => {
  const w = repoWithWorktree();
  assert.deepEqual(await worktreeLooseEnds(w), []);
  // Ignored files (the linked node_modules, build output) aren't loose ends.
  fs.mkdirSync(path.join(w.path, 'node_modules'));
  fs.writeFileSync(path.join(w.path, 'node_modules', 'x.js'), '');
  assert.deepEqual(await worktreeLooseEnds(w), []);
  sh(w.path, 'echo two > b.txt && git add b.txt && git commit -qm two && echo three > c.txt && echo changed > a.txt');
  assert.deepEqual(await worktreeLooseEnds(w), [
    { kind: 'unlanded', text: "1 commit on thread/abcd1234 isn't on main" },
    { kind: 'uncommitted', text: "2 files in its worktree aren't committed" },
  ]);
  // Landed (main moved to the branch) and committed: nothing left.
  sh(w.path, 'git add -A && git commit -qm three');
  sh(w.mainCheckout, 'git merge -q --ff-only thread/abcd1234');
  assert.deepEqual(await worktreeLooseEnds(w), []);
  // A branch that's gone (cleaned up) isn't one either.
  assert.deepEqual(await worktreeLooseEnds({ ...w, branch: 'thread/gone', path: path.join(w.path, 'nope') }), []);
});

test('looseEnds: threads it started count while working or waiting on Mike; check-ins are listed but leave Archive safe', () => {
  const ends = childLooseEnds([
    { id: 'a', title: 'Fix the relay', working: true, needsYou: false },
    { id: 'b', title: 'Pick a picker', working: false, needsYou: true },
    { id: 'c', title: 'All done', working: false, needsYou: false },
  ]);
  assert.deepEqual(ends.map((e) => [e.kind, e.threadId]), [['thread', 'a'], ['thread', 'b']]);
  assert.match(ends[0].text, /"Fix the relay".*still working/);
  assert.match(ends[1].text, /"Pick a picker".*waiting for you/);
  const sched = (o: Partial<Schedule>): Schedule => ({ id: 'x', title: 'Check-in', text: '', whenText: '', paused: false, createdAt: '', updatedAt: '', runs: 0, ...o });
  const booked = scheduleLooseEnds('t1', [
    sched({ id: '1', threadId: 't1', title: 'Review the week', nextRunAt: '2026-11-13T01:30:00Z' }),
    sched({ id: '2', threadId: 't1', title: 'Paused', nextRunAt: '2026-11-10T00:00:00Z', paused: true }),
    sched({ id: '3', threadId: 'other', title: 'Not this one', nextRunAt: '2026-11-10T00:00:00Z' }),
    sched({ id: '4', threadId: 't1', title: 'Already ran', nextRunAt: undefined }),
  ]);
  assert.deepEqual(booked, [{ kind: 'scheduled', text: 'Review the week', at: '2026-11-13T01:30:00Z' }]);
  assert.equal(anyOpen(booked), false);
  assert.equal(anyOpen([...booked, ...ends]), true);
});

test('looseEnds: a done turn gets a check on its summary, kept up to date, and only for its own card', async () => {
  const h = setup();
  const { thread } = h.runner.create({ text: 'Fix it', accountId: 'claude-main' });
  const id = thread.id;
  const st = h.runner.threads.get(id)!;
  const w = repoWithWorktree();
  st.rec.worktree = w;
  sh(w.path, 'echo two > b.txt && git add b.txt && git commit -qm two');
  finish(h, id);
  await until(() => Boolean(h.runner.get(id).looseEnds), 'the check');
  let s = h.runner.get(id);
  assert.deepEqual(checkSummary(s), []);
  assert.equal(s.looseEnds?.seq, st.rec.lastTurnCompletedSeq);
  assert.deepEqual(s.looseEnds?.ends.map((e) => e.kind), ['unlanded']);

  // A thread it started, still working, shows on its card, and goes once that finishes.
  const child = h.runner.create({ text: 'Side job', accountId: 'claude-main', startedBy: { kind: 'thread', label: 'Agent', threadId: id } }).thread.id;
  h.runner.recheckLooseEnds(st);
  await until(() => Boolean(h.runner.get(id).looseEnds?.ends.some((e) => e.threadId === child)), 'the child listed');
  finish(h, child);
  await until(() => !h.runner.get(id).looseEnds?.ends.some((e) => e.threadId === child), 'the child gone');

  // Landed from a terminal: opening the thread checks again.
  sh(w.mainCheckout, 'git merge -q --ff-only thread/abcd1234');
  h.clock.advance(10_000);
  h.runner.recheckLooseEnds(st, true);
  await until(() => h.runner.get(id).looseEnds?.ends.length === 0, 'nothing left');

  // A check-in it booked is listed (main.ts wires this to the Scheduler).
  h.runner.schedulesFor = (t) => [{ id: 's', title: 'Review the week', text: '', whenText: '', paused: false, createdAt: '', updatedAt: '', runs: 0, threadId: t, nextRunAt: '2026-11-13T01:30:00Z' }];
  h.runner.recheckLooseEnds(st);
  await until(() => h.runner.get(id).looseEnds?.ends[0]?.kind === 'scheduled', 'the check-in');

  // A new turn's card waits for its own check: the old one isn't shown.
  h.runner.send(id, 'one more thing');
  s = h.runner.get(id);
  assert.equal(s.looseEnds, undefined);
});

test('looseEnds: not checked when off, when the turn needs Mike, or when it archives itself', async () => {
  const off = setup(false);
  const a = off.runner.create({ text: 'Fix it', accountId: 'claude-main' }).thread.id;
  finish(off, a);
  const h = setup();
  const b = h.runner.create({ text: 'Fix it', accountId: 'claude-main' }).thread.id;
  finish(h, b, { needs: 'you', asks: [{ question: 'Deploy it?' }] });
  const c = h.runner.create({ text: 'Fix it', accountId: 'claude-main' }).thread.id;
  finish(h, c, { needs: 'nothing', archive: true });
  // The one that's on and done, to know the checks have had time to run.
  const d = h.runner.create({ text: 'Fix it', accountId: 'claude-main' }).thread.id;
  finish(h, d);
  await until(() => Boolean(h.runner.get(d).looseEnds), 'the control');
  assert.equal(off.runner.get(a).looseEnds, undefined);
  assert.equal(h.runner.get(b).looseEnds, undefined);
  assert.equal(h.runner.get(c).looseEnds, undefined);
  assert.deepEqual(h.runner.get(d).looseEnds?.ends, []);
});

// The check-in of 11 Nov: most typed "anything else?" asks came from
// threads with no worktree of their own (misc, tandem, the video folders),
// and the answers were things only the agent knew (a reply worth sending,
// PRs waiting on a decision, an export being redone).
test('looseEnds: files a thread changed in a shared checkout count, not the rest of its status; unpushed commits too', async () => {
  const root = fs.realpathSync(tmpDir('tb-loose-shared-'));
  const remote = path.join(root, 'remote.git');
  sh(root, `git init -q --bare -b main ${remote}`);
  const repo = path.join(root, 'tandem');
  fs.mkdirSync(repo);
  sh(repo, `git init -q -b main && git remote add origin ${remote} && echo one > a.txt && echo one > b.txt && git add -A && git commit -qm init && git push -q -u origin main`);
  const a = path.join(repo, 'a.txt');
  // Another thread's work in the same checkout isn't this one's.
  sh(repo, 'echo theirs > b.txt');
  assert.deepEqual(await editedLooseEnds([a], []), []);
  sh(repo, 'echo mine > a.txt && mkdir -p new && echo x > new/c.txt');
  assert.deepEqual(await editedLooseEnds([a, path.join(repo, 'new', 'c.txt'), '/nowhere/gone.txt', 'relative.txt'], []), [
    { kind: 'uncommitted', text: "2 files it changed in tandem aren't committed" },
  ]);
  sh(repo, 'git add a.txt new && git commit -qm mine');
  const sha = sh(repo, 'git rev-parse HEAD');
  assert.deepEqual(await editedLooseEnds([a], [sha, 'f'.repeat(40)]), [{ kind: 'unpushed', text: "1 commit it made in tandem isn't pushed" }]);
  sh(repo, 'git push -q');
  assert.deepEqual(await editedLooseEnds([a], [sha]), []);
  // Its own worktree is worktreeLooseEnds' to say.
  sh(repo, 'echo again > a.txt');
  assert.deepEqual(await editedLooseEnds([a], [], repo), []);
  // No remote: nothing to push to, so no unpushed line.
  const local = path.join(root, 'local');
  fs.mkdirSync(local);
  sh(local, 'git init -q -b main && echo x > x.txt && git add -A && git commit -qm x');
  assert.deepEqual(await editedLooseEnds([path.join(local, 'x.txt')], [sh(local, 'git rev-parse HEAD')]), []);
});

// Mike, 13 Nov 2026: a done card listed "10 commits it made in slot-13
// aren't pushed", and the same for three more slots, for work that had all
// landed. tb land rebases a thread's commits onto main, so their own ids are
// never on a remote; and the slots are worktrees of one repo, so the same
// commits were counted once for each.
test('looseEnds: commits landed by a rebase are pushed, and worktrees of one repo count them once', async () => {
  const root = fs.realpathSync(tmpDir('tb-loose-landed-'));
  const remote = path.join(root, 'remote.git');
  sh(root, `git init -q --bare -b main ${remote}`);
  const repo = path.join(root, 'threadbear');
  fs.mkdirSync(repo);
  sh(repo, `git init -q -b main && git remote add origin ${remote} && echo one > a.txt && git add -A && git commit -qm init && git push -q -u origin main && git remote set-head origin main`);
  // The thread's work on its own branch, in a worktree as tb land's are.
  const wt = path.join(root, 'slot-1');
  sh(repo, `git worktree add -q -b thread/x ${wt}`);
  sh(wt, 'echo mine > a.txt && git commit -qam "Fix the thing"');
  const mine = sh(wt, 'git rev-parse HEAD');
  sh(wt, 'echo more > b.txt && git add b.txt && git commit -qm "And the other thing"');
  const more = sh(wt, 'git rev-parse HEAD');
  // Not landed yet: open, and named once though two checkouts know them.
  assert.deepEqual(await editedLooseEnds([], [mine, more], undefined, { dirs: [repo, wt] }), [{ kind: 'unpushed', text: "2 commits it made in threadbear aren't pushed" }]);
  // Main moves on, then tb land rebases the work onto it and pushes: new ids,
  // same author time and words. One of them had a conflict sorted on the way.
  sh(repo, 'echo theirs > c.txt && git add c.txt && git commit -qm "Someone else" && git push -q');
  sh(wt, 'git rebase -q main');
  sh(wt, 'git commit -q --amend --no-edit --allow-empty');
  sh(repo, 'git merge -q --ff-only thread/x && git push -q');
  assert.notEqual(sh(repo, 'git rev-parse HEAD'), more, 'new ids');
  assert.deepEqual(await editedLooseEnds([], [mine, more], undefined, { dirs: [repo, wt] }), []);
});

// Codex's review, 13 Nov 2026: what makes a rebased commit look landed
// mustn't hide real work here.
test('looseEnds: an amend of a pushed commit, still on its branch, is unpushed; a master repo is read too', async () => {
  const root = fs.realpathSync(tmpDir('tb-loose-amend-'));
  const remote = path.join(root, 'remote.git');
  sh(root, `git init -q --bare -b master ${remote}`);
  const repo = path.join(root, 'app');
  fs.mkdirSync(repo);
  sh(repo, `git init -q -b master && git remote add origin ${remote} && echo one > a.txt && git add -A && git commit -qm init && git push -q -u origin master`);
  sh(repo, 'echo mine > a.txt && git commit -qam "Fix the thing" && git push -q');
  const pushed = sh(repo, 'git rev-parse HEAD');
  // Changed and amended, not pushed: same author time and words as the one on master.
  sh(repo, 'echo better > a.txt && git commit -q -a --amend --no-edit');
  const amended = sh(repo, 'git rev-parse HEAD');
  assert.deepEqual(await editedLooseEnds([], [pushed, amended], undefined, { dirs: [repo] }), [{ kind: 'unpushed', text: "1 commit it made in app isn't pushed" }]);
  // No origin/HEAD here: master is found as main is, and a rebased landing on it counts.
  sh(repo, 'git push -q -f');
  assert.deepEqual(await editedLooseEnds([], [pushed, amended], undefined, { dirs: [repo] }), []);
});

// Security review, 11 Nov 2026 (finding 85): a folder from a zip carries its
// own .git/config, and a `filter.<name>.clean` there, picked by its
// .gitattributes, ran from the done card's `git status` as the owner, in
// Ask mode, with nobody asked. A submodule's own config did the same
// through the safe git, as git runs another git inside it to see whether
// it changed.
test("looseEnds: a repo's own filter, or a submodule's, never runs from the check", async () => {
  const root = fs.realpathSync(tmpDir('tb-loose-filter-'));
  const marker = path.join(root, 'ran');
  const payload = path.join(root, 'payload.sh');
  fs.writeFileSync(payload, `#!/bin/sh\necho "$1" >> '${marker}'\ncat\n`, { mode: 0o755 });
  const sub = path.join(root, 'vendor-src');
  fs.mkdirSync(sub);
  sh(sub, 'git init -q -b main && echo "*.txt filter=sub" > .gitattributes && echo one > v.txt && git add -A && git commit -qm v');
  const repo = path.join(root, 'from-a-zip');
  fs.mkdirSync(repo);
  sh(repo, `git init -q -b main && echo "*.md filter=evil" > .gitattributes && echo one > notes.md && git add -A && git commit -qm init`);
  sh(repo, `git -c protocol.file.allow=always submodule add -q ${sub} vendor && git commit -qm vendor`);
  // Planted after the commits, as an archive would bring them.
  sh(repo, `git config filter.evil.clean "${payload} repo" && git config submodule.vendor.ignore none`);
  sh(path.join(repo, 'vendor'), `git config filter.sub.clean "${payload} submodule"`);
  // Same-size edits, so git has to hash them (through the filter) to know.
  sh(repo, 'echo two > notes.md && echo two > vendor/v.txt');
  const ran = () => (fs.existsSync(marker) ? [...new Set(fs.readFileSync(marker, 'utf8').split('\n').filter(Boolean))].sort() : []);
  // Plain git runs both, so the checks below mean something.
  sh(repo, 'git status --porcelain >/dev/null');
  assert.deepEqual(ran(), ['repo', 'submodule']);
  fs.rmSync(marker);
  sh(repo, 'touch -t 202001010000 notes.md vendor/v.txt');
  assert.deepEqual(await editedLooseEnds([path.join(repo, 'notes.md')], []), [{ kind: 'uncommitted', text: "1 file it changed in from-a-zip isn't committed" }]);
  // Its own worktree: the whole status, submodule and all.
  sh(repo, 'touch -t 202001010001 notes.md vendor/v.txt');
  const own: WorktreeRecord = { repo: 'from-a-zip', mainCheckout: repo, path: repo, branch: 'main', base: sh(repo, 'git rev-parse main') };
  assert.deepEqual(await worktreeLooseEnds(own), [{ kind: 'uncommitted', text: "1 file in its worktree isn't committed" }]);
  // And a file it changed inside the submodule, which is a repo of its own.
  sh(repo, 'touch -t 202001010002 notes.md vendor/v.txt');
  assert.deepEqual(await editedLooseEnds([path.join(repo, 'vendor', 'v.txt')], []), [{ kind: 'uncommitted', text: "1 file it changed in vendor isn't committed" }]);
  assert.deepEqual(ran(), []);
});

// 11 Nov 2026: thread 0222c475 said Done with its work pushed to draft PRs
// that weren't merged, and the card showed a plain Done.
test('looseEnds: work pushed to a branch that main lacks is open until its PR is merged or closed', async () => {
  const root = fs.realpathSync(tmpDir('tb-loose-pr-'));
  const remote = path.join(root, 'remote.git');
  sh(root, `git init -q --bare -b main ${remote}`);
  const repo = path.join(root, 'website');
  fs.mkdirSync(repo);
  sh(repo, `git init -q -b main && git remote add origin ${remote} && echo one > a.txt && git add -A && git commit -qm init && git push -q -u origin main && git remote set-head origin main`);
  const a = path.join(repo, 'a.txt');
  sh(repo, 'git checkout -q -b mike/examples && echo two > a.txt && git commit -qam examples && git push -q -u origin mike/examples');
  const sha = sh(repo, 'git rev-parse HEAD');
  const asked: string[] = [];
  const lookup = (pr: Pr | undefined): PrLookup => async (r, b) => {
    asked.push(`${r} ${b}`);
    return pr;
  };
  // A local remote isn't GitHub's, so there's no PR to ask about.
  assert.deepEqual(await editedLooseEnds([a], [], undefined, { pushed: [sha], prs: lookup(undefined) }), [
    { kind: 'unmerged', text: "Work it pushed to mike/examples in website isn't on main" },
  ]);
  assert.deepEqual(asked, []);
  // On GitHub: an open PR names itself, a draft says so, a merged or closed
  // one (squashed, so the commit never reaches main) isn't open.
  sh(repo, `git remote set-url origin https://github.com/convex/website.git && git config remote.origin.pushurl ${remote}`);
  assert.deepEqual(await editedLooseEnds([a], [], undefined, { pushed: [sha], prs: lookup({ number: 1092, state: 'OPEN', isDraft: true }) }), [
    { kind: 'unmerged', text: 'PR #1092 in website is still a draft' },
  ]);
  assert.deepEqual(asked, ['convex/website mike/examples']);
  assert.deepEqual(await editedLooseEnds([a], [], undefined, { pushed: [sha], prs: lookup({ number: 1092, state: 'OPEN', isDraft: false }) }), [
    { kind: 'unmerged', text: "PR #1092 in website isn't merged" },
  ]);
  assert.deepEqual(await editedLooseEnds([a], [], undefined, { pushed: [sha], prs: lookup({ number: 1092, state: 'MERGED', isDraft: false, headRefOid: sh(repo, 'git rev-parse origin/mike/examples') }) }), []);
  assert.deepEqual(await editedLooseEnds([a], [], undefined, { pushed: [sha], prs: lookup({ number: 1092, state: 'CLOSED', isDraft: false, headRefOid: sh(repo, 'git rev-parse origin/mike/examples') }) }), []);
  // A branch reused after its PR merged: newer pushes aren't covered by it.
  const merged = { number: 1092, state: 'MERGED' as const, isDraft: false, headRefOid: sh(repo, 'git rev-parse origin/mike/examples') };
  sh(repo, 'echo three > a.txt && git commit -qam more && git push -q origin mike/examples');
  const more = sh(repo, 'git rev-parse HEAD');
  assert.deepEqual(await editedLooseEnds([a], [], undefined, { pushed: [sha, more], prs: lookup(merged) }), [
    { kind: 'unmerged', text: "Work it pushed to mike/examples in website isn't on main" },
  ]);
  // A thread that changed files only through a shell is found by its folder.
  assert.deepEqual(await editedLooseEnds([], [], undefined, { pushed: [more], dirs: [repo], prs: lookup(undefined) }), [
    { kind: 'unmerged', text: "Work it pushed to mike/examples in website isn't on main" },
  ]);
  // Once main has it, there's nothing to ask.
  asked.length = 0;
  sh(repo, 'git checkout -q main && git merge -q --ff-only mike/examples && git push -q origin main');
  assert.deepEqual(await editedLooseEnds([a], [], undefined, { pushed: [sha, more], prs: lookup({ number: 1092, state: 'OPEN', isDraft: false }) }), []);
  assert.deepEqual(asked, []);
  assert.equal(githubRepo('git@github.com:convex/website.git'), 'convex/website');
  assert.equal(githubRepo('https://github.com/mikecann/threadbear'), 'mikecann/threadbear');
  assert.equal(githubRepo('https://gitlab.com/a/b.git'), undefined);
});

// 13 Nov 2026 (thread b7dd46fc): of the 14 lines agents had put on Mike's
// done cards through end_turn's `open`, none gave him anything to do
// ("Geoff putting the portal and lights pieces into the next newsletter"),
// so the card lists only what the runner checked. A session from before
// still sends them: they're taken and dropped.
test("looseEnds: end_turn's open is gone: an older session's lines aren't kept, offered or put on the card", async () => {
  assert.deepEqual(needsFromTool({ needs: 'nothing', open: ['Geoff putting the pieces into the next newsletter'] }), { needs: 'nothing' });
  assert.deepEqual(needsFromTool({ needs: 'nothing', open: [] }), { needs: 'nothing' });
  assert.equal(droppedOpenLines({ needs: 'nothing', open: ['Waiting on Geoff', ''] }), true);
  assert.equal(droppedOpenLines({ needs: 'nothing', open: [] }), false);
  assert.equal(droppedOpenLines({ needs: 'nothing' }), false);
  const h = setup();
  const repo = fs.realpathSync(tmpDir('tb-loose-misc-'));
  sh(repo, 'git init -q -b main && echo one > notes.md && git add -A && git commit -qm init');
  const { thread } = h.runner.create({ text: 'Fix it', accountId: 'claude-main', cwd: repo });
  const id = thread.id;
  const tools = buildThreadbearTools(h.runner, id, Date.now, undefined, undefined, undefined, { oneReply: () => true, sleep: async () => {} });
  const endTurn = tools.find((x) => x.name === 'end_turn')!;
  assert.ok(!('open' in (endTurn.inputSchema as unknown as { shape: Record<string, unknown> }).shape));
  const s = h.adapter.sessions.filter((x) => x.opts.threadId === id).at(-1)!;
  s.init();
  s.out({ kind: 'event', event: { type: 'tool.call', itemId: 'w1', name: 'Edit', kind: 'edit', title: 'Edit notes.md', input: { file_path: path.join(repo, 'notes.md') } } });
  sh(repo, 'echo two > notes.md');
  // The older session's call is told where its lines go instead.
  const input = { needs: 'nothing', headline: 'Draft is ready', open: ['Geoff putting the pieces into the next newsletter'] };
  s.out({ kind: 'event', event: { type: 'tool.call', itemId: 'e1', name: END_TURN, kind: 'mcp', title: 'end_turn', input } });
  const said = (await endTurn.handler(input as never, {})) as { content: { text: string }[]; isError?: boolean };
  assert.ok(!said.isError);
  assert.match(said.content[0].text, /^Noted: this thread/);
  assert.match(said.content[0].text, /"open" lines aren't shown any more: say them in your reply/);
  const plain = (await endTurn.handler({ needs: 'nothing', headline: 'Draft is ready' } as never, {})) as { content: { text: string }[] };
  assert.doesNotMatch(plain.content[0].text, /"open" lines/);
  // A Codex thread's tool list is fixed when it starts, so it sends them too.
  const codexEnd = buildThreadbearTools(h.runner, id, Date.now, undefined, undefined, undefined, { provider: 'codex', oneReply: () => true, sleep: async () => {} }).find((x) => x.name === 'end_turn')! as unknown as { formFor: (args: unknown) => { handler: (a: unknown, e: unknown) => Promise<{ content: { text: string }[] }> } };
  assert.match((await codexEnd.formFor(input).handler({ needs: 'nothing', headline: 'Draft is ready' }, {})).content[0].text, /"open" lines aren't shown any more/);
  s.text('m1', 'Done.');
  s.result('completed');
  await until(() => Boolean(h.runner.get(id).looseEnds), 'the check');
  const ends = h.runner.get(id).looseEnds!.ends;
  assert.deepEqual(ends.map((e) => e.kind), ['uncommitted']);
  assert.match(ends[0].text, /1 file it changed in .* isn't committed/);
  assert.deepEqual(checkSummary(h.runner.get(id)), []);
});

// What seven of those 14 lines were saying ("waiting in the landing
// queue"), from what the runner knows: archiving stops the thread's CLI
// and its background jobs with it, so a job still going is on the card.
test("looseEnds: a background job still running is on the card, says archiving stops it, and goes when it ends", async () => {
  assert.deepEqual(runningLooseEnds([{ kind: 'shell', description: 'Land the fix on main' }, { kind: 'shell', description: 'npm test', finished: true }, { kind: 'agent', description: '' }]), [
    { kind: 'running', text: 'Still running: Land the fix on main. Archiving stops it.' },
    { kind: 'running', text: 'Still running: a background job. Archiving stops it.' },
  ]);
  assert.equal(runningLooseEnds([{ kind: 'shell', description: `${'x'.repeat(200)}.` }])[0].text.length < 160, true);
  assert.equal(runningLooseEnds(Array.from({ length: 5 }, (_, i) => ({ kind: 'shell' as const, description: `job ${i}` }))).length, 3);
  assert.equal(anyOpen(runningLooseEnds([{ kind: 'shell', description: 'x' }])), true);
  const h = setup();
  const { thread } = h.runner.create({ text: 'Fix it', accountId: 'claude-main', cwd: fs.realpathSync(tmpDir('tb-loose-job-')) });
  const id = thread.id;
  const s = h.adapter.sessions.filter((x) => x.opts.threadId === id).at(-1)!;
  s.init();
  s.out({ kind: 'tasks', tasks: [{ id: 'b1', type: 'local_bash', kind: 'shell', description: 'Land the fix on main' }] });
  finish(h, id);
  await until(() => Boolean(h.runner.get(id).looseEnds), 'the check');
  assert.deepEqual(h.runner.get(id).looseEnds!.ends, [{ kind: 'running', text: 'Still running: Land the fix on main. Archiving stops it.' }]);
  assert.deepEqual(checkSummary(h.runner.get(id)), []);
  // Codex starts no turn to read a finished job's result, so the line has
  // to go by itself.
  (h.adapter as { wakesAfterTasks?: boolean }).wakesAfterTasks = false;
  s.out({ kind: 'tasks', tasks: [] });
  await until(() => h.runner.get(id).looseEnds?.ends.length === 0, 'the check after the job ended');
});

// Daybreak 115 (13 Nov 2026): a Codex job's description is its whole
// command. Cut at 99 characters with ". Archiving stops it." after, a key
// the runner knows (one typed on the phone) that crossed the cut went up
// all but its last character: the scrub knows a cut-off key only at the
// very end of a text. The line is scrubbed as sync scrubs it (Scrubber.doc).
test('looseEnds: a running job cut short never sends part of a key the runner knows', () => {
  const scrubber = new Scrubber({ env: {}, folders: false });
  const key = 'a1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8f9';
  const dotted = 'Zq8Wx3Lk9Vm2Pn7Rt4Ys6Hd1Jf5Gc0Bv...';
  scrubber.known.add(key);
  scrubber.known.add(dotted);
  useScrubForCuts((t) => redactSecrets(scrubber.known.mask(t)).text);
  const leaks = (text: string, secret: string) => {
    for (let i = 0; i + 8 <= secret.length; i++) if (text.includes(secret.slice(i, i + 8))) return true;
    return false;
  };
  try {
    const cases = [
      `curl -H "X-Token: ${'q'.repeat(20)}" ${key} --retry 3`, // the write-up's: 36 before, 8 after
      `${'x'.repeat(98 - 10)} ${key}`, // key starts just before the cut
      `${'x'.repeat(40)}\n\t${key}\nmore after the key`, // whitespace closed up first
      `${'y'.repeat(100 - key.length - 1)} ${key}`, // exactly 100: whole, and masked whole
      `${'y'.repeat(100 - key.length)}${key}`,
      `deploy --token ${dotted}`, // a key that ends in dots keeps them
      `${'z'.repeat(80)} ${dotted}`,
      `postgres://alice:${'p'.repeat(60)}@db.internal.example/app ${'w'.repeat(20)}`, // shape rules too
    ];
    for (const description of cases) {
      const [end] = runningLooseEnds([{ kind: 'shell', description }]);
      assert.ok(end.text.endsWith(' Archiving stops it.') || end.text.startsWith('Still running (archiving stops it): '), end.text);
      const sent = scrubber.doc({ looseEnds: [{ kind: end.kind, text: end.text }] }).looseEnds[0].text;
      assert.ok(!leaks(sent, key), sent);
      assert.ok(!leaks(sent, dotted.replace(/\.+$/, '')), sent);
      assert.ok(!leaks(sent, 'p'.repeat(60)), sent);
    }
    // Still cut to about the same length, and plain words are cut where they were.
    const plain = runningLooseEnds([{ kind: 'shell', description: 'w '.repeat(100) }])[0].text;
    assert.ok(plain.length < 160, plain);
    assert.match(plain, /^Still running \(archiving stops it\): (w )+w…$/);
  } finally {
    useScrubForCuts(scrubText);
  }
  // Codex's review: a key the runner learns of only after the line was made
  // (a .env still being read when the turn ended). Cut, the line ends with
  // the cut words, so the scrub's check for a value cut off at the end sees it.
  const late = new Scrubber({ env: {}, folders: false });
  const lateKey = 'f0e1d2c3b4a5968778695a4b3c2d1e0ff0e1d2c3b4a5968778695a4b3c2d1e0f';
  const [made] = runningLooseEnds([{ kind: 'shell', description: `curl -H "X-Token: ${'q'.repeat(20)}" ${lateKey} --retry 3` }]);
  late.known.add(lateKey);
  const out = late.doc({ text: made.text }).text;
  for (let i = 0; i + 8 <= lateKey.length; i++) assert.ok(!out.includes(lateKey.slice(i, i + 8)), out);
  // Codex's second review. A key ending in dots, learned later, in a short
  // line: whole, so the whole-value match still finds it.
  const dots = new Scrubber({ env: {}, folders: false });
  const dotKey = 'Zq8Wx3Lk9Vm2Pn7Rt4Ys6Hd1Jf5Gc0Bv...';
  const [short] = runningLooseEnds([{ kind: 'shell', description: `deploy --token ${dotKey}` }]);
  assert.equal(short.text, `Still running: deploy --token ${dotKey} Archiving stops it.`);
  dots.known.add(dotKey);
  assert.ok(!dots.doc({ t: short.text }).t.includes('Zq8Wx3Lk'), dots.doc({ t: short.text }).t);
  // Two late values, one the start of the other, the longer cut off.
  const both = new Scrubber({ env: {}, folders: false });
  const longKey = 'kP3vN8qR2xL7mT4wZ9bH6cJ1fD5gS0aYkP3vN8qR2xL7mT4wZ9bH6cJ1fD5gS0aY';
  const [cut] = runningLooseEnds([{ kind: 'shell', description: `${'r'.repeat(39)} ${longKey} --and-more` }]);
  both.known.add(longKey);
  both.known.add(longKey.slice(0, 20));
  const sentBoth = both.doc({ t: cut.text }).t;
  for (let i = 0; i + 8 <= longKey.length; i++) assert.ok(!sentBoth.includes(longKey.slice(i, i + 8)), sentBoth);
});
