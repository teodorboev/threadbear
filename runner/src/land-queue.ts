// The landing queue behind `tb land` (docs/thread-worktrees.md, "Landing
// together"). On 12 Nov a dozen threads finished at once, and with each
// landing holding the repo's lock through its own 5 to 9 minute check,
// several waited over 40 minutes and two were killed by their agents' time
// limits still in the line. Now a landing that has to wait leaves an entry
// here first, and whichever landing holds the lock next checks everything
// queued ahead of it together with its own, in one check, and lands the lot.
//
// This file is only the entries on disk: one JSON file each in
// `<git common dir>/threadbear-land-queue/`, written whole (to a temp file,
// then renamed) so a reader never sees half of one. The owner writes its
// entry before waiting for the lock and removes it when it's done; the
// lock's holder writes the result into it. worktrees.ts does the git.
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';

// Where each check leaves its line (worktrees.ts, logCheckTimes), beside the
// landing lock in the repo's common git directory. The runner's updater
// reads it too, to leave out checks a landing has already passed (update.ts,
// landChecked).
export const LAND_TIMES_FILE = 'threadbear-land-times.log';

export type QueueResult = {
  // The entry's own commits as they landed on main (its rebased tip), and
  // their `git log --oneline`, for the owner to print.
  landed: string;
  commits: string;
  // The other branches in the same check.
  together: string[];
};

export type QueueEntry = {
  id: string;
  pid: number;
  at: number; // when it joined: the line's order
  branch: string;
  path: string; // its worktree
  base: string; // the main it was rebased on
  commit: string; // what it asked to land
  // The check its patch asks for (null for none): only landings asking for
  // the same one are checked together.
  check?: string | null;
  checkFrom?: string; // where its check was looked up, from its root
  // The check hosts its tb land asked for (its THREADBEAR_CHECK_HOST), if
  // any. A group's check runs in the holder's tb, so these go to it as
  // THREADBEAR_CHECK_PREFER: tried first, the holder's hosts still behind
  // them (checkPreference in worktrees.ts).
  checkHost?: string;
  // tb land --despite-red: the repair for a red main. Goes first in a
  // group check, outside the limit.
  fix?: boolean;
  // The most landings it may be checked with. Each failed check of a group
  // halves it for everyone in that group, so a failure is narrowed down in a
  // few checks rather than one by one.
  limit?: number;
  limitAt?: string; // the main that limit was set on; it lapses once main moves
  result?: QueueResult;
};

export const queueDir = (common: string): string => path.join(common, 'threadbear-land-queue');
const runningFile = (common: string): string => path.join(common, 'threadbear-land-running.json');

function writeWhole(file: string, value: unknown): void {
  const tmp = `${file}.${process.pid}.${crypto.randomBytes(4).toString('hex')}.tmp`;
  fs.writeFileSync(tmp, JSON.stringify(value));
  fs.renameSync(tmp, file);
}

function readJson<T>(file: string): T | undefined {
  try {
    return JSON.parse(fs.readFileSync(file, 'utf8')) as T;
  } catch {
    return undefined;
  }
}

export function joinQueue(common: string, e: Omit<QueueEntry, 'id' | 'pid' | 'at'>): QueueEntry {
  const dir = queueDir(common);
  fs.mkdirSync(dir, { recursive: true });
  const entry: QueueEntry = { id: `${Date.now()}-${process.pid}-${crypto.randomBytes(3).toString('hex')}`, pid: process.pid, at: Date.now(), ...e };
  writeWhole(path.join(dir, `${entry.id}.json`), entry);
  return entry;
}

export function readEntry(common: string, id: string): QueueEntry | undefined {
  return readJson<QueueEntry>(path.join(queueDir(common), `${id}.json`));
}

export function writeEntry(common: string, e: QueueEntry): void {
  // Only an entry that's still there: its owner may have given up.
  if (fs.existsSync(path.join(queueDir(common), `${e.id}.json`))) writeWhole(path.join(queueDir(common), `${e.id}.json`), e);
}

export function leaveQueue(common: string, id: string): void {
  fs.rmSync(path.join(queueDir(common), `${id}.json`), { force: true });
}

function alive(pid: number): boolean {
  try {
    process.kill(pid, 0);
    return true;
  } catch (e) {
    return (e as NodeJS.ErrnoException).code === 'EPERM';
  }
}

// Everyone waiting, first come first, without results yet. An entry whose
// tb has gone (killed by an agent's time limit, say) is removed: nobody's
// asking for it any more.
export function waiting(common: string): QueueEntry[] {
  let names: string[];
  try {
    names = fs.readdirSync(queueDir(common)).filter((n) => n.endsWith('.json'));
  } catch {
    return [];
  }
  const found: QueueEntry[] = [];
  for (const n of names) {
    const e = readJson<QueueEntry>(path.join(queueDir(common), n));
    if (!e || typeof e.pid !== 'number') continue;
    if (!alive(e.pid)) {
      leaveQueue(common, e.id);
      continue;
    }
    if (!e.result) found.push(e);
  }
  return found.sort((a, b) => a.at - b.at || a.id.localeCompare(b.id));
}

// What the lock's holder is checking, for the waiting line.
export type Running = {
  startedAt: number;
  branches: string[];
  // Optional so a tb waiting on an older holder can still read its status.
  pid?: number;
  entries?: string[];
};

export function setRunning(common: string, r: Running | undefined): void {
  try {
    if (r) writeWhole(runningFile(common), r);
    else fs.rmSync(runningFile(common), { force: true });
  } catch {
    // Only the waiting line reads it.
  }
}

export function readRunning(common: string): Running | undefined {
  const running = readJson<Running>(runningFile(common));
  return running?.pid !== undefined && !alive(running.pid) ? undefined : running;
}

// How long a passing check usually takes here: the median of the last 20
// in the times log, or undefined before there are any.
export function typicalCheckSeconds(timesFile: string): number | undefined {
  let text: string;
  try {
    text = fs.readFileSync(timesFile, 'utf8');
  } catch {
    return undefined;
  }
  const secs: number[] = [];
  for (const line of text.trimEnd().split('\n').slice(-200)) {
    try {
      const e = JSON.parse(line) as { ok?: boolean; dryRun?: boolean; seconds?: number };
      if (e.ok && !e.dryRun && typeof e.seconds === 'number') secs.push(e.seconds);
    } catch {
      // A line cut short.
    }
  }
  const last = secs.slice(-20).sort((a, b) => a - b);
  return last.length ? last[Math.floor(last.length / 2)] : undefined;
}

const minutes = (s: number): string => (s < 60 ? 'under a minute' : s < 90 ? '1 min' : `${Math.round(s / 60)} min`);

// Where a waiting landing is in the line, and roughly how long it has.
// Entries stay in the queue during their group check. Those have only the
// rest of this check to go; the others wait for this check and one more.
export function waitingLine(common: string, timesFile: string, me: QueueEntry | undefined, now = Date.now()): string {
  // The holder writes the result on disk. The waiting tb still has its
  // original entry and may not get the lock until another check finishes.
  if (me && (readEntry(common, me.id)?.result || me.result)) {
    return ': already landed with its group; waiting for the landing lock to finish up.';
  }
  const running = readRunning(common);
  // A new tb can join on the same branch after its old one was killed.
  // The old entry's commit may be in the check, but the new one's isn't.
  const checking = (e: QueueEntry): boolean => !!running && (running.entries
    ? running.entries.includes(e.id) : running.branches.includes(e.branch));
  const all = waiting(common).filter((e) => !checking(e));
  const ahead = me ? all.filter((e) => e.id !== me.id && (e.at < me.at || (e.at === me.at && e.id < me.id))).length : all.length;
  const typical = typicalCheckSeconds(timesFile);
  const parts: string[] = [];
  if (running) {
    const ago = Math.max(0, Math.round((now - running.startedAt) / 1000));
    if (me && checking(me)) {
      const peers = running.branches.length - 1;
      const withOthers = peers ? ` with ${peers} other landing${peers === 1 ? '' : 's'}` : '';
      let estimate = '';
      if (typical !== undefined) {
        const left = ago >= typical ? 'longer than usual' : `so roughly ${minutes(Math.max(60, typical - ago))} to go`;
        estimate = `, ${left} (a check takes about ${minutes(typical)} here)`;
      }
      return `: this landing is being checked${withOthers}; the check has been running ${minutes(ago)}${estimate}.`;
    }
    const what = running.branches.length > 1 ? `${running.branches[0]} and ${running.branches.length - 1} more` : running.branches[0] ?? 'another landing';
    parts.push(`the check for ${what} has been running ${minutes(ago)}`);
    if (typical !== undefined) {
      const left = Math.max(60, typical - ago) + typical;
      parts.push(`${ahead ? `${ahead} more waiting ahead of this one, all checked together with it next` : 'this one is checked next'}, so roughly ${minutes(left)} to go (a check takes about ${minutes(typical)} here)`);
    } else if (ahead) {
      parts.push(`${ahead} more waiting ahead of this one, all checked together with it next`);
    }
  } else if (ahead) {
    parts.push(`${ahead} more waiting ahead of this one, all checked together with it next`);
  }
  return parts.length ? `: ${parts.join('; ')}.` : '...';
}
