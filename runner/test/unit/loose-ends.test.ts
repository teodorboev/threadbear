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
import { agentLooseEnds, anyOpen, childLooseEnds, editedLooseEnds, githubRepo, scheduleLooseEnds, worktreeLooseEnds, type Pr, type PrLookup } from '../../src/loose-ends.ts';
import { needsFromTool } from '../../src/needs.ts';
import { buildThreadbearTools } from '../../src/threadbear-mcp.ts';
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

test("looseEnds: end_turn's open is the agent's word, kept when empty, offered only while on", () => {
  assert.deepEqual(needsFromTool({ needs: 'nothing', open: [] }), { needs: 'nothing', open: [] });
  assert.deepEqual(needsFromTool({ needs: 'nothing' }), { needs: 'nothing' });
  const said = needsFromTool({ needs: 'nothing', open: ['Yuriy asked for this on X: worth a reply', 'Yuriy asked for this on X: worth a reply', '', 'b', 'c', 'd'] });
  assert.deepEqual(said?.open, ['Yuriy asked for this on X: worth a reply', 'b', 'c']);
  assert.deepEqual(agentLooseEnds(said?.open).map((e) => e.kind), ['open', 'open', 'open']);
  assert.equal(anyOpen(agentLooseEnds(['x'])), true);
  const h = setup();
  const { thread } = h.runner.create({ text: 'Fix it', accountId: 'claude-main' });
  const shape = (on: boolean) => (buildThreadbearTools(h.runner, thread.id, Date.now, undefined, undefined, undefined, { oneReply: () => true, looseEnds: () => on }).find((x) => x.name === 'end_turn')!.inputSchema as unknown as { shape: Record<string, unknown> }).shape;
  assert.ok('open' in shape(true));
  assert.ok(!('open' in shape(false)));
});

test('looseEnds: what the agent said is open, and files it changed outside a worktree, go on the card', async () => {
  const h = setup();
  const repo = fs.realpathSync(tmpDir('tb-loose-misc-'));
  sh(repo, 'git init -q -b main && echo one > notes.md && git add -A && git commit -qm init');
  const { thread } = h.runner.create({ text: 'Fix it', accountId: 'claude-main', cwd: repo });
  const id = thread.id;
  const s = h.adapter.sessions.filter((x) => x.opts.threadId === id).at(-1)!;
  s.init();
  s.out({ kind: 'event', event: { type: 'tool.call', itemId: 'w1', name: 'Edit', kind: 'edit', title: 'Edit notes.md', input: { file_path: path.join(repo, 'notes.md') } } });
  sh(repo, 'echo two > notes.md');
  finish(h, id, { needs: 'nothing', open: ['Yuriy asked for this on X: worth a reply'] });
  await until(() => Boolean(h.runner.get(id).looseEnds), 'the check');
  const ends = h.runner.get(id).looseEnds!.ends;
  assert.deepEqual(ends.map((e) => e.kind), ['uncommitted', 'open']);
  assert.match(ends[0].text, /1 file it changed in .* isn't committed/);
  assert.equal(ends[1].text, 'Yuriy asked for this on X: worth a reply');
  assert.deepEqual(checkSummary(h.runner.get(id)), []);
});
