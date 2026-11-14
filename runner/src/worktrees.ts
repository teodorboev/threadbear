// A worktree per thread (docs/thread-worktrees.md): a repo whose patch has
// `worktree` gives each new thread a git worktree and branch of its own,
// made from main, and `tb land` puts the branch on main once its check
// passes. This file is the git side: making one, landing one and cleaning
// one up. The runner (runner.ts) decides when, and says what happened in
// the thread; tb (cli.ts) runs land.
//
// Everything here runs real git. Nothing ever pushes.
import { execFile, spawn } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import type { WorktreePatch } from './patches.ts';
import { inBackground } from './background.ts';
import { mainCheckoutOf as linkedMainCheckout } from './patch-trust.ts';
import { untrustedGitArgs } from './safe-git.ts';
import { LAND_TIMES_FILE, joinQueue, leaveQueue, queueDir, readEntry, setRunning, waiting, waitingLine, writeEntry, type QueueEntry } from './land-queue.ts';

// What a thread's record keeps about its worktree.
export type WorktreeRecord = {
  repo: string; // the repo's name: the main checkout's folder name ("threadbear")
  mainCheckout: string; // the checkout with main checked out, where it lands
  path: string; // the worktree's root
  branch: string; // "thread/<id8>"
  base: string; // main's commit when it was made
  removedAt?: string; // cleaned up when the thread was archived
  // Archived, and the cleanup hasn't had its say yet: it waits for the CLI and
  // turn-end hooks, so a runner stopping or crashing can come first. The next
  // runner does it (settleLostCleanups).
  cleanupOwed?: boolean;
  // The patch's worktree.keep: the folder is set aside for another thread
  // when this one's done with it, rather than removed (see "spares").
  keep?: string[];
  // Set aside as a spare (with removedAt): the folder is still there, but
  // it's another thread's now, or will be.
  spare?: boolean;
  // The patch that asked for the worktree, relative to `path`: its copy
  // there is where the teardown is read from. Absent in records from before
  // 9 Nov, which look for it from the thread's folder.
  patchFile?: string;
  // When the patch's teardown last ran (it runs as the cleanup starts). Each
  // archive clears it, so it runs once per archive: a delete or the next
  // runner picking the cleanup up again doesn't run it twice. The thread's
  // next turn in the worktree clears it too, saying the teardown ran.
  tornDownAt?: string;
  // Made for a thread another runner moved here (runner/moves.ts placeMove):
  // its files are that runner's work. Whether that work is held from the
  // always-trust is patch-trust-unvouched.json's to say; this lets the runner
  // work the list out again if that file is lost (settleUnvouched).
  movedIn?: true;
  // The mark a runner of 10 Nov 2026 put here instead of that file, read
  // only by settleUnvouched.
  unvouched?: true;
};

export const MAIN = 'main';

export function threadBranch(threadId: string): string {
  return `thread/${threadId.slice(0, 8)}`;
}

type GitResult = { ok: boolean; code: number; stdout: string; stderr: string };

// How long one git may take before it's killed and counts as failed. Long,
// as a land's merge can run the repo's hooks; past it, git has hung (a .git
// swapped for a fifo, say: Codex's review, 9 Nov), and an archive's cleanup
// waiting on it held up messages, actions and handovers for ever. Tests
// shorten it.
export const gitLimits = { timeoutMs: 5 * 60_000 };

// git in `cwd`. Variables that point git at some other repo (set when this
// runs from inside a git hook) are dropped, so -C always wins, and the
// repo's own config can't start an fsmonitor program (safe-git.ts). Killed
// past gitLimits, which makes it a failure (`ok: false`).
export function git(cwd: string, args: string[], extraEnv: NodeJS.ProcessEnv = {}): Promise<GitResult> {
  const env: NodeJS.ProcessEnv = { ...process.env, GIT_TERMINAL_PROMPT: '0', ...extraEnv };
  for (const k of ['GIT_DIR', 'GIT_WORK_TREE', 'GIT_INDEX_FILE', 'GIT_COMMON_DIR', 'GIT_PREFIX']) delete env[k];
  return new Promise((resolve) => {
    execFile('git', untrustedGitArgs(cwd, args), { env, maxBuffer: 16 * 1024 * 1024, timeout: gitLimits.timeoutMs, killSignal: 'SIGKILL' }, (err, stdout, stderr) => {
      // Killed (past the limit) is -1, never 1: callers read git's own 1 as
      // an answer ("no such ref", "not an ancestor").
      const e = err as { code?: unknown; signal?: unknown } | null;
      const code = !e ? 0 : typeof e.code === 'number' ? e.code : e.signal ? -1 : 1;
      resolve({ ok: !err, code, stdout: String(stdout), stderr: String(stderr) });
    });
  });
}

async function gitOut(cwd: string, args: string[], extraEnv?: NodeJS.ProcessEnv): Promise<string | undefined> {
  const r = await git(cwd, args, extraEnv);
  return r.ok ? r.stdout.trim() : undefined;
}

// Commits in `range` with a line starting `Important:` that git's trailer
// parser doesn't read (one line each: short hash and subject), or undefined
// when git couldn't say. Uses the same parser as the release's
// `%(trailers:key=Important)`, so what passes here is what gets flagged.
async function unreadImportant(cwd: string, range: string): Promise<string[] | undefined> {
  const r = await gitOut(cwd, ['log', '--first-parent', '--no-merges', '-i', '--grep=^Important:', '--format=%h%x1f%s%x1f%(trailers:key=Important,valueonly,separator=%x20)', range]);
  if (r === undefined) return undefined;
  return r
    .split('\n')
    .filter(Boolean)
    .map((l) => l.split('\x1f'))
    .filter(([, , why]) => !why?.trim())
    .map(([hash, subject]) => `${hash} ${subject}`);
}

function firstLine(text: string): string {
  return (
    text
      .split('\n')
      .map((l) => l.replace(/^(fatal|error): /, '').trim())
      .find(Boolean) ?? 'git failed'
  );
}

// The checkouts of a repo, from `git worktree list --porcelain`.
export type Checkout = { path: string; branch?: string; head?: string; locked?: string };

export async function listCheckouts(cwd: string): Promise<Checkout[] | undefined> {
  const out = await gitOut(cwd, ['worktree', 'list', '--porcelain']);
  return out === undefined ? undefined : parseCheckouts(out);
}

export function parseCheckouts(out: string): Checkout[] {
  const list: Checkout[] = [];
  for (const block of out.split(/\n\s*\n/)) {
    const c: Checkout = { path: '' };
    for (const line of block.split('\n')) {
      if (line.startsWith('worktree ')) c.path = line.slice('worktree '.length);
      else if (line.startsWith('branch ')) c.branch = line.slice('branch '.length).replace(/^refs\/heads\//, '');
      else if (line.startsWith('HEAD ')) c.head = line.slice('HEAD '.length);
      else if (line === 'locked' || line.startsWith('locked ')) c.locked = line.slice('locked '.length);
    }
    if (c.path) list.push(c);
  }
  return list;
}

export async function mainCheckoutOf(cwd: string): Promise<string | undefined> {
  return (await listCheckouts(cwd))?.find((c) => c.branch === MAIN)?.path;
}

export function realpath(p: string): string {
  try {
    return fs.realpathSync(p);
  } catch {
    return path.resolve(p);
  }
}

function isDir(p: string): boolean {
  try {
    return fs.statSync(p).isDirectory();
  } catch {
    return false;
  }
}

// The same subfolder of `to` as `cwd` is of `from`, or `to` itself when
// that folder isn't there (it was untracked, or `cwd` was elsewhere).
export function sameSubfolder(cwd: string, from: string, to: string): string {
  const rel = path.relative(realpath(from), realpath(cwd));
  if (!rel || rel.startsWith('..') || path.isAbsolute(rel)) return to;
  const there = path.join(to, rel);
  return isDir(there) ? there : to;
}

// --------------------------------------------------------------- making

// `patchFile`: the worktree's own copy of the patch that asked for it.
export type MadeWorktree = { ok: true; worktree: WorktreeRecord; cwd: string; patchFile: string } | { ok: false; reason: string };

// Makes the thread's worktree from main in the main checkout. A leftover
// branch of the thread's name is a reason, never overwritten.
//
// Without the patch's `keep`: `git worktree add -b thread/<id8>
// <parent>/<repo>-threads/<id8> main`, and a leftover folder of that name
// is a reason too.
//
// With `keep`: a spare, if there is one (see "spares" below), switched to
// the new branch with its kept folders left as they were; otherwise a new
// `<repo>-threads/slot-<n>`. The folder isn't named after the thread, as
// it outlives it.
export function makeWorktree(opts: { cwd: string; threadId: string; patch: WorktreePatch }): Promise<MadeWorktree> {
  // One at a time, so two new threads never take the same spare or slot.
  const made = making.then(() => makeWorktreeNow(opts));
  making = made.then(
    () => undefined,
    () => undefined,
  );
  return made;
}

let making: Promise<void> = Promise.resolve();

async function makeWorktreeNow(opts: { cwd: string; threadId: string; patch: WorktreePatch }): Promise<MadeWorktree> {
  const top = await gitOut(opts.patch.folder, ['rev-parse', '--show-toplevel']);
  if (!top) return { ok: false, reason: `the ${opts.patch.patch} patch asks for one, but ${opts.patch.folder} isn't in a git repo` };
  const mainCheckout = await mainCheckoutOf(top);
  if (!mainCheckout) return { ok: false, reason: `no checkout of ${path.basename(top)} has ${MAIN} checked out` };
  const repo = path.basename(mainCheckout);
  const threads = path.join(path.dirname(mainCheckout), `${repo}-threads`);
  const id8 = opts.threadId.slice(0, 8);
  const branch = threadBranch(opts.threadId);
  const keep = opts.patch.keep;
  if (!keep && fs.existsSync(path.join(threads, id8))) return { ok: false, reason: `there's already a folder at ${path.join(threads, id8)}` };
  if ((await git(mainCheckout, ['rev-parse', '--verify', '--quiet', `refs/heads/${branch}`])).ok) {
    return { ok: false, reason: `the branch ${branch} already exists` };
  }
  const base = await gitOut(mainCheckout, ['rev-parse', MAIN]);
  if (!base) return { ok: false, reason: `couldn't read ${MAIN} in ${mainCheckout}` };
  const patchFile = path.relative(top, realpath(opts.patch.file));
  const done = (where: string): MadeWorktree => ({
    ok: true,
    worktree: { repo, mainCheckout, path: where, branch, base, ...(keep ? { keep } : {}), patchFile },
    cwd: sameSubfolder(opts.cwd, top, where),
    patchFile: path.join(where, patchFile),
  });

  if (keep) {
    for (const spare of await spares(mainCheckout, threads)) {
      if (!(await claimSpare(spare))) continue;
      if (await reuse(spare, branch, base, keep)) return done(spare);
      // Couldn't be put right: it stays unlocked, out of the pool, for
      // someone to look at, and the thread gets a new slot.
    }
  }

  let where = path.join(threads, id8);
  if (keep) {
    let n = 1;
    while (fs.existsSync(path.join(threads, `slot-${n}`))) n++;
    where = path.join(threads, `slot-${n}`);
  }
  fs.mkdirSync(threads, { recursive: true });
  const add = await git(mainCheckout, ['worktree', 'add', '-b', branch, where, base]);
  if (!add.ok) return { ok: false, reason: `git worktree add failed: ${firstLine(add.stderr)}` };
  return done(where);
}

// ---------------------------------------------------------------- spares
//
// A repo whose patch has worktree.keep (its build caches, like
// "app/.build") gets its worktrees reused: SwiftPM's cache only works in
// the folder it was built in, so every new worktree was a cold build of
// several minutes. When a thread is archived with everything landed, its
// worktree is detached, its branch deleted, and the folder locked with
// the reason below: a spare. The next new thread unlocks one, wipes
// everything git doesn't track except the kept folders, and checks out its
// branch there, so its first build only recompiles what changed.

const SPARE = 'threadbear spare';

// How many spares a repo keeps. Each holds its build caches (a gigabyte or
// so for Threadbear's), so past this an archived thread's worktree is
// removed as before.
export const MAX_SPARES = 6;

// The repo's spares, in `threads` (its <repo>-threads folder).
async function spares(mainCheckout: string, threads: string): Promise<string[]> {
  const dir = realpath(threads);
  return ((await listCheckouts(mainCheckout)) ?? [])
    .filter((c) => c.locked === SPARE && path.dirname(realpath(c.path)) === dir && isDir(c.path))
    .map((c) => c.path)
    .sort();
}

// Takes a spare out of the pool, or says another got there first. Not
// `git worktree unlock`: two at once both read the lock, and the second
// one's unlink finding no file still counts as success, so both took the
// spare (Codex, 8 Nov). A rename of git's lock file is atomic, so only
// one claimer wins; borrow-worktree.sh claims the same way.
async function claimSpare(spare: string): Promise<boolean> {
  const admin = await gitOut(spare, ['rev-parse', '--absolute-git-dir']);
  if (!admin) return false;
  const lock = path.join(admin, 'locked');
  const claimed = `${lock}.claimed-${process.pid}-${Date.now()}`;
  try {
    fs.renameSync(lock, claimed);
  } catch {
    return false;
  }
  fs.rmSync(claimed, { force: true });
  return true;
}

// A claimed spare, made like new on `branch` at `base`. The kept folders
// are left as they are; everything else git doesn't track goes, ignored
// files too (an old thread's node_modules, say), and setup runs again.
async function reuse(where: string, branch: string, base: string, keep: string[]): Promise<boolean> {
  const exclude = keep.flatMap((k) => ['-e', `/${k.replace(/^\/+|\/+$/g, '')}`]);
  for (const args of [['reset', '-q', '--hard'], ['clean', '-ffdxq', ...exclude], ['checkout', '-q', '-b', branch, base]]) {
    if (!(await git(where, args)).ok) return false;
  }
  return true;
}

// What the agent is told in its appended system prompt. The timeout: time
// queued behind another landing counts against the agent's own limit for a
// command (Claude Code's background default is 30 minutes), and on 11 Nov
// one cut a land off partway through its check that way.
export function worktreeInstructions(w: WorktreeRecord): string {
  return [
    `This thread has its own git worktree of ${w.repo} at ${w.path}, on the branch ${w.branch}, made from ${MAIN}.`,
    `Commit your work there, on that branch. Other threads work in worktrees of their own, and the main checkout at ${w.mainCheckout} isn't yours to edit.`,
    `When the work is done and checked, run \`tb land\` in the worktree: it rebases the branch on ${MAIN}, runs the repo's check and fast-forwards ${MAIN} to it. It never pushes. It may first wait for another thread's landing, so run it in the background with the longest timeout allowed. After landing you can carry on, and the next \`tb land\` lands only what's new.`,
  ].join(' ');
}

// --------------------------------------------------------------- landing

export type LandTarget = {
  path: string; // the worktree's root
  branch: string;
  mainCheckout: string;
  check?: string; // the patch's worktree.check
};

export type LandOptions = {
  dryRun?: boolean;
  out?: (line: string) => void;
  err?: (line: string) => void;
  // How the check runs; tests pass a plain shell.
  shell?: string[];
  // The CLI reads the patch after rebasing, including changes to the
  // required check made by a landing it just waited for.
  readCheck?: () => Promise<string | undefined>;
  // Why these files (what the landing changes, from the worktree's root)
  // can't land now, or undefined: the CLI's is a GitHub workflow that's red
  // on main (land-gate.ts). Asked after the rebase, before the check, so a
  // held landing doesn't wait out a check first.
  hold?: (root: string, files: string[]) => Promise<string | undefined>;
  // `tb land --despite-red`: this landing is the repair for what's red, so
  // `hold` isn't asked about its own files. In the queue it's a fix: it
  // goes first in any group check and the halving never leaves it out.
  despiteRed?: boolean;
  // Where readCheck looks from, relative to the worktree's root (a
  // subfolder's patch can ask for its own check). Only landings that look
  // from the same place are checked together, so the check read on the
  // stack is the one each of them asks for.
  checkFrom?: string;
  // The check hosts this landing asks for: THREADBEAR_CHECK_HOST if left out.
  checkHost?: string;
};

// The hosts a landing asked for, as check.sh would take them (an ssh host
// name each, comma-separated), or undefined for none.
const HOST_NAME = /^[A-Za-z0-9][A-Za-z0-9._@-]*$/;
function askedHosts(v: string | undefined): string[] {
  return (v ?? '').split(',').map((h) => h.trim()).filter((h) => HOST_NAME.test(h));
}
// A group's check runs in the holder's tb, with the holder's environment, so
// what the others asked for went nowhere: on 13 Nov a landing that set
// THREADBEAR_CHECK_HOST=cloud to keep off Tinker Desk's long builds was
// checked there, in another thread's group. Each entry keeps what its own
// tb asked for, and the group's check gets them all, in line order after the
// holder's, as THREADBEAR_CHECK_PREFER: scripts/check-remote.sh tries those
// hosts first and keeps the rest of its list behind them, so a group that
// needs a Mac still finds one.
export function checkPreference(own: string | undefined, others: (string | undefined)[]): string | undefined {
  const all = [...askedHosts(own), ...others.flatMap(askedHosts)];
  return all.length ? [...new Set(all)].join(',') : undefined;
}

// How many lines of a failed check's output are shown.
const CHECK_TAIL_LINES = 40;

// `tb land`: refuses with uncommitted changes, rebases on main, runs the
// check, fast-forwards main in its checkout and prints what landed.
// Returns the exit code. Every refusal leaves main as it was.
//
// One landing at a time holds the repo's lock, through its check and the
// fast-forward, so main only ever moves to a commit whose check passed. But
// the ones waiting don't each wait for a check of their own (12 Nov: a dozen
// in the line, 40 minutes and more each): a landing joins the queue
// (land-queue.ts) before it waits, and whoever holds the lock checks all
// that are waiting together with its own, stacked on main in the order they
// came, and lands them all at once. A failed group check is narrowed down by
// halves (docs/thread-worktrees.md, "Landing together").
export async function land(t: LandTarget, opts: LandOptions = {}): Promise<number> {
  const out = opts.out ?? ((l: string) => process.stdout.write(l + '\n'));
  const err = opts.err ?? ((l: string) => process.stderr.write(l + '\n'));
  const commonRel = await gitOut(t.path, ['rev-parse', '--git-common-dir']);
  if (!commonRel) {
    err(`${t.path} isn't a git checkout, so there's nothing to land.`);
    return 1;
  }
  const common = path.resolve(t.path, commonRel);
  const gitDir = path.resolve(t.path, (await gitOut(t.path, ['rev-parse', '--git-dir'])) ?? commonRel);
  const timesFile = path.join(common, LAND_TIMES_FILE);
  const asked = Date.now();

  // One tb land at a time in a worktree: it rebases before taking the
  // repo's lock now, and two at once would abort each other's rebase.
  let mineOnly: () => Promise<void>;
  try {
    const now = await holdLock(path.join(gitDir, 'threadbear-land-worktree.lock'), false);
    if (now) mineOnly = now;
    else {
      out('Another tb land is running in this worktree; waiting for it to finish...');
      mineOnly = (await holdLock(path.join(gitDir, 'threadbear-land-worktree.lock'), true))!;
    }
  } catch (e) {
    err(`Couldn't lock the worktree for landing: ${(e as Error).message}`);
    return 1;
  }
  let entry: QueueEntry | undefined;
  let entryHeld: (() => Promise<void>) | undefined;
  try {
    const unrecovered = await recoverDetached(t, gitDir, out, err);
    if (unrecovered !== undefined) return unrecovered;
    await recoverLandedFor(t, common, out);

    // Refusals, and conflicts with main as it is now, come straight away
    // rather than after the wait. It's done again with the lock held, as
    // main will have moved.
    const ready = await prepare(t, out, err, opts.despiteRed ? undefined : opts.hold);
    if (typeof ready === 'number') return ready;

    // A dry run, and a branch with a merge on it (it's merged with main,
    // never replayed onto others), are never put in someone else's check.
    // The entry carries the check this branch asks for: only landings that
    // ask for the same one are checked together. Its own lock says its tb
    // is still there (a pid can be reused; a lock dies with its holder).
    if (!opts.dryRun && !ready.hasMerge) {
      try {
        const check = opts.readCheck ? await opts.readCheck() : t.check;
        const checkHost = askedHosts(opts.checkHost ?? process.env.THREADBEAR_CHECK_HOST).join(',');
        entry = joinQueue(common, {
          branch: t.branch, path: t.path, base: ready.base, commit: ready.commit, check: check ?? null, checkFrom: opts.checkFrom ?? '',
          ...(checkHost ? { checkHost } : {}),
          ...(opts.despiteRed ? { fix: true } : {}),
        });
        entryHeld = await holdLock(entryLockFile(common, entry.id), false);
        if (!entryHeld) throw new Error('entry lock taken');
      } catch {
        if (entry) leaveQueue(common, entry.id);
        entry = undefined; // it still lands, by itself
      }
    }
    let release: () => Promise<void>;
    try {
      release = await lockLanding(path.join(common, 'threadbear-land.lock'), out, () => waitingLine(common, timesFile, entry));
    } catch (e) {
      err(`Couldn't lock the repo for landing: ${(e as Error).message}`);
      return 1;
    }
    try {
      const mine = entry && readEntry(common, entry.id);
      if (mine?.result) return await landedByAnother(t, mine, out);
      // An earlier tb of this branch, killed, may have landed while this one
      // waited.
      await recoverLandedFor(t, common, out);
      return await landLocked(t, opts, { file: timesFile, waitedMs: Date.now() - asked, entryId: entry?.id }, common, gitDir, mine?.limit === undefined ? undefined : { limit: mine.limit, at: mine.limitAt });
    } finally {
      await release();
    }
  } finally {
    if (entry) leaveQueue(common, entry.id);
    await entryHeld?.();
    if (entry) fs.rmSync(entryLockFile(common, entry.id), { force: true });
    await mineOnly();
  }
}

// What landed for a commit whose tb had gone by then (original commit ->
// landed tip), in the git common dir, so its next tb land moves its branch
// there before anything else, even if this one couldn't (the worktree was
// busy or had changes). Otherwise that tb land would rebase the original
// again, and a commit made empty on purpose would land twice.
const landedForDir = (common: string): string => path.join(common, 'threadbear-land-done');

function writeLandedFor(common: string, commit: string, landed: string): void {
  try {
    fs.mkdirSync(landedForDir(common), { recursive: true });
    fs.writeFileSync(path.join(landedForDir(common), commit), landed);
    // Old ones go: a branch moved on long since won't match them.
    const old = Date.now() - 7 * 24 * 3600_000;
    for (const f of fs.readdirSync(landedForDir(common))) {
      const file = path.join(landedForDir(common), f);
      if (fs.statSync(file).mtimeMs < old) fs.rmSync(file, { force: true });
    }
  } catch {
    // moveGoneBranch still tries.
  }
}

async function recoverLandedFor(t: LandTarget, common: string, out: (line: string) => void): Promise<void> {
  const head = await gitOut(t.path, ['rev-parse', 'HEAD']);
  if (!head || !/^[0-9a-f]{40,64}$/.test(head)) return;
  const file = path.join(landedForDir(common), head);
  let landed: string;
  try {
    landed = fs.readFileSync(file, 'utf8').trim();
  } catch {
    return;
  }
  // A note left by an older tb land for a commit that landed as it was:
  // the branch is already there.
  if (landed === head) {
    fs.rmSync(file, { force: true });
    return;
  }
  if ((await gitOut(t.path, ['rev-parse', '--abbrev-ref', 'HEAD'])) !== t.branch) return;
  if (!(await git(t.path, ['merge-base', '--is-ancestor', landed, `refs/heads/${MAIN}`])).ok) return;
  if ((await git(t.path, ['reset', '-q', '--keep', landed])).ok) {
    out(`An earlier tb land of ${t.branch} landed while its tb was stopped; the branch is moved to what landed.`);
    fs.rmSync(file, { force: true });
  }
}

// A landed entry's branch, moved to what landed for an owner that's gone,
// with that worktree's own tb land lock held, and only while it's clean and
// still on what it asked to land.
async function moveGoneBranch(e: QueueEntry, landed: string): Promise<void> {
  const gitDir = await gitOut(e.path, ['rev-parse', '--absolute-git-dir']);
  if (!gitDir) return;
  let release: (() => Promise<void>) | undefined;
  try {
    release = await holdLock(path.join(gitDir, 'threadbear-land-worktree.lock'), false);
  } catch {
    return;
  }
  if (!release) return; // a new tb land there sees to it
  try {
    if ((await gitOut(e.path, ['rev-parse', 'HEAD'])) === e.commit &&
        (await gitOut(e.path, ['rev-parse', '--abbrev-ref', 'HEAD'])) === e.branch &&
        (await gitOut(e.path, ['status', '--porcelain', '--untracked-files=all'])) === '') {
      // The note stays: another worktree on the same commit may still need
      // it (Codex's review, 13 Nov). Old ones go after a week.
      await git(e.path, ['reset', '-q', '--keep', landed]);
    }
  } finally {
    await release();
  }
}

const entryLockFile = (common: string, id: string): string => path.join(queueDir(common), `${id}.lock`);

// Whether an entry's tb is still waiting for its answer: it holds its
// entry's lock until it exits.
async function ownerWaiting(common: string, e: QueueEntry): Promise<boolean> {
  if (!fs.existsSync(entryLockFile(common, e.id))) return false;
  const now = readEntry(common, e.id);
  if (!now || now.result) return false;
  try {
    const free = await holdLock(entryLockFile(common, e.id), false);
    if (!free) return true;
    await free();
    return false;
  } catch {
    return false;
  }
}

// A group check runs with HEAD detached at the stack (landLocked). This
// marker, in the worktree's own git dir, says so while it runs; a tb killed
// partway leaves it, and the next tb land in the worktree (none can be
// running: it holds the worktree's lock) puts the branch back.
const detachedMarker = (gitDir: string): string => path.join(gitDir, 'threadbear-land-checking.json');

// Back on the branch from a group check's detached HEAD, `top` (the stack).
// Commits made there meanwhile go onto the branch first, as a rebase would
// carry them: on 13 Nov an agent committed its fix there while its group
// check ran, the next tb land checked the branch out, and the fix was left
// on no branch at all, silently, so the same check failed again. One that
// won't go on cleanly leaves HEAD where it is and says how to keep them.
// `top` is unknown only in a marker from before it was written down.
async function leaveStack(t: LandTarget, gitDir: string, own: string, top: string | undefined): Promise<{ ok: true; carried?: string } | { ok: false; reason: string }> {
  const head = await gitOut(t.path, ['rev-parse', 'HEAD']);
  const detached = (await gitOut(t.path, ['rev-parse', '--abbrev-ref', 'HEAD'])) === 'HEAD';
  if (!detached || !head) {
    const back = await git(t.path, ['checkout', '-q', t.branch]);
    return back.ok ? { ok: true } : { ok: false, reason: `${t.branch} couldn't be checked out in ${t.path}: ${firstLine(back.stderr || back.stdout)}` };
  }
  // A marker from before `top` was written down can't tell the stack from
  // commits made on it, so it never checks out over them (Codex, 13 Nov).
  if (!top) {
    return { ok: false, reason: `HEAD in ${t.path} is detached at ${head.slice(0, 9)}, and tb land can't tell whether commits were made on it. If any are yours, keep them (git log there, then git cherry-pick them onto ${t.branch}); then git checkout ${t.branch} there and run tb land again.` };
  }
  // The checkout, only from `from`: HEAD is read again just before, and the
  // reflog after says what it really moved from, as a commit can be made
  // in between (Codex, 13 Nov).
  const checkoutFrom = async (from: string, carried?: string): Promise<{ ok: true; carried?: string } | { ok: false; reason: string }> => {
    const pre = carried ? `${carried}\nBut ` : '';
    const now = await gitOut(t.path, ['rev-parse', 'HEAD']);
    if (now !== from) {
      return { ok: false, reason: `${pre}HEAD in ${t.path} moved to ${now?.slice(0, 9) ?? '?'} while tb land was putting ${t.branch} back, so it's left there. Keep anything of yours on it (git cherry-pick ${from.slice(0, 9)}..${now?.slice(0, 9) ?? 'HEAD'} onto ${t.branch}), then git checkout ${t.branch} and run tb land again.` };
    }
    const back = await git(t.path, ['checkout', '-q', t.branch]);
    if (!back.ok) return { ok: false, reason: `${pre}${t.branch} couldn't be checked out in ${t.path}: ${firstLine(back.stderr || back.stdout)}. Check it out there, then run tb land again.` };
    // This checkout's own entry, not whatever is newest by now (a hook's
    // commit after it, say): detached, git names where it moved from in full.
    const log = (await gitOut(t.path, ['reflog', 'show', '-n', '20', '--format=%gs', 'HEAD'])) ?? '';
    const left = /^checkout: moving from ([0-9a-f]{40,}) to /m.exec(log)?.[1];
    if (left !== from) {
      return { ok: false, reason: `${pre}A commit was made in ${t.path} just as ${t.branch} was checked out again, and it's on no branch now: ${left?.slice(0, 9) ?? 'see git reflog'}. Keep it with git cherry-pick ${from.slice(0, 9)}..${left?.slice(0, 9) ?? '<it>'}, then run tb land again.` };
    }
    return { ok: true, ...(carried ? { carried } : {}) };
  };
  if (head === top) return checkoutFrom(head);
  const keep = `Nothing was moved, so they're safe where HEAD is. To keep them: git checkout ${t.branch}, then git cherry-pick ${top.slice(0, 9)}..${head.slice(0, 9)} in ${t.path}, then run tb land again.`;
  const made = (await git(t.path, ['merge-base', '--is-ancestor', top, head])).ok
    ? await gitOut(t.path, ['rev-list', '--reverse', `${top}..${head}`])
    : undefined;
  if (made === undefined) {
    return { ok: false, reason: `HEAD in ${t.path} was moved off the checked group (${top.slice(0, 9)}) to ${head.slice(0, 9)} while tb land had it, so ${t.branch} wasn't checked out again, in case that has work of yours on it. Check out ${t.branch} there when it's safe, then run tb land again.` };
  }
  const list = made.split('\n').filter(Boolean);
  const merges = await gitOut(t.path, ['rev-list', '--merges', `${top}..${head}`]);
  const tip = merges === '' ? await pick(t.path, list, own) : undefined;
  const ids = (await gitOut(t.path, ['log', '--oneline', '--no-decorate', `${top}..${head}`])) ?? list.join('\n');
  if (!tip) {
    return { ok: false, reason: [`Commits were made in ${t.path} on top of the checked group, which isn't ${t.branch}, and they don't go onto ${t.branch} cleanly:`, ...ids.split('\n').map((l) => `  ${l}`), keep].join('\n') };
  }
  if (!(await git(t.path, ['update-ref', `refs/heads/${t.branch}`, tip, own])).ok) {
    return { ok: false, reason: [`Commits were made in ${t.path} on top of the checked group, and ${t.branch} couldn't be moved to take them:`, ...ids.split('\n').map((l) => `  ${l}`), keep].join('\n') };
  }
  // Those are on the branch now: if the checkout below stops, the next tb
  // land carries only what's past `head` (Codex, 13 Nov).
  try {
    if (fs.existsSync(detachedMarker(gitDir))) fs.writeFileSync(detachedMarker(gitDir), JSON.stringify({ branch: t.branch, own: tip, top: head }));
  } catch {
    // Then the next one stops and says what to keep, as with an old marker.
  }
  return checkoutFrom(head, `Commits made on the checked group while tb land had this worktree were moved onto ${t.branch}, as the group itself isn't yours to land:\n${ids.split('\n').map((l) => `  ${l}`).join('\n')}`);
}

// A number is the exit code to stop with: the branch couldn't be put back.
async function recoverDetached(t: LandTarget, gitDir: string, out: (line: string) => void, err: (line: string) => void): Promise<number | undefined> {
  let m: { branch?: string; own?: string; top?: string } | undefined;
  try {
    m = JSON.parse(fs.readFileSync(detachedMarker(gitDir), 'utf8'));
  } catch {
    return undefined;
  }
  if (m?.branch === t.branch && m.own &&
      (await gitOut(t.path, ['rev-parse', '--abbrev-ref', 'HEAD'])) === 'HEAD' &&
      (await gitOut(t.path, ['rev-parse', t.branch])) === m.own) {
    const r = await leaveStack(t, gitDir, m.own, typeof m.top === 'string' ? m.top : undefined);
    if (r.ok) {
      out(`An earlier tb land was stopped during a check with others' work; ${t.branch} is checked out again${r.carried ? '.' : ' as it was.'}`);
      if (r.carried) out(r.carried);
      fs.rmSync(detachedMarker(gitDir), { force: true });
      return undefined;
    }
    // Stop, keeping the marker, rather than land the branch without
    // commits made on the stack.
    err(`An earlier tb land was stopped during a check with others' work, and left HEAD detached at it. ${r.reason}`);
    return 1;
  }
  // Only once it no longer applies: on the branch again, or the branch
  // moved on by hand. Kept while it could still put things back.
  const on = await gitOut(t.path, ['rev-parse', '--abbrev-ref', 'HEAD']);
  if (on !== 'HEAD' || (await gitOut(t.path, ['rev-parse', t.branch])) !== m?.own) fs.rmSync(detachedMarker(gitDir), { force: true });
  return undefined;
}

// Another landing's check took this one with it. Its branch is moved to
// the commits as they landed (rebased on the ones ahead), unless it's
// moved on since it asked: then the next tb land rebases what's new.
async function landedByAnother(t: LandTarget, e: QueueEntry, out: (line: string) => void): Promise<number> {
  const r = e.result!;
  const head = await gitOut(t.path, ['rev-parse', 'HEAD']);
  const on = await gitOut(t.path, ['rev-parse', '--abbrev-ref', 'HEAD']);
  const clean = (await gitOut(t.path, ['status', '--porcelain', '--untracked-files=all'])) === '';
  out(`Landed on ${MAIN} in ${t.mainCheckout}, in one check with ${r.together.join(', ')}:`);
  out(r.commits);
  if (head === e.commit && on === t.branch && clean && (await git(t.path, ['reset', '-q', '--keep', r.landed])).ok) return 0;
  out(`${t.branch} has moved on since, so it's left as it is; the next tb land lands only what's new.`);
  return 0;
}

// Advisory locks are released by the OS, including when tb crashes. The
// holder exits when its parent's pipe closes; no PID files to go stale or
// delete under another waiter. Keep the lock file itself between runs.
// While it waits it says where it is in the line, and again every few
// minutes.
// An OS lock on `file`, held by a small child until released or this
// process exits; undefined if `wait` is false and someone else has it.
async function holdLock(file: string, wait: boolean): Promise<(() => Promise<void>) | undefined> {
  const holder = "process.stdin.resume(); process.stdin.on('end', () => process.exit(0)); process.stdout.write('locked');";
  const mac = process.platform === 'darwin';
  // 75 is EX_TEMPFAIL for lockf, and explicitly chosen for flock. A
  // nonblocking try distinguishes contention from a slow Node startup.
  const args = mac ? ['-k', ...(wait ? [] : ['-t', '0'])] : ['-x', '-E', '75', ...(wait ? [] : ['-n'])];
  const child = spawn(mac ? '/usr/bin/lockf' : 'flock', [...args, file, process.execPath, '-e', holder], {
    stdio: ['pipe', 'pipe', 'pipe'],
  });
  const closed = new Promise<void>((resolve) => child.once('close', () => resolve()));
  let errors = '';
  child.stderr.on('data', (d) => { errors += d; });
  const acquired = await new Promise<boolean>((resolve, reject) => {
    child.once('error', reject);
    child.once('close', (code) => {
      if (!wait && code === 75) resolve(false);
      else reject(new Error(errors.trim() || `landing lock exited (${code})`));
    });
    child.stdout.once('data', () => resolve(true));
  });
  if (!acquired) return undefined;
  return async () => {
    child.stdin.end();
    await closed;
  };
}

async function lockLanding(file: string, out: (line: string) => void, where: () => string): Promise<() => Promise<void>> {
  const take = (wait: boolean) => holdLock(file, wait);
  const release = await take(false);
  if (release) return release;
  const say = (first: boolean) => {
    let line: string;
    try {
      line = where();
    } catch {
      line = '...';
    }
    out(`${first ? 'Waiting for another landing of this repo to finish' : 'Still waiting to land'}${line}`);
  };
  say(true);
  const timer = setInterval(() => say(false), landLimits.progressMs);
  try {
    return (await take(true))!;
  } finally {
    clearInterval(timer);
  }
}

// How often a waiting landing says where it is. Tests shorten it.
export const landLimits = { progressMs: 3 * 60_000 };

// A passing check prints nothing of its own, so on 9 Nov finding out where
// landings spent their time meant scraping transcripts and running timed
// checks by hand, and a test that only passed on check.sh's second run went
// unseen. So each check leaves a line in `threadbear-land-times.log`, beside
// the landing lock (docs/thread-worktrees.md, "Where a landing's time goes").
export const LAND_TIMES_MAX_BYTES = 1024 * 1024;
export const LAND_TIMES_KEEP = 1000;

type CheckStep = { name: string; status: 'ok' | 'FAIL' | 'skip'; seconds?: number; secondRun?: string[] };

// What a check's output says about where its time went, from check.sh's and
// check-remote.sh's own lines. Another repo's check just has no steps.
export function parseCheckTimes(text: string): { host?: string; copySeconds?: number; steps: CheckStep[] } {
  const found: { host?: string; copySeconds?: number; steps: CheckStep[] } = { steps: [] };
  let last: CheckStep | undefined;
  for (const line of text.split('\n')) {
    const host = /^Checking on (\S+) \((?:copied in (\d+)s)?/.exec(line);
    if (host) {
      // A run starting on a host: any steps before it are from a host that
      // dropped out partway (check-remote.sh goes to the next), not this run's.
      found.host = host[1];
      delete found.copySeconds;
      if (host[2] !== undefined) found.copySeconds = Number(host[2]);
      found.steps = [];
      last = undefined;
      continue;
    }
    // No host could take it, so check.sh runs it here (its own words): the
    // steps that follow weren't on a host, and the updater mustn't read
    // them as if they were (update.ts, passedLandCheck).
    if (/^So the checks run on this Mac instead\b/.test(line)) {
      delete found.host;
      delete found.copySeconds;
      found.steps = [];
      last = undefined;
      continue;
    }
    const step = /^(ok|FAIL|skip)\s+(\S.*?)\s+(?:(\d+)s\b|\()(.*)$/.exec(line);
    if (step) {
      last = { name: step[2], status: step[1] as CheckStep['status'] };
      if (step[3] !== undefined) last.seconds = Number(step[3]);
      if (/passed on a second run/.test(step[4])) last.secondRun = [];
      found.steps.push(last);
      continue;
    }
    const again = /^\s+✖ (.*?)(?: \([\d.]+ms\))?$/.exec(line);
    if (again && last?.secondRun) last.secondRun.push(again[1]);
  }
  return found;
}

// Never in a landing's way: anything that goes wrong here is only a line
// missing from a log. Not following a link and not waiting on a fifo, as
// opening one would hold the landing lock for good.
function logCheckTimes(file: string, entry: Record<string, unknown>): void {
  try {
    const { O_WRONLY, O_APPEND, O_CREAT, O_NOFOLLOW, O_NONBLOCK } = fs.constants;
    const fd = fs.openSync(file, O_WRONLY | O_APPEND | O_CREAT | O_NOFOLLOW | O_NONBLOCK, 0o644);
    try {
      if (!fs.fstatSync(fd).isFile()) return;
      fs.writeSync(fd, `${JSON.stringify(entry)}\n`);
      if (fs.fstatSync(fd).size <= LAND_TIMES_MAX_BYTES) return;
    } finally {
      fs.closeSync(fd);
    }
    // Only one landing at a time gets here: the landing lock is held.
    const kept = fs.readFileSync(file, 'utf8').trimEnd().split('\n').slice(-LAND_TIMES_KEEP);
    fs.writeFileSync(file, `${kept.join('\n')}\n`);
  } catch {
    // See above.
  }
}

// Steps 1 and 2: the worktree's on its branch and clean, the branch is on
// main (rebased, or main merged in), there's something to land and its
// Important: lines will be read. A number is the exit code to stop with.
async function prepare(
  t: LandTarget,
  out: (line: string) => void,
  err: (line: string) => void,
  hold?: LandOptions['hold'],
): Promise<number | { hasMerge: boolean; base: string; commit: string; commits: string }> {
  const fail = (...lines: string[]): number => {
    err(lines.join('\n'));
    return 1;
  };

  const head = await gitOut(t.path, ['rev-parse', '--abbrev-ref', 'HEAD']);
  if (head === undefined) return fail(`${t.path} isn't a git checkout, so there's nothing to land.`);
  if (head !== t.branch) return fail(`The worktree at ${t.path} is on ${head}, not ${t.branch}. Check out ${t.branch} there, then run tb land again.`);
  const notReady = await mainNotReady(t.mainCheckout);
  if (notReady) return fail(notReady);

  // 1. Nothing uncommitted: it wouldn't land, and a rebase could trip on it.
  const status = await gitOut(t.path, ['status', '--porcelain', '--untracked-files=all']);
  if (status === undefined) return fail(`Couldn't read git status in ${t.path}.`);
  if (status) {
    return fail(
      `There are uncommitted changes in ${t.path}, so nothing landed:`,
      ...status.split('\n').map((l) => `  ${l}`),
      'Commit them (or remove them), then run tb land again.',
    );
  }

  // 2. Rebase on main. A conflict is undone, and the files named. A branch
  // with a merge on it (Mike's release, brought in: docs/bringing-in-updates.md)
  // is never rebased, as that flattens the merge and rewrites his commits,
  // and then the copy never looks up to date with his release again. It
  // merges main in instead, when main has moved.
  // Unreadable (git timed out) is a failure, never "no merges": that would
  // rebase a merge flat (Codex's third look, 9 Nov).
  const base = await gitOut(t.path, ['rev-parse', MAIN]);
  if (!base) return fail(`Couldn't read ${MAIN} in ${t.path}, so nothing landed. Run tb land again.`);
  const merges = await gitOut(t.path, ['rev-list', '--merges', `${base}..${t.branch}`]);
  if (merges === undefined) return fail(`Couldn't read ${t.branch}'s history in ${t.path}, so nothing landed. Run tb land again.`);
  const hasMerge = Boolean(merges);
  const behind = !(await git(t.path, ['merge-base', '--is-ancestor', base, t.branch])).ok;
  if (hasMerge && behind) {
    const merged = await git(t.path, ['merge', '--no-edit', MAIN]);
    if (!merged.ok) {
      const conflicted = (await gitOut(t.path, ['diff', '--name-only', '--diff-filter=U'])) ?? '';
      await git(t.path, ['merge', '--abort']);
      if (conflicted) {
        return fail(
          `Merging ${MAIN} into ${t.branch} (it has a merge on it, so it isn't rebased) ran into conflicts in:`,
          ...conflicted.split('\n').map((f) => `  ${f}`),
          `The merge was undone, so ${t.branch} is as it was and ${MAIN} wasn't touched. Run git merge ${MAIN} in ${t.path}, sort out those files, commit, then run tb land again.`,
        );
      }
      return fail(`git merge ${MAIN} failed in ${t.path}, so nothing landed: ${firstLine(merged.stderr || merged.stdout)}`);
    }
  } else if (!hasMerge) {
    const rebase = await git(t.path, ['rebase', base]);
    if (!rebase.ok) {
      const conflicted = (await gitOut(t.path, ['diff', '--name-only', '--diff-filter=U'])) ?? '';
      await git(t.path, ['rebase', '--abort']);
      if (conflicted) {
        return fail(
          `Rebasing ${t.branch} on ${MAIN} ran into conflicts in:`,
          ...conflicted.split('\n').map((f) => `  ${f}`),
          `The rebase was undone, so ${t.branch} is as it was and ${MAIN} wasn't touched. Run git rebase ${MAIN} in ${t.path}, sort out those files, finish the rebase, then run tb land again.`,
        );
      }
      return fail(`git rebase ${MAIN} failed in ${t.path}, so nothing landed: ${firstLine(rebase.stderr || rebase.stdout)}`);
    }
  }

  // Unreadable is a failure, not "nothing to land".
  const commits = await gitOut(t.path, ['log', '--oneline', '--first-parent', `${base}..${t.branch}`]);
  if (commits === undefined) return fail(`Couldn't read what ${t.branch} has to land in ${t.path}, so nothing landed. Run tb land again.`);
  if (!commits) {
    out(`Nothing to land: ${t.branch} has no commits that aren't on ${MAIN}.`);
    return 0;
  }

  // 2b. An `Important:` line git doesn't read as a trailer is refused: the
  // release collects them with git's trailer parser (upstream.ts), which
  // only looks in a message's last paragraph, so one set apart from the
  // Co-Authored-By block flags nothing and forks never hear (9 Nov: seven
  // security fixes went out that way). Merged-in commits (a fork bringing
  // in Mike's release) aren't this branch's own, so first-parent only.
  const unread = await unreadImportant(t.path, `${base}..${t.branch}`);
  if (unread === undefined) return fail(`Couldn't read ${t.branch}'s commit messages in ${t.path}, so nothing landed. Run tb land again.`);
  if (unread.length) {
    return fail(
      `These commits have an Important: line that git doesn't read as a trailer, so a release wouldn't flag them and nothing landed:`,
      ...unread.map((l) => `  ${l}`),
      'Put the Important: line in the same last paragraph as Co-Authored-By (no blank line between them), or at the end of the message if there is none. Use git commit --amend or git rebase to reword, then run tb land again.',
    );
  }
  // 2c. Held while something it would set off on GitHub is red on main.
  if (hold) {
    const held = await holdFor(t.path, base, t.branch, hold);
    if (held) return fail(held);
  }

  const commit = await gitOut(t.path, ['rev-parse', 'HEAD']);
  if (!commit) return fail(`Couldn't read HEAD in ${t.path}.`);
  return { hasMerge, base, commit, commits };
}

// Why what `from...to` changes can't land now (opts.hold), or undefined.
// NUL-separated: git quotes a name with other letters in it otherwise
// ("desktop/caf\303\251.rs"), which no path filter matches.
async function holdFor(cwd: string, from: string, to: string, hold: NonNullable<LandOptions['hold']>): Promise<string | undefined> {
  const changed = await git(cwd, ['diff', '--name-only', '-z', `${from}...${to}`]);
  if (!changed.ok) return `Couldn't read what ${to} changes in ${cwd}, so nothing landed. Run tb land again.`;
  return hold(cwd, changed.stdout.split('\0').filter(Boolean));
}

// Why a group's files can't land now: what each member's commits change
// as they're stacked (replayed, so a fix that moved a file ahead of it is
// followed to where the change really lands), and this one's on top,
// except a fix's: that's the repair for what's red.
async function groupHeld(
  t: LandTarget,
  main: string,
  group: { entry: QueueEntry; tip: string }[],
  top: string,
  opts: LandOptions,
): Promise<string | undefined> {
  if (!opts.hold) return undefined;
  const ranges: [string, string][] = [];
  let prev = main;
  for (const g of group) {
    if (!g.entry.fix) ranges.push([prev, g.tip]);
    prev = g.tip;
  }
  if (!opts.despiteRed) ranges.push([prev, top]);
  const files = new Set<string>();
  for (const [from, to] of ranges) {
    const changed = await git(t.path, ['diff', '--name-only', '-z', `${from}..${to}`]);
    if (!changed.ok) return `Couldn't read what ${to} changes in ${t.path}, so nothing landed. Run tb land again.`;
    for (const f of changed.stdout.split('\0').filter(Boolean)) files.add(f);
  }
  return files.size ? opts.hold(t.path, [...files]) : undefined;
}

// `from..to`'s commits (first parent, no merges) made again on `onto`, as a
// rebase would, without touching any working tree: a three-way merge of
// each one's change (git merge-tree), then a commit with its author and
// message. A change that's already there is dropped, as rebase drops it.
// Undefined on a conflict or if git can't say.
async function replay(cwd: string, from: string, to: string, onto: string): Promise<string | undefined> {
  // As rebase picks them: `from` is on `onto`, so `onto...to`'s right side
  // is from..to, less any whose change is already on `onto` (a cherry-pick
  // of it, say), as rebase leaves those out.
  if (!(await git(cwd, ['merge-base', '--is-ancestor', from, onto])).ok) return undefined;
  // A commit made empty is never one of those, as rebase keeps it.
  const marked = await gitOut(cwd, ['rev-list', '--reverse', '--right-only', '--cherry-mark', '--no-merges', `${onto}...${to}`]);
  if (marked === undefined) return undefined;
  const list: string[] = [];
  for (const line of marked.split('\n').filter(Boolean)) {
    const c = line.slice(1);
    if (line[0] === '=' && (await gitOut(cwd, ['rev-parse', `${c}^{tree}`])) !== (await gitOut(cwd, ['rev-parse', `${c}^^{tree}`]))) continue;
    list.push(c);
  }
  return pick(cwd, list, onto);
}

// Commits `list`, oldest first, made again on `onto` without touching any
// worktree, as rebase would; undefined if one doesn't go on cleanly.
async function pick(cwd: string, list: string[], onto: string): Promise<string | undefined> {
  let tip = onto;
  for (const c of list) {
    const merged = await git(cwd, ['merge-tree', '--write-tree', `--merge-base=${c}^`, tip, c]);
    if (!merged.ok) return undefined;
    const tree = merged.stdout.split('\n')[0].trim();
    // A commit made empty is kept, as rebase keeps it; one that only
    // becomes empty here is dropped, as rebase drops it.
    const wasEmpty = (await gitOut(cwd, ['rev-parse', `${c}^{tree}`])) === (await gitOut(cwd, ['rev-parse', `${c}^^{tree}`]));
    if (!wasEmpty && tree === (await gitOut(cwd, ['rev-parse', `${tip}^{tree}`]))) continue;
    const meta = await gitOut(cwd, ['log', '-1', '--format=%an%x00%ae%x00%ad', '--date=raw', c]);
    const message = await git(cwd, ['log', '-1', '--format=%B', c]);
    if (!meta || !message.ok) return undefined;
    const [name, email, date] = meta.split('\0');
    const made = await gitOut(cwd, ['commit-tree', tree, '-p', tip, '-m', message.stdout.replace(/\n+$/, '')], {
      GIT_AUTHOR_NAME: name,
      GIT_AUTHOR_EMAIL: email,
      GIT_AUTHOR_DATE: date,
    });
    if (!made) return undefined;
    tip = made;
  }
  return tip;
}

// The others waiting, stacked on `main` first come first, as far as
// `limit` allows (counting this one). One that has moved on since it asked,
// or doesn't go on cleanly, is left waiting: its own tb land sees to it.
async function stackWaiting(
  t: LandTarget,
  common: string,
  main: string,
  limit: number,
  check: string | null,
  checkFrom: string,
  withFixes = true,
  includeOthers = true,
): Promise<{ tip: string; group: { entry: QueueEntry; tip: string; commits: string }[] }> {
  let tip = main;
  let cap = limit;
  let counted = 0;
  const group: { entry: QueueEntry; tip: string; commits: string }[] = [];
  // A fix (tb land --despite-red: the repair for a main that's red) goes
  // first and isn't counted against the limit, so the halving never leaves
  // it out. On 13 Nov a group behind a broken main was halved down three
  // checks, each failing on main's own error, while the one-line repair
  // waited behind it for a group of its own.
  const all = waiting(common);
  const ordered = withFixes ? [...all.filter((e) => e.fix), ...all.filter((e) => !e.fix)] : all.filter((e) => !e.fix);
  for (const e of ordered) {
    if (e.pid === process.pid && e.path === t.path) continue; // this one
    const c = e.fix ? Infinity : Math.min(cap, e.limit !== undefined && e.limitAt === main ? e.limit : Infinity);
    if (!e.fix && counted + 2 > c) continue;
    if (!e.fix && !includeOthers) continue;
    if (e.path === t.path || e.branch === t.branch) continue;
    if ((e.check ?? null) !== check || (e.checkFrom ?? '') !== checkFrom) continue;
    if (!(await ownerWaiting(common, e))) continue;
    if ((await gitOut(e.path, ['rev-parse', 'HEAD'])) !== e.commit) continue;
    if (!(await git(t.path, ['merge-base', '--is-ancestor', e.base, main])).ok) continue;
    if ((await gitOut(t.path, ['rev-list', '--merges', `${e.base}..${e.commit}`])) !== '') continue;
    const next = await replay(t.path, e.base, e.commit, tip);
    if (!next || next === tip) continue;
    const commits = await gitOut(t.path, ['log', '--oneline', '--first-parent', `${tip}..${next}`]);
    if (commits === undefined) continue;
    group.push({ entry: e, tip: next, commits });
    tip = next;
    if (!e.fix) {
      counted++;
      cap = c;
    }
  }
  return { tip, group };
}

async function landLocked(
  t: LandTarget,
  opts: LandOptions,
  times: { file: string; waitedMs: number; entryId?: string },
  common: string,
  gitDir: string,
  startLimit: { limit: number; at?: string } | undefined,
): Promise<number> {
  const out = opts.out ?? ((l: string) => process.stdout.write(l + '\n'));
  const err = opts.err ?? ((l: string) => process.stderr.write(l + '\n'));
  const fail = (...lines: string[]): number => {
    err(lines.join('\n'));
    return 1;
  };

  // Steps 1 and 2 again: main has likely moved while this waited.
  const ready = await prepare(t, out, err, opts.despiteRed ? undefined : opts.hold);
  if (typeof ready === 'number') return ready;
  const own = ready.commit;
  const main = ready.base;
  // A limit from a failed group check only holds while main is where it
  // was: a failure on an old main (13 Nov: its own compile error) says
  // nothing about a new one, and kept every later landing checked alone.
  let limit = startLimit && startLimit.at === main ? startLimit.limit : Infinity;
  let withFixes = true;

  for (;;) {
    // 3. The others waiting go on main first, then this branch on top. The
    // check runs here, on that stack, with HEAD detached at it: the branch
    // itself only moves once it has all landed, so a tb killed partway never
    // leaves other threads' unlanded work on it.
    let checked = own;
    let group: { entry: QueueEntry; tip: string; commits: string }[] = [];
    let check: string | undefined;
    try {
      check = opts.readCheck ? await opts.readCheck() : t.check;
    } catch (e) {
      return fail(`Couldn't read the repo's patches for its check: ${(e as Error).message}`);
    }
    if (!opts.dryRun && !ready.hasMerge) {
      let stacked = await stackWaiting(t, common, main, limit, check ?? null, opts.checkFrom ?? '', withFixes);
      let top = stacked.group.length ? await replay(t.path, main, own, stacked.tip) : undefined;
      // Held on GitHub for what the others change (not a fix's, which is
      // the repair): then only the fixes go with it, and the others go by
      // themselves later, each hearing why. Still held with only the fixes
      // (this one's own change, moved by one): it goes alone.
      if (opts.hold && top && (await groupHeld(t, main, stacked.group, top, opts))) {
        stacked = await stackWaiting(t, common, main, limit, check ?? null, opts.checkFrom ?? '', withFixes, false);
        top = stacked.group.length ? await replay(t.path, main, own, stacked.tip) : undefined;
        if (top && (await groupHeld(t, main, stacked.group, top, opts))) top = undefined;
      }
      if (stacked.group.length) {
        if (top && top !== stacked.tip) {
          try {
            fs.writeFileSync(detachedMarker(gitDir), JSON.stringify({ branch: t.branch, own, top }));
          } catch {
            // Only recovery after a crash reads it.
          }
          if ((await git(t.path, ['checkout', '-q', '--detach', top])).ok) {
            checked = top;
            group = stacked.group;
          } else {
            fs.rmSync(detachedMarker(gitDir), { force: true });
          }
        }
      }
    }
    const together = group.map((g) => g.entry.branch);
    // Back on the branch, as it was, after a group check that didn't land.
    // Commits made on the stack meanwhile go onto the branch (leaveStack),
    // and then this run stops rather than land without them: `carried`.
    let stuck = `${t.path} is left with HEAD detached at the checked group; run git checkout ${t.branch} there.`;
    let carried = false;
    const back = async (): Promise<boolean> => {
      if (!group.length) return true;
      const r = await leaveStack(t, gitDir, own, checked);
      if (!r.ok) {
        stuck = r.reason;
        return false;
      }
      fs.rmSync(detachedMarker(gitDir), { force: true });
      if (r.carried) {
        carried = true;
        out(r.carried);
      }
      return true;
    };
    const committedMeanwhile = `Commits were made in ${t.path} while it was being checked, so nothing landed; they're on ${t.branch} now. Wait for tb land to finish before committing there, and run tb land again to land them.`;

    try {
      // The stack has the others' patches too, so read it again there.
      if (group.length) check = opts.readCheck ? await opts.readCheck() : t.check;
    } catch (e) {
      await back();
      return fail(`Couldn't read the repo's patches for its check: ${(e as Error).message}`);
    }
    if (check) {
      out(group.length
        ? `Running the check, together with ${together.join(', ')}: ${check}`
        : `Running the check: ${check}`);
      setRunning(common, {
        startedAt: Date.now(), branches: [t.branch, ...together], pid: process.pid,
        entries: [...(times.entryId ? [times.entryId] : []), ...group.map((g) => g.entry.id)],
      });
      const prefer = group.length
        ? checkPreference(opts.checkHost ?? process.env.THREADBEAR_CHECK_HOST, group.map((g) => g.entry.checkHost))
        : undefined;
      // THREADBEAR_CHECK_GROUP says it's a group's check even when nobody
      // asked for a host, so check.sh never takes the holder's own (an empty
      // one would run everyone's check on this Mac; Codex's second review).
      const r = await runCheck(check, t.path, opts.shell, group.length ? { THREADBEAR_CHECK_GROUP: '1', ...(prefer ? { THREADBEAR_CHECK_PREFER: prefer } : {}) } : {});
      setRunning(common, undefined);
      // Whether the check ran on the commit it's logged for: asked before the
      // line is written, as the runner's updater takes a passing line as
      // proof for that commit (update.ts, landChecked), and a check of files
      // edited meanwhile proves nothing about it (Codex, 13 Nov). The same
      // questions are asked again below, where they stop the landing.
      const intact = r.ok &&
        (await gitOut(t.path, ['rev-parse', 'HEAD'])) === checked &&
        (await gitOut(t.path, ['status', '--porcelain', '--untracked-files=all'])) === '';
      logCheckTimes(times.file, {
        at: new Date().toISOString(),
        branch: t.branch,
        ...(group.length ? { together } : {}),
        commit: checked,
        ok: r.ok,
        ...(r.ok ? {} : { exit: r.code }),
        seconds: Math.round(r.durationMs / 1000),
        waitedSeconds: Math.round(times.waitedMs / 1000),
        ...(opts.dryRun ? { dryRun: true } : {}),
        ...parseCheckTimes(r.text),
        // After what the check printed, so nothing it prints can be these.
        check,
        ...(intact ? { intact: true } : {}),
      });
      if (!r.ok && group.length) {
        // Show the group's failure now: finding the individual landing can
        // take several more checks, and its logs are already available.
        out([
          `The check failed with ${group.length + 1} landings together${r.code !== undefined ? ` (exit ${r.code})` : ''}, so ${MAIN} wasn't touched. The last lines of its output:`,
          'Wait for tb land to finish before editing this worktree.',
          '',
          r.text.trimEnd().split('\n').slice(-CHECK_TAIL_LINES).join('\n').slice(-8000),
        ].join('\n'));
        // One of them (or two together) broke it. Halve the group and go
        // again, this one with the first half; the rest wait their turn.
        // No individual landing is refused until its own check fails.
        if (!(await back())) return fail(`The check failed with ${group.length + 1} landings together, so nothing landed. ${stuck}`);
        if (carried) return fail(committedMeanwhile);
        // Fixes always come along, so once it's down to this one and the
        // fixes, the fixes are left out next, to see whose it is.
        const counted = group.filter((g) => !g.entry.fix).length;
        if (counted === 0) withFixes = false;
        limit = Math.max(1, Math.ceil((counted + 1) / 2));
        for (const g of group) {
          const now = readEntry(common, g.entry.id);
          if (now && !now.result) writeEntry(common, { ...now, limit: Math.min(now.limitAt === main ? now.limit ?? Infinity : Infinity, limit), limitAt: main });
        }
        out('Checking them in smaller groups to find which.');
        continue;
      }
      if (!r.ok) {
        const tail = r.text.trimEnd().split('\n').slice(-CHECK_TAIL_LINES);
        return fail(
          `The check failed${r.code !== undefined ? ` (exit ${r.code})` : ''}, so ${MAIN} wasn't touched. The last lines of its output:`,
          '',
          ...tail,
          '',
          'Fix it, commit, and run tb land again.',
        );
      }
      out(`The check passed (${Math.round(r.durationMs / 1000)} s).`);
    }

    if ((await gitOut(t.path, ['rev-parse', 'HEAD'])) !== checked ||
        (await gitOut(t.path, ['rev-parse', t.branch])) !== own ||
        (await gitOut(t.path, ['rev-parse', '--abbrev-ref', 'HEAD'])) !== (group.length ? 'HEAD' : t.branch) ||
        (await gitOut(t.path, ['status', '--porcelain', '--untracked-files=all'])) !== '') {
      if (group.length && (await gitOut(t.path, ['rev-parse', t.branch])) === own &&
          (await gitOut(t.path, ['status', '--porcelain', '--untracked-files=all'])) === '') {
        if (!(await back())) return fail(`The branch or files in ${t.path} changed while the check ran, so nothing landed. ${stuck}`);
        if (carried) return fail(committedMeanwhile);
      }
      return fail(`The branch or files in ${t.path} changed while the check ran, so nothing landed.${group.length ? ` (The check was of ${t.branch} with ${together.join(', ')}; HEAD was detached at it, so check out ${t.branch} if it still is.)` : ''} Commit the changes and run tb land again.`);
    }

    // One whose tb went during the check (killed: on 12 Nov session
    // restarts killed waiting tb lands again and again, and checking again
    // without each one meant two passing group checks in a row landed
    // nothing) still lands, as long as its branch is still what it asked to
    // land: that's what it asked for. Its next tb land then has nothing to
    // land. One whose branch has moved on is checked again without.
    if (group.length) {
      const movedOn = [];
      for (const g of group) {
        if (await ownerWaiting(common, g.entry)) continue;
        if ((await gitOut(g.entry.path, ['rev-parse', 'HEAD'])) !== g.entry.commit) movedOn.push(g.entry.branch);
      }
      if (movedOn.length) {
        if (!(await back())) return fail(`${movedOn.join(', ')} stopped waiting and moved on during the check, so nothing landed. ${stuck}`);
        if (carried) return fail(committedMeanwhile);
        out(`${movedOn.join(', ')} stopped waiting and moved on during the check, so it's checked again without ${movedOn.length > 1 ? 'them' : 'it'}.`);
        continue;
      }
    }

    // Asked again now the check is done: a run that was going when it
    // started may have failed since. Others in the group wait their turn.
    if (opts.hold && !opts.dryRun) {
      const held = group.length ? await groupHeld(t, main, group, checked, opts) : opts.despiteRed ? undefined : await holdFor(t.path, main, checked, opts.hold);
      if (held) {
        if (!(await back())) err(stuck);
        return fail(held);
      }
    }

    if (opts.dryRun) {
      out(`Would land on ${MAIN} in ${t.mainCheckout}:`);
      out(ready.commits);
      return 0;
    }

    // 4. Fast-forward main where it's checked out. Asked again first: the
    // check took minutes, and on 12 Nov a rebase someone started in the
    // main checkout meanwhile had HEAD detached, so the merge moved that
    // instead of main, and the rebase's end set main without it.
    const unready = await mainNotReady(t.mainCheckout);
    if (unready) {
      if (!(await back())) err(stuck);
      return fail(unready);
    }
    const before = await gitOut(t.mainCheckout, ['rev-parse', `refs/heads/${MAIN}`]);
    const merge = await git(t.mainCheckout, ['merge', '--ff-only', checked]);
    if (!merge.ok) {
      if (!(await back())) err(stuck);
      const text = merge.stderr + merge.stdout;
      const files = overwrittenFiles(text);
      if (files.length) {
        return fail(
          `The main checkout at ${t.mainCheckout} has uncommitted changes in files this would change, so nothing landed:`,
          ...files.map((f) => `  ${f}`),
          `Commit or move those changes there (someone else may be working on them), then run tb land again.`,
        );
      }
      if (/fast-forward/i.test(text)) return fail(`${MAIN} moved while this ran, so nothing landed. Run tb land again.`);
      return fail(`git merge --ff-only ${t.branch} failed in ${t.mainCheckout}, so nothing landed: ${firstLine(text)}`);
    }
    // And it's main that moved, as its reflog says, not some other HEAD.
    const after = await gitOut(t.mainCheckout, ['rev-parse', `refs/heads/${MAIN}`]);
    const logged = await gitOut(t.mainCheckout, ['reflog', '-1', '--format=%H', `refs/heads/${MAIN}`]);
    const onMain = await gitOut(t.mainCheckout, ['symbolic-ref', '-q', 'HEAD']);
    if (after !== checked || logged !== checked || onMain !== `refs/heads/${MAIN}`) {
      if (!(await back())) err(stuck);
      return fail(
        `git merge --ff-only in ${t.mainCheckout} finished, but ${MAIN} is at ${after?.slice(0, 9) ?? 'nothing git can read'} (was ${before?.slice(0, 9) ?? '?'}), not the checked ${checked.slice(0, 9)}, and HEAD there is ${onMain || 'detached'}. Nothing landed.`,
        `Something else is working in the main checkout. Look at git status and git reflog there, then run tb land again.`,
      );
    }

    // 5. What landed. The others hear from their entries; this branch moves
    // to its commits as they landed.
    let mine = ready.commits;
    if (group.length) {
      for (const g of group) {
        const now = readEntry(common, g.entry.id);
        if (now) writeEntry(common, { ...now, result: { landed: g.tip, commits: g.commits, together: [t.branch, ...together.filter((b) => b !== g.entry.branch)] } });
        // One whose tb has gone never hears: move its branch here, as it
        // would have, or its next tb land would land a commit made empty
        // on purpose a second time (rebase keeps those).
        // Not when it landed as it was (first in the stack, so rebased to
        // the same commits, as happens within the same second): there's
        // nothing to move, and a note from a commit to itself made its next
        // tb land say it moved the branch (red on GitHub's Linux, 13 Nov).
        if (g.tip !== g.entry.commit && !(await ownerWaiting(common, g.entry))) {
          writeLandedFor(common, g.entry.commit, g.tip);
          await moveGoneBranch(g.entry, g.tip);
        }
      }
      // Through leaveStack, as a commit made since the check was looked at
      // would otherwise be checked out away.
      const moved = (await git(t.path, ['update-ref', `refs/heads/${t.branch}`, checked, own])).ok
        ? await leaveStack(t, gitDir, checked, checked)
        : undefined;
      if (moved?.ok) {
        fs.rmSync(detachedMarker(gitDir), { force: true });
        if (moved.carried) out(moved.carried);
      } else if (moved) err(`It landed, and ${t.branch} is on what landed. ${moved.reason}`);
      else err(`It landed, but ${t.branch} couldn't be moved to it in ${t.path}: HEAD is detached at what landed. Run git checkout ${t.branch} there and git reset --keep ${MAIN}.`);
      mine = (await gitOut(t.path, ['log', '--oneline', '--first-parent', `${group.at(-1)!.tip}..${checked}`])) ?? mine;
    }
    out(`Landed on ${MAIN} in ${t.mainCheckout}:`);
    out(mine);
    if (group.length) out(`In the same check, and landed with it: ${together.join(', ')}.`);
    return 0;
  }
}

// Why tb land can't move main in the main checkout now, or undefined: HEAD
// has to be main itself (not detached) with nothing half done there, as a
// rebase, merge or cherry-pick in progress would move a detached HEAD and
// then set main without what landed (12 Nov: a sync with GitHub, by hand,
// lost a landing that way).
export async function mainNotReady(mainCheckout: string): Promise<string | undefined> {
  const head = await gitOut(mainCheckout, ['symbolic-ref', '-q', 'HEAD']);
  const gitDirRel = await gitOut(mainCheckout, ['rev-parse', '--git-dir']);
  if (!gitDirRel) return `Git couldn't read the main checkout at ${mainCheckout}, so tb land can't move ${MAIN} there.`;
  const gitDir = path.resolve(mainCheckout, gitDirRel);
  const busy: [string, string][] = [
    ['rebase-merge', 'a rebase'],
    ['rebase-apply', 'a rebase'],
    ['MERGE_HEAD', 'a merge'],
    ['CHERRY_PICK_HEAD', 'a cherry-pick'],
    ['REVERT_HEAD', 'a revert'],
    ['BISECT_LOG', 'a bisect'],
    // A cherry-pick or revert of several, stopped: it's still going after
    // a hand commit takes CHERRY_PICK_HEAD away.
    ['sequencer', 'a cherry-pick or revert'],
  ];
  const found = busy.find(([f]) => fs.existsSync(path.join(gitDir, f)));
  if (found) {
    return `The main checkout at ${mainCheckout} is in the middle of ${found[1]} (${path.join(gitDir, found[0])}), so tb land can't move ${MAIN} there and nothing landed. Someone may be syncing it with GitHub: let them finish or abort it (git status there says which), then run tb land again.`;
  }
  if (head !== `refs/heads/${MAIN}`) {
    const what = head ? head.replace(/^refs\/heads\//, '') : 'a detached HEAD';
    return `The main checkout at ${mainCheckout} is on ${what}, not ${MAIN}, so tb land can't move ${MAIN} there.`;
  }
  return undefined;
}

// The files git names when a merge would overwrite local changes (or
// untracked files) in the checkout: the tab-indented lines after the
// "would be overwritten by merge:" line.
export function overwrittenFiles(text: string): string[] {
  const files: string[] = [];
  let listing = false;
  for (const line of text.split('\n')) {
    if (/would be overwritten by merge:?$/.test(line.trim())) listing = true;
    else if (listing && /^\s+\S/.test(line)) files.push(line.trim());
    else listing = false;
  }
  return files;
}

// Runs the check through a shell, keeping the end of its output (that's
// where a test runner says what failed).
function runCheck(command: string, cwd: string, shell = ['/bin/sh', '-c'], extraEnv: NodeJS.ProcessEnv = {}): Promise<{ ok: boolean; code?: number; text: string; durationMs: number }> {
  const started = Date.now();
  // At background priority: a land's check is the full quick suite.
  const [bin, args] = inBackground(shell[0], [...shell.slice(1), command]);
  const keep = 256 * 1024;
  return new Promise((resolve) => {
    let text = '';
    const take = (b: Buffer) => {
      text += b.toString('utf8');
      if (text.length > keep * 2) text = text.slice(-keep);
    };
    // Never one left over from whatever started this tb: only a group sets it.
    const env: NodeJS.ProcessEnv = { ...process.env, ...extraEnv };
    if (!extraEnv.THREADBEAR_CHECK_PREFER) delete env.THREADBEAR_CHECK_PREFER;
    if (!extraEnv.THREADBEAR_CHECK_GROUP) delete env.THREADBEAR_CHECK_GROUP;
    const child = spawn(bin, args, { cwd, env, stdio: ['ignore', 'pipe', 'pipe'] });
    child.stdout.on('data', take);
    child.stderr.on('data', take);
    child.on('error', (e) => resolve({ ok: false, text: `${text}\n${e.message}`, durationMs: Date.now() - started }));
    child.on('close', (code) => resolve({ ok: code === 0, code: code ?? undefined, text, durationMs: Date.now() - started }));
  });
}

// For `tb land` outside a thread (or in a thread without one of its own):
// the folder's checkout, if it's on a thread/ branch, and where main is.
// `threadId`, when tb runs in a thread: a branch made for another thread
// (thread/<its id8>) is refused, as that's another thread's work. On 8 Nov
// a borrowed worktree turned out to be shared with a thread brought back,
// and the borrower committed on that thread's branch without noticing.
// The top of the git checkout `cwd` is in, or undefined outside one.
export function checkoutTop(cwd: string): Promise<string | undefined> {
  return gitOut(cwd, ['rev-parse', '--show-toplevel']);
}

// A worktree Claude Code made for a sub-agent (Agent's worktree isolation,
// `claude --worktree`): `<repo>/.claude/worktrees/<name>` on
// `worktree-<name>`. The thread/ rule is there so a stray tb land doesn't
// put main, or a branch someone keeps by hand, onto main; these are made
// for one helper's work and thrown away, like a thread's own.
function isClaudeWorktree(top: string, branch: string): boolean {
  return branch === `worktree-${path.basename(top)}` && path.basename(path.dirname(top)) === 'worktrees' && path.basename(path.dirname(path.dirname(top))) === '.claude';
}

export async function landTargetFromGit(cwd: string, threadId?: string): Promise<{ target?: Omit<LandTarget, 'check'>; reason?: string }> {
  const top = await checkoutTop(cwd);
  if (!top) return { reason: `${cwd} isn't in a git checkout. Run tb land in a thread's worktree.` };
  let branch = await gitOut(top, ['rev-parse', '--abbrev-ref', 'HEAD']);
  // Detached by a tb land killed during a group check: its marker names the
  // branch, and land() puts it back.
  if (branch === 'HEAD') {
    const gitDir = await gitOut(top, ['rev-parse', '--absolute-git-dir']);
    try {
      const m = gitDir ? JSON.parse(fs.readFileSync(detachedMarker(gitDir), 'utf8')) as { branch?: unknown } : undefined;
      if (typeof m?.branch === 'string' && m.branch.startsWith('thread/') &&
          (await git(top, ['rev-parse', '--verify', '--quiet', `refs/heads/${m.branch}`])).ok) branch = m.branch;
    } catch {
      // No marker: detached some other way.
    }
  }
  if (branch === 'HEAD') {
    // A worktree borrowed before borrow-worktree.sh made a branch, or one
    // detached by hand.
    return { reason: `${top} is detached, not on a thread/ branch. tb land runs in a thread's own worktree; to land from this one, git checkout -b thread/<name> in it first.` };
  }
  if (!branch?.startsWith('thread/') && !(branch && isClaudeWorktree(top, branch))) {
    return { reason: `${top} is on ${branch ?? 'no branch'}, not a thread/ branch. tb land runs in a thread's own worktree.` };
  }
  const owner = /^thread\/([0-9a-f]{8})$/.exec(branch)?.[1];
  if (threadId && owner && owner !== threadId.slice(0, 8)) {
    return {
      reason:
        `${top} is on ${branch}, thread ${owner}'s branch, not this thread's. Another thread is working in this folder, so don't commit or land here: ` +
        `move your own commits to a branch of your own (git branch <name> <commit>), put this one back as it was, and borrow a worktree of your own. ` +
        `To land ${branch} anyway, run THREADBEAR_THREAD_ID= tb land.`,
    };
  }
  const mainCheckout = await mainCheckoutOf(top);
  if (!mainCheckout) return { reason: `No checkout of this repo has ${MAIN} checked out, so there's nowhere to land.` };
  return { target: { path: top, branch, mainCheckout } };
}

// ------------------------------------------------------------- cleaning up

// `taken`: the folder is another checkout's now (another thread's worktree,
// a spare or a borrowed one), so it was left alone; only this thread's
// landed branch, if it was still there, went.
// `branchLeft`: the folder went but its branch couldn't be deleted. Still
// removed, as the folder can be made again for someone else (Codex, 8 Nov).
export type Cleanup = { removed: true; spare?: boolean; taken?: boolean; branchLeft?: string } | { removed: false; reason: string };

// Whether a worktree the record still has is gone: its branch has, and its
// folder either has too ('gone') or is another checkout's now ('taken', a
// reused slot). Reads only, for records that missed hearing it went.
export async function worktreeGone(w: WorktreeRecord): Promise<false | 'gone' | 'taken'> {
  const branch = await git(w.mainCheckout, ['rev-parse', '--verify', '--quiet', `refs/heads/${w.branch}`]);
  // 1 is "no such ref"; anything else (no repo there) proves nothing.
  if (branch.ok || branch.code !== 1) return false;
  if (!isDir(w.path)) return 'gone';
  const checkouts = await listCheckouts(w.mainCheckout);
  if (!checkouts) return false;
  const here = realpath(w.path);
  return checkouts.some((c) => realpath(c.path) === here && c.branch === w.branch) ? false : 'taken';
}

// Whether the folder at the worktree's path is itself the worktree git made
// there: a real folder, not a link (git's own list follows one, so a folder
// swapped for a link to the main checkout passed as the worktree, and its
// teardown ran in main: Codex's review, 9 Nov), whose own .git names the
// admin folder that names it back, of this repo (patch-trust.ts checks
// that the same way for trust).
function isWorktreeItself(w: WorktreeRecord): boolean {
  try {
    if (!fs.lstatSync(w.path).isDirectory()) return false;
  } catch {
    return false;
  }
  const real = realpath(w.path);
  // No link in its last part, however the folders above it are reached.
  if (real !== path.join(realpath(path.dirname(w.path)), path.basename(w.path))) return false;
  const main = linkedMainCheckout(real);
  return main !== undefined && realpath(main) === realpath(w.mainCheckout);
}

// The start of a small file, without waiting: undefined when it isn't a
// plain file (opening a fifo would block the runner) or is over `max`.
export function readPlainFile(file: string, max = 1024 * 1024): string | undefined {
  let fd: number | undefined;
  try {
    fd = fs.openSync(file, fs.constants.O_RDONLY | fs.constants.O_NONBLOCK);
    const st = fs.fstatSync(fd);
    if (!st.isFile() || st.size > max) return undefined;
    const buf = Buffer.alloc(st.size);
    const n = st.size ? fs.readSync(fd, buf, 0, st.size, 0) : 0;
    return buf.subarray(0, n).toString('utf8');
  } catch {
    return undefined;
  } finally {
    if (fd !== undefined) fs.closeSync(fd);
  }
}

// The worktree's folder, as a real path, if right now it's the worktree git
// made there (isWorktreeItself) and its own HEAD is `head`: the thread's
// branch, or the commit a cleanup detached it at. Never any HEAD at all: a
// switch to another branch while the cleanup's status ran, then removing it
// with --force, took edits made on that branch (Codex's third look, 9 Nov). Synchronous, so it can be the last
// thing before running something there or changing it, with nothing awaited
// between: checked before an await, a swap for a link to the main checkout
// while git answered passed, and the teardown ran in main (Codex's second
// look, 9 Nov). Run there by the path this returns.
export function worktreeFolderNow(w: WorktreeRecord, head = `ref: refs/heads/${w.branch}`): string | undefined {
  if (!isWorktreeItself(w)) return undefined;
  const real = realpath(w.path);
  const dotGit = readPlainFile(path.join(real, '.git'), 8192);
  const named = dotGit?.split('\n').find((l) => l.startsWith('gitdir:'))?.slice('gitdir:'.length).trim();
  if (!named) return undefined;
  const now = readPlainFile(path.join(path.resolve(real, named), 'HEAD'), 8192);
  return now?.trim() === head ? real : undefined;
}

// Whether the worktree's folder is still a checkout of its own branch, so
// what runs there (its teardown) runs in the thread's work, not in a reused
// slot that's another thread's now, nor anywhere a link leads. Checked again
// with worktreeFolderNow just before anything runs.
export async function stillCheckedOut(w: WorktreeRecord): Promise<boolean> {
  if (!isWorktreeItself(w)) return false;
  const checkouts = await listCheckouts(w.mainCheckout);
  const here = realpath(w.path);
  return Boolean(checkouts?.some((c) => realpath(c.path) === here && c.branch === w.branch));
}

// When its thread is archived or deleted: a worktree whose branch is all
// on main is removed with its branch (`--force` only for untracked
// files), or, with `keep` and room for another spare, set aside as one;
// otherwise both stay, and the reason says why.
export async function cleanUpWorktree(w: WorktreeRecord, stillOurs: () => boolean = () => true): Promise<Cleanup> {
  // Checked before each change: a runner handing the thread over stops
  // waiting for a slow git after a while, and from then the folder may be
  // the new runner's to use, so nothing more is changed here.
  const LET_GO: Cleanup = { removed: false, reason: 'the thread was handed to another runner' };
  // git's own 1 is "no such ref"; anything else (it timed out, no repo
  // there) says nothing, and reading it as gone would remove or pool a
  // checkout whose branch has work on it (Codex's third look, 9 Nov).
  const ref = await git(w.mainCheckout, ['rev-parse', '--verify', '--quiet', `refs/heads/${w.branch}`]);
  if (!ref.ok && ref.code !== 1) return { removed: false, reason: `git couldn't tell whether ${w.branch} is still there` };
  let branchThere = ref.ok;
  if (branchThere) {
    const merged = await git(w.mainCheckout, ['merge-base', '--is-ancestor', w.branch, MAIN]);
    if (!merged.ok) return { removed: false, reason: `${w.branch} has commits that aren't on ${MAIN}` };
  }
  // A record can outlive its worktree without hearing (THREADBEAR-16: a
  // handover saved it while the cleanup ran), and slot folders are reused.
  // So the folder is only touched if it's still this branch's checkout:
  // otherwise it may be another thread's, and --force would take its work.
  let ours = false;
  if (fs.existsSync(w.path)) {
    const checkouts = await listCheckouts(w.mainCheckout);
    if (!checkouts) return { removed: false, reason: `git couldn't list the worktrees of ${w.mainCheckout}` };
    const here = realpath(w.path);
    // And the folder is that checkout itself, not a link to some other
    // (the main checkout, whose status and removal would be read as this).
    ours = checkouts.some((c) => realpath(c.path) === here && c.branch === w.branch) && isWorktreeItself(w);
    if (!ours) {
      if (branchThere) {
        if (!stillOurs()) return LET_GO;
        const del = await git(w.mainCheckout, ['branch', '-D', w.branch]);
        // The folder's someone else's either way, so it counts as removed.
        if (!del.ok) return { removed: true, taken: true, branchLeft: `deleting ${w.branch} failed: ${firstLine(del.stderr)}` };
      }
      return { removed: true, taken: true };
    }
  }
  if (ours) {
    // Each git that reads or changes the folder itself goes by the folder
    // checked just before it (worktreeFolderNow): a swap for a link to the
    // main checkout while an earlier git ran would otherwise have main's
    // status read as this one's and main's checkout detached.
    const CHANGED: Cleanup = { removed: false, reason: `its folder ${w.path} changed while it was being cleaned up, so it was left alone` };
    let head = `ref: refs/heads/${w.branch}`;
    let here = worktreeFolderNow(w, head);
    if (!here) return CHANGED;
    const status = await gitOut(here, ['status', '--porcelain', '--untracked-files=all']);
    if (status === undefined) return { removed: false, reason: `git couldn't read ${w.path}` };
    const lines = status ? status.split('\n') : [];
    if (lines.some((l) => !l.startsWith('??'))) return { removed: false, reason: `the worktree has uncommitted changes` };
    if (w.keep && (await spares(w.mainCheckout, path.dirname(w.path))).length < MAX_SPARES) {
      // Detached where it is (no files change), so the branch can go; its
      // untracked files go when the next thread takes it.
      if (!stillOurs()) return LET_GO;
      here = worktreeFolderNow(w);
      if (!here) return CHANGED;
      const tip = await gitOut(here, ['rev-parse', 'HEAD']);
      if (!tip) return { removed: false, reason: `git couldn't read ${w.path}` };
      if ((await git(here, ['checkout', '-q', '--detach'])).ok) {
        // From here it's on that commit, detached, not on the branch.
        head = tip;
        if (!stillOurs()) return LET_GO;
        if (branchThere && (await git(w.mainCheckout, ['branch', '-D', w.branch])).ok) branchThere = false;
        if (!stillOurs()) return LET_GO;
        if (!branchThere && (await git(w.mainCheckout, ['worktree', 'lock', '--reason', SPARE, w.path])).ok) return { removed: true, spare: true };
      }
      // Couldn't be set aside: removed instead, below.
    }
    const args = ['worktree', 'remove', ...(lines.length ? ['--force'] : []), w.path];
    if (!stillOurs()) return LET_GO;
    // On its branch still, or detached just above (a spare that couldn't be
    // locked), and nothing else.
    if (!worktreeFolderNow(w, head)) return CHANGED;
    const rm = await git(w.mainCheckout, args);
    if (!rm.ok) return { removed: false, reason: `git worktree remove failed: ${firstLine(rm.stderr)}` };
  } else {
    if (!stillOurs()) return LET_GO;
    await git(w.mainCheckout, ['worktree', 'prune']);
  }
  // Checked above that it's all on main, so -D loses nothing.
  if (branchThere) {
    if (!stillOurs()) return LET_GO;
    const del = await git(w.mainCheckout, ['branch', '-D', w.branch]);
    if (!del.ok) return { removed: true, branchLeft: `deleting ${w.branch} failed: ${firstLine(del.stderr)}` };
  }
  // The <repo>-threads folder, once it's empty.
  try {
    fs.rmdirSync(path.dirname(w.path));
  } catch {
    // Not empty, or gone.
  }
  return { removed: true };
}

// When a worktree has to stay after its thread is archived or deleted (work
// that isn't on main, uncommitted changes), its build caches (the patch's
// `keep`) go. Nothing else can use them: no other thread takes the folder
// until its branch is landed or dropped, and only a spare's caches are
// reused. On 13 Nov 2026 such slots held 39 GB of app builds on Mike's Mac,
// and he asked for them to go once no thread can use them. A thread brought
// back just builds again. Only while the folder is still this branch's own
// checkout, and only real folders inside it, never through a link to
// somewhere else. Each is renamed aside first, right after that check with
// nothing awaited between, then deleted: deleting a gigabyte or two takes a
// while, and the check can't hold for that long. Returns what went.
export async function clearBuildCaches(w: WorktreeRecord): Promise<string[]> {
  if (!w.keep?.length || !(await stillCheckedOut(w))) return [];
  const cleared: string[] = [];
  for (const k of w.keep) {
    const rel = k.replace(/^\/+|\/+$/g, '');
    if (!rel || rel.split('/').includes('..')) continue;
    const here = worktreeFolderNow(w);
    if (!here) break;
    const dir = path.join(here, rel);
    let aside: string;
    try {
      // A real folder, reached without a link anywhere in `rel`.
      if (!fs.lstatSync(dir).isDirectory() || realpath(dir) !== dir) continue;
      aside = path.join(here, `.threadbear-clearing-${rel.replace(/\//g, '-')}-${Date.now()}`);
      fs.renameSync(dir, aside);
    } catch {
      continue;
    }
    cleared.push(rel);
    await fs.promises.rm(aside, { recursive: true, force: true }).catch(() => undefined);
  }
  return cleared;
}
