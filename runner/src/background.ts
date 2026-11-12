import fs from 'node:fs';
import path from 'node:path';
import { defaultHome, expandHome, peekRunnerJson } from './config.ts';

// Builds and tests Threadbear starts itself (the land's check, a new
// worktree's setup, turn hooks like the runner typecheck, the release
// builds) run at a lower priority than what you're doing, so a land's check
// doesn't take the Mac over: one once held Mike's M5 above load 24 for ten
// minutes (6 Nov). The agent-side hook only lowers commands an agent runs
// itself, not what the runner or `tb land` spawns.
//
// How much lower is runner.json's `jobPriority` (or THREADBEAR_JOB_PRIORITY):
//   background  macOS's background priority, `taskpolicy -b`: efficiency
//               cores only, throttled disk. Mike's own rule for his Macs
//               ("Heavy compute" in his CLAUDE.md), so his runner.json says
//               it. Everything it starts inherits it.
//   low         `nice -n 10`, the default: full speed on an idle Mac, below
//               what you're using on a busy one.
//   normal      as it is.
// Background was everyone's default until 11 Nov. On Ville's Mac it made
// one landing take four hours: a quick check went past half an hour, and
// timing tests failed on the efficiency cores. Under nice it passed in
// under ten minutes. Without taskpolicy (Linux), background is low.
export const TASKPOLICY = '/usr/sbin/taskpolicy';
export type JobPriority = 'background' | 'low' | 'normal';
export const DEFAULT_JOB_PRIORITY: JobPriority = 'low';

export function jobPriorityOf(v: unknown): JobPriority | undefined {
  return v === 'background' || v === 'low' || v === 'normal' ? v : undefined;
}

// Read each time, so a change to runner.json needs no restart, and from
// the THREADBEAR_HOME of whoever asks (tb land in a thread has the
// runner's). An unreadable file is the default.
export function jobPriority(env: NodeJS.ProcessEnv = process.env): JobPriority {
  const fromEnv = jobPriorityOf(env.THREADBEAR_JOB_PRIORITY);
  if (fromEnv) return fromEnv;
  try {
    const home = path.resolve(expandHome(env.THREADBEAR_HOME || env.THREADBEAR_DATA_DIR || defaultHome()));
    return jobPriorityOf((peekRunnerJson(home) as { jobPriority?: unknown }).jobPriority) ?? DEFAULT_JOB_PRIORITY;
  } catch {
    return DEFAULT_JOB_PRIORITY;
  }
}

export function inBackground(
  bin: string,
  args: string[],
  opts: { platform?: NodeJS.Platform; exists?: (p: string) => boolean; priority?: JobPriority } = {},
): [string, string[]] {
  const platform = opts.platform ?? process.platform;
  const exists = opts.exists ?? fs.existsSync;
  const priority = opts.priority ?? jobPriority();
  if (priority === 'normal' || platform === 'win32') return [bin, args];
  if (priority === 'background' && platform === 'darwin' && exists(TASKPOLICY)) return [TASKPOLICY, ['-b', bin, ...args]];
  // nice is /usr/bin/nice on macOS and every Linux; found on the PATH if not.
  return [exists('/usr/bin/nice') ? '/usr/bin/nice' : 'nice', ['-n', '10', bin, ...args]];
}
