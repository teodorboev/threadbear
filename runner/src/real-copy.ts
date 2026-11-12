// A dev runner on a copy of the real data dir (dev-runner.sh --clone-real),
// for trying a change on real-sized data. A copy's threads name Mike's real
// Claude sessions and folders, so on 11 Nov 2026 a dev runner on a plain
// `cp -R` of the data dir carried on a thread the copy said was mid-turn,
// resumed its real session in ~/dev/me/misc and ran a command there.
//
// So a copy holds REAL_COPY_FILE, and a runner that finds it:
// - neutralises every thread it loads (neutraliseCopied): idle, no session
//   id, no queue, carry-on, wake, wait, schedule or worktree, and its folder
//   swapped for one under the copy's work folder (named after the real one,
//   so the sidebar still groups by folder);
// - marks each `copied`, and never starts a CLI for one (Runner.startSession).
//   A thread started in the dev runner itself runs as usual.
// cloneRealHome makes the copy: threads and projects only, never
// runner.json, secrets, schedules, the backend's keys or sync state.
import fs from 'node:fs';
import path from 'node:path';
import type { ThreadRecord } from './store.ts';
import { atomicWriteFileSync, readJsonSync } from './fsutil.ts';

export const REAL_COPY_FILE = 'copy-of-real.json';

export type RealCopy = { from: string; at: string; work: string };

export function realCopy(home: string): RealCopy | undefined {
  const file = path.join(home, REAL_COPY_FILE);
  if (!fs.existsSync(file)) return undefined;
  const c = readJsonSync<RealCopy>(file);
  // Unreadable: still a copy. Its work folder falls back to <home>/work.
  return { from: c?.from ?? '', at: c?.at ?? '', work: c?.work || path.join(home, 'work') };
}

export const COPIED_REFUSAL = "This thread is a copy of one of your real threads (a dev runner on --clone-real data), so it doesn't run here. Start a new thread to try something.";

// What a thread's record keeps once copied: what it shows, nothing that
// makes it run, wake, wait, or touch a real folder, session or worktree.
export function neutraliseCopied(rec: ThreadRecord, work: string): ThreadRecord {
  const r = rec as Record<string, unknown>;
  for (const k of [
    'nativeSessionId', 'sessionUsed', 'sessionCostTotal', 'contextUsage', 'turnId',
    'stoppedMidTurn', 'restartCarryOn', 'stoppedWork', 'stoppedWorkWakes',
    'waitingForRoom', 'limitHitTurn', 'autoCarryOn', 'waitingForNetwork', 'netCarryOn',
    'wakeWhen', 'waitingOn', 'labReviewOwed', 'handoff', 'moved', 'resumeAt',
    'worktree', 'worktreePending', 'schedule', 'share', 'panelRequest', 'snoozedUntilRestart', 'untilRelease',
  ]) delete r[k];
  rec.queue = [];
  rec.pendingApprovals = [];
  if (rec.status !== 'idle') rec.status = 'idle';
  if (!isUnder(rec.cwd, work)) rec.cwd = path.join(work, path.basename(rec.cwd || 'folder') || 'folder');
  fs.mkdirSync(rec.cwd, { recursive: true });
  rec.copied = true;
  return rec;
}

function isUnder(p: string, dir: string): boolean {
  const rel = path.relative(dir, p);
  return rel === '' || (!!rel && !rel.startsWith('..') && !path.isAbsolute(rel));
}

// What a copy brings: the threads (records, events, pictures) and the
// projects they're in. Nothing else in the data dir: runner.json (accounts,
// relay), secrets, schedules, backend keys, sync state, patch trust, the
// restart file and releases all act on the real world.
const COPIED = ['threads', 'projects.json', 'project-notes', 'project-icons', 'project-tasks.json'];

export function cloneRealHome(from: string, to: string, work: string, now = new Date()): { threads: number } {
  if (!fs.existsSync(path.join(from, 'threads'))) throw new Error(`${from} has no threads folder to copy.`);
  fs.mkdirSync(to, { recursive: true });
  if (fs.existsSync(path.join(to, 'threads'))) throw new Error(`${to} already has threads; clone into an empty folder.`);
  // Written first, so a runner started on a half-made copy still treats it as one.
  atomicWriteFileSync(path.join(to, REAL_COPY_FILE), JSON.stringify({ from, at: now.toISOString(), work } satisfies RealCopy, null, 1) + '\n');
  // dereference: a link to the real data dir's threads would otherwise be
  // written through by the neutralising below, and by the runner after.
  for (const name of COPIED) {
    const src = path.join(from, name);
    if (fs.existsSync(src)) fs.cpSync(src, path.join(to, name), { recursive: true, dereference: true });
  }
  fs.mkdirSync(work, { recursive: true });
  let threads = 0;
  const dir = path.join(to, 'threads');
  for (const id of fs.readdirSync(dir)) {
    const file = path.join(dir, id, 'thread.json');
    const rec = readJsonSync<ThreadRecord>(file);
    if (!rec) continue;
    atomicWriteFileSync(file, JSON.stringify(neutraliseCopied(rec, work), null, 1) + '\n');
    threads++;
  }
  return { threads };
}

// For dev-runner.sh: node src/real-copy.ts <from> <to> <work>
if (import.meta.main) {
  const [from, to, work] = process.argv.slice(2);
  if (!from || !to || !work) {
    console.error('usage: node src/real-copy.ts <from data dir> <to> <work folder>');
    process.exit(2);
  }
  try {
    const { threads } = cloneRealHome(from, to, work);
    console.log(`Copied ${threads} threads from ${from}, every one idle and without its session; they won't run here.`);
  } catch (e) {
    console.error((e as Error).message);
    process.exit(1);
  }
}
