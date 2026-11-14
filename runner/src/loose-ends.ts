// Loose ends (experiment looseEnds; docs/protocol.md, "Loose ends"): what's
// still open on a thread whose last turn said it's done, checked by the
// runner so the done card can say whether Archive is safe without Mike
// asking the agent. The nightly retro counted 27 typed "anything else, or
// archive?" asks on 6 Nov 2026: Archive puts the thread away straight off,
// and he wanted someone's word that nothing was left first. The one real
// loose end in the threads it looked at was a branch not on main
// (938c1cd1), so these are the things a runner can see for itself.
import { execFile } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import type { BackgroundTask, LooseEnd, Schedule } from './protocol.ts';
import { MAIN, type WorktreeRecord } from './worktrees.ts';
import { untrustedGit } from './safe-git.ts';
import { cutSafely } from './safe-cut.ts';

// Any repo a thread touched can be anyone's (a clone it just made, a folder
// from a zip with its own .git/config), and these run by themselves when a
// turn ends, in any mode. So every git here goes through safe-git: no
// filters, hooks, fsmonitor, lazy fetch or submodule's own config (Codex's
// review and the security review, 11 Nov 2026).
const readGit = (dir: string, args: string[]) => untrustedGit(dir, args, { timeoutMs: 30_000 });

// The thread's own worktree: commits on its branch that main doesn't have,
// and files in it that aren't committed. Untracked ones count, as a new
// file the agent wrote and never committed is lost when the worktree goes;
// ignored ones (build output, the linked node_modules) don't show at all.
// Through safe-git like the rest (readGit): the worktree's files
// can be another runner's (a move), and its status would otherwise run the
// filters its .gitattributes pick and look inside its submodules with
// their own config, all automatically when a turn ends (security review,
// 11 Nov 2026).
export async function worktreeLooseEnds(w: WorktreeRecord): Promise<LooseEnd[]> {
  const out: LooseEnd[] = [];
  const ahead = await readGit(w.mainCheckout, ['rev-list', '--count', `${MAIN}..refs/heads/${w.branch}`]);
  const commits = ahead.ok ? Number.parseInt(ahead.stdout.trim(), 10) : 0;
  if (commits > 0) out.push({ kind: 'unlanded', text: `${commits === 1 ? '1 commit' : `${commits} commits`} on ${w.branch} ${commits === 1 ? "isn't" : "aren't"} on ${MAIN}` });
  const status = await readGit(w.path, ['status', '--porcelain', '--untracked-files=all']);
  const files = status.ok ? status.stdout.split('\n').filter((l) => l.trim()).length : 0;
  if (files > 0) out.push({ kind: 'uncommitted', text: `${files === 1 ? '1 file' : `${files} files`} in its worktree ${files === 1 ? "isn't" : "aren't"} committed` });
  return out;
}

// Files the thread changed (its Highlights' edited list) outside a
// worktree of its own: a thread in ~/dev/me/misc or a repo without
// worktrees shares its checkout, so only its own files count, not the
// whole folder's status, which holds other threads' work too. And the
// commits it made there that no remote has, in a repo that has one: his
// own repos push straight to main. The check-in of 11 Nov found most of
// his typed "anything else?" asks came from threads like these, where
// the card could only see threads and check-ins. `skip` is the thread's
// own worktree, which worktreeLooseEnds covers.
// `pushed` are the commits its Highlights saw it push: the ones that can
// be pushed and still not finished (unmergedLooseEnds). `dirs` are more
// folders to look in for its commits, the thread's own folder: a thread
// that only changed files through a shell has no `edited` to find its repo
// by (Codex's review, 11 Nov 2026).
export type EditedOptions = { pushed?: string[]; dirs?: string[]; prs?: PrLookup };


/** The remote's main branch ("origin/main"), as origin/HEAD names it, else main or master. */
async function remoteMain(top: string): Promise<string | undefined> {
  const head = await readGit(top, ['symbolic-ref', '--short', 'refs/remotes/origin/HEAD']);
  let main = head.ok ? head.stdout.trim() : '';
  for (const guess of ['origin/main', 'origin/master']) {
    if (main) break;
    if ((await readGit(top, ['rev-parse', '--verify', '--quiet', `refs/remotes/${guess}`])).ok) main = guess;
  }
  return main.startsWith('origin/') ? main : undefined;
}

// How far back main is read for a landed commit: a day or two of a busy
// repo's commits, read once per check.
const LANDED_LOOKBACK = 3000;

/**
 * How many of `shas`, on no remote branch themselves, landed on the remote's
 * main as other commits: tb land rebases a thread's work onto main, which
 * gives each commit a new id (and a conflict sorted on the way, new
 * contents), but keeps its author, author time and words, and leaves the
 * old commit on no branch at all. One still on a local branch is work here
 * that hasn't gone (an amend of a pushed commit keeps its time and words
 * too, Codex's review, 13 Nov 2026), so only one on no branch can match.
 */
async function landedByRebase(top: string, shas: string[]): Promise<number> {
  const main = await remoteMain(top);
  if (!main) return 0;
  const keys: string[] = [];
  for (const sha of shas) {
    const local = await readGit(top, ['branch', '--contains', sha]);
    if (!local.ok || local.stdout.trim()) continue;
    // rev-list, not show or log: safe-git runs only what's been looked at.
    const r = await readGit(top, ['rev-list', '--no-commit-header', '--format=%at%x09%ae%x09%s', '-1', sha]);
    if (r.ok && r.stdout.trim()) keys.push(r.stdout.trim());
  }
  if (!keys.length) return 0;
  // A count, not --since: that goes by commit time and stops early on a
  // clock that was off (Codex's review).
  const log = await readGit(top, ['rev-list', '--no-commit-header', '--format=%at%x09%ae%x09%s', `--max-count=${LANDED_LOOKBACK}`, `refs/remotes/${main}`]);
  if (!log.ok) return 0;
  const onMain = new Set(log.stdout.split('\n').map((l) => l.trim()).filter(Boolean));
  return keys.filter((k) => onMain.has(k)).length;
}

export async function editedLooseEnds(edited: string[], commits: string[], skip?: string, opts: EditedOptions = {}): Promise<LooseEnd[]> {
  const { pushed = [], dirs = [], prs = githubPr } = opts;
  const repos = new Map<string, string[]>();
  const tops = new Map<string, string | undefined>();
  const topOf = async (file: string, isDir = false): Promise<string | undefined> => {
    let dir = isDir ? file : path.dirname(file);
    // A file since deleted, in a folder since deleted: its nearest folder.
    while (!fs.existsSync(dir) && path.dirname(dir) !== dir) dir = path.dirname(dir);
    if (!tops.has(dir)) {
      const r = await readGit(dir, ['rev-parse', '--show-toplevel']);
      tops.set(dir, r.ok ? r.stdout.trim() : undefined);
    }
    return tops.get(dir);
  };
  const inside = (file: string, dir: string) => file === dir || file.startsWith(dir + path.sep);
  for (const file of edited) {
    if (!path.isAbsolute(file) || (skip && inside(file, skip))) continue;
    const top = await topOf(file);
    if (!top || (skip && inside(top, skip))) continue;
    repos.set(top, [...(repos.get(top) ?? []), path.relative(top, file)]);
  }
  // Its own folder, for the commits only: no files of it count as changed.
  if (commits.length || pushed.length) {
    for (const dir of dirs) {
      if (!path.isAbsolute(dir) || (skip && inside(dir, skip))) continue;
      const top = await topOf(dir, true);
      if (top && !(skip && inside(top, skip)) && !repos.has(top)) repos.set(top, []);
    }
  }
  const out: LooseEnd[] = [];
  const budget = { lookups: 0 };
  // Each commit once per repo: worktrees of one repo share its commits, and
  // a thread's borrowed slots each named the same ten (Mike, 13 Nov 2026).
  // Per repo (its common .git), not overall: two clones with different
  // remotes are two places it could still need pushing (Codex's review).
  const seen = new Set<string>();
  for (const [top, files] of repos) {
    const name = path.basename(top);
    const status = files.length ? await readGit(top, ['status', '--porcelain', '--untracked-files=all', '--', ...files]) : undefined;
    const n = status?.ok ? status.stdout.split('\n').filter((l) => l.trim()).length : 0;
    if (n > 0) out.push({ kind: 'uncommitted', text: `${n === 1 ? '1 file' : `${n} files`} it changed in ${name} ${n === 1 ? "isn't" : "aren't"} committed` });
    // A commit the thread made is in this repo if git knows it here.
    const remotes = await readGit(top, ['remote']);
    if (!remotes.ok || !remotes.stdout.trim()) continue;
    const common = await readGit(top, ['rev-parse', '--path-format=absolute', '--git-common-dir']);
    const repo = common.ok ? common.stdout.trim() : top;
    const off: string[] = [];
    for (const sha of commits) {
      if (seen.has(`${repo} ${sha}`) || !(await readGit(top, ['cat-file', '-e', `${sha}^{commit}`])).ok) continue;
      seen.add(`${repo} ${sha}`);
      const on = await readGit(top, ['branch', '-r', '--contains', sha]);
      if (on.ok && !on.stdout.trim()) off.push(sha);
    }
    const unpushed = off.length ? off.length - (await landedByRebase(top, off)) : 0;
    if (unpushed > 0) out.push({ kind: 'unpushed', text: `${unpushed === 1 ? '1 commit' : `${unpushed} commits`} it made in ${name} ${unpushed === 1 ? "isn't" : "aren't"} pushed` });
    out.push(...(await unmergedLooseEnds(top, pushed, prs, budget)));
  }
  return out;
}

// Pushed isn't finished. On 11 Nov 2026 the examples thread (0222c475) said
// Done with its work pushed to three stacked draft PRs in convex/website,
// none merged, and the card showed a plain Done: Mike nearly archived it.
// So a commit it pushed that's on an origin branch but not origin's default
// one is open, one line per branch, naming the PR when GitHub has one. A
// merged or closed PR whose head is still the branch's tip isn't (a squash
// merge leaves the commits off main for good); one from before newer pushes
// to a reused branch doesn't cover them. A commit no remote branch has any
// more (rebased and force-pushed) can't be told, so it's left out. Known
// gaps (Codex's review): only origin is looked at, so work pushed to a fork
// remote isn't seen; a branch squashed into another thread's stacked base
// counts as done there; and a repo it only reached through a shell, outside
// its own folder, isn't found. end_turn's needs rule covers what this
// misses.
const MAX_UNMERGED = 3;
// PR lookups for one check, across its repos. The commits need no cap of
// their own: Highlights keeps at most 80 items.
const MAX_LOOKUPS = 6;
async function unmergedLooseEnds(top: string, pushed: string[], prs: PrLookup, budget: { lookups: number }): Promise<LooseEnd[]> {
  if (!pushed.length) return [];
  const main = await remoteMain(top);
  if (!main) return [];
  const branches = new Set<string>();
  for (const sha of pushed) {
    if (!/^[0-9a-f]{7,64}$/i.test(sha)) continue;
    if (!(await readGit(top, ['cat-file', '-e', `${sha}^{commit}`])).ok) continue;
    if ((await readGit(top, ['merge-base', '--is-ancestor', sha, `refs/remotes/${main}`])).ok) continue;
    const on = await readGit(top, ['for-each-ref', '--format=%(refname)', '--contains', sha, 'refs/remotes/origin/']);
    if (!on.ok) continue;
    for (const ref of on.stdout.split('\n').map((l) => l.trim())) {
      const b = ref.slice('refs/remotes/origin/'.length);
      if (ref.startsWith('refs/remotes/origin/') && b && b !== 'HEAD' && `origin/${b}` !== main) branches.add(b);
    }
  }
  if (!branches.size) return [];
  const name = path.basename(top);
  const into = main.slice('origin/'.length);
  const url = await readGit(top, ['remote', 'get-url', 'origin']);
  const repo = url.ok ? githubRepo(url.stdout.trim()) : undefined;
  const out: LooseEnd[] = [];
  for (const branch of [...branches].sort()) {
    if (out.length >= MAX_UNMERGED) break;
    const pr = repo && budget.lookups++ < MAX_LOOKUPS ? await prs(repo, branch) : undefined;
    if (pr && pr.state !== 'OPEN') {
      const tip = await readGit(top, ['rev-parse', '--verify', '--quiet', `refs/remotes/origin/${branch}^{commit}`]);
      if (tip.ok && tip.stdout.trim() === pr.headRefOid) continue;
    }
    out.push({
      kind: 'unmerged',
      text: pr?.state === 'OPEN' ? `PR #${pr.number} in ${name} ${pr.isDraft ? 'is still a draft' : "isn't merged"}` : `Work it pushed to ${branch} in ${name} isn't on ${into}`,
    });
  }
  return out;
}

// "owner/name" for a github.com remote, else undefined.
export function githubRepo(url: string): string | undefined {
  const m = url.match(/^(?:https:\/\/(?:[^@/]+@)?github\.com\/|git@github\.com:|ssh:\/\/git@github\.com\/)([\w.-]+)\/([\w.-]+?)(?:\.git)?\/?$/);
  return m ? `${m[1]}/${m[2]}` : undefined;
}

export type Pr = { number: number; state: 'OPEN' | 'MERGED' | 'CLOSED'; isDraft: boolean; headRefOid?: string };
export type PrLookup = (repo: string, branch: string) => Promise<Pr | undefined>;

// The newest PR from that branch, through the gh CLI, or undefined (no gh,
// not signed in, offline, no PR). Run outside the repo with -R, so the
// repo's own git config never runs under gh. Kept a minute, the lookup
// itself included, as opening a thread checks again every few seconds.
const PR_TTL_MS = 60_000;
const prCache = new Map<string, { at: number; pr: Promise<Pr | undefined> }>();
export const githubPr: PrLookup = (repo, branch) => {
  const key = `${repo}#${branch}`;
  const hit = prCache.get(key);
  if (hit && Date.now() - hit.at < PR_TTL_MS) return hit.pr;
  const pr = new Promise<Pr | undefined>((resolve) => {
    execFile(
      'gh',
      ['pr', 'list', '-R', repo, '--head', branch, '--state', 'all', '--limit', '1', '--json', 'number,state,isDraft,headRefOid'],
      { cwd: os.tmpdir(), timeout: 15_000, killSignal: 'SIGKILL', env: { ...process.env, GH_PROMPT_DISABLED: '1', GIT_TERMINAL_PROMPT: '0' } },
      (err, stdout) => {
        if (err) return resolve(undefined);
        try {
          const [p] = JSON.parse(String(stdout)) as Pr[];
          resolve(p && typeof p.number === 'number' ? p : undefined);
        } catch {
          resolve(undefined);
        }
      },
    );
  });
  prCache.set(key, { at: Date.now(), pr });
  return pr;
};

// Background work the thread's CLI is still running (a shell, a monitor,
// a sub-agent): archiving stops the CLI and that with it, so the line
// says so. Seven of the 14 lines agents wrote for the card before 13 Nov
// 2026 were this ("waiting in the landing queue"); the runner knows it
// without being told. One that's finished and waiting to be read isn't
// running any more.
const MAX_RUNNING = 3;
const MAX_JOB_CHARS = 100;

export function runningLooseEnds(tasks: BackgroundTask[]): LooseEnd[] {
  return tasks
    .filter((t) => !t.finished)
    .slice(0, MAX_RUNNING)
    .map((t) => {
      // A Codex job's description is its whole command, which can hold a
      // key. The line goes to Convex with words after it, so a plain cut
      // through a key sent all but its last character: the scrub knows a
      // cut-off key only at the very end of a text (Daybreak, 13 Nov 2026).
      // So the cut moves back to before anything the scrub would take out,
      // Nothing comes off its end either: dots taken off a key the scrub
      // only learns of later would leave it unrecognisable (Codex's review).
      const what = t.description.replace(/\s+/g, ' ').trim();
      const short = cutSafely(what, MAX_JOB_CHARS, '…');
      // Cut, its words go last, ending in the "…": a key the scrub learns
      // of only after this line was made (a .env still being read) is then
      // a cut-off value at the very end, which it does take out (Codex's
      // review). Words after it hid it from that.
      if (short !== what) return { kind: 'running' as const, text: `Still running (archiving stops it): ${short}` };
      if (/[.!?…]$/.test(what)) return { kind: 'running' as const, text: `Still running: ${what} Archiving stops it.` };
      return { kind: 'running' as const, text: `Still running: ${what || 'a background job'}. Archiving stops it.` };
    });
}

// A thread this one started that hasn't finished: still working, or
// waiting on Mike. Hibernating or done ones aren't open business here.
export type ChildThread = { id: string; title: string; working: boolean; needsYou: boolean };

export function childLooseEnds(children: ChildThread[]): LooseEnd[] {
  const out: LooseEnd[] = [];
  for (const c of children) {
    if (c.working) out.push({ kind: 'thread', threadId: c.id, text: `"${c.title}", a thread it started, is still working` });
    else if (c.needsYou) out.push({ kind: 'thread', threadId: c.id, text: `"${c.title}", a thread it started, is waiting for you` });
  }
  return out;
}

// A schedule that will message this thread again (a check-in it booked).
// Not a reason to keep it: an archived thread comes back when one runs.
// It's on the card so Mike knows archiving it doesn't lose the check-in.
export function scheduleLooseEnds(threadId: string, schedules: Schedule[]): LooseEnd[] {
  return schedules
    .filter((s) => s.threadId === threadId && !s.paused && s.nextRunAt)
    .sort((a, b) => (a.nextRunAt! < b.nextRunAt! ? -1 : 1))
    .slice(0, 3)
    .map((s) => ({ kind: 'scheduled' as const, text: s.title, at: s.nextRunAt }));
}

// Whether any of them should stop a plain Archive: a booked check-in doesn't.
export function anyOpen(ends: LooseEnd[]): boolean {
  return ends.some((e) => e.kind !== 'scheduled');
}
