import fs from 'node:fs';
import path from 'node:path';
import { execFile } from 'node:child_process';
import type { Clock, TimerHandle } from './clock.ts';
import type { Logger } from './log.ts';
import type { BackgroundTask } from './adapters/types.ts';
import { atomicWriteFileSync, readJsonSync } from './fsutil.ts';

// Background jobs that look stuck (docs/protocol.md, "Stuck background
// jobs"). A thread whose turn has ended stays in Running while its CLI
// still has background shells or sub-agents going, and one that can never
// finish keeps it there for good: on 29 Oct a thread sat in Running for
// over an hour on `until ! pgrep -f snip2.py` loops that matched their own
// shell. Every few minutes this looks at idle Claude threads with
// background work and sends the thread a message about any job that has
// been going a while, hasn't written output for as long, and (a shell)
// used next to no CPU since the last look. The agent checks on it and
// stops it or carries on; it asks Mike if it can't tell. Once per job.
//
// A job that waits on Mike by design is left alone until its own timeout:
// `browserbear handoff <tab> --wait` sits silent with no CPU until he
// presses Done (5 Nov: two nudges in a row for one). The shell's command
// lines show it, so it's told apart from a loop that can never end.
// So is a `tb land`, which waits its turn in the landing line (isLanding).
//
// Claude only: its jobs write to output files under /tmp/claude-<uid>
// (a sub-agent's is a link to its transcript), which give the start and
// last-output times. Codex's background terminals leave nothing to read.

export const SWEEP_MS = 5 * 60_000;
// How long a job has to have been going, and quiet, before it counts.
export const QUIET_MS = 30 * 60_000;
// A shell that used less CPU than this between two looks is idle.
export const IDLE_CPU_SECONDS = 2;

export type IdleBackground = { threadId: string; sessionId: string; tasks: BackgroundTask[] };

// A job's output file: when it was made (the job started) and last written.
export type TaskFile = { startedAt: number; lastOutputAt: number };
// A background shell under a thread's CLI: the job it writes for (from the
// output file on its stdout), the CPU it and everything under it have used,
// and what's running there ("zsh", "sleep").
// `processes` are everything under it, with full command lines and when
// each started, for telling a wait on Mike apart.
export type BackgroundShell = { taskId?: string; cpuSeconds: number; commands: string[]; processes?: ShellProcess[] };
export type ShellProcess = { command: string; startedAt: number };

// How long past a waiting command's own timeout before it counts as stuck
// after all: it should have exited by itself by then.
export const WAIT_GRACE_MS = 5 * 60_000;
// browserbear's own default for handoff --wait (cli.mjs DEFAULT_WAIT_S).
const HANDOFF_DEFAULT_WAIT_S = 20 * 60;
// browserbear's flags that take a value (cli.mjs parse()).
const BROWSERBEAR_VALUE_FLAGS = new Set(['profile', 'group', 'until-url', 'until-text', 'timeout']);

// How long a process says it will wait on the user, in ms, or undefined
// when it isn't such a process. Only `browserbear handoff` with --wait,
// --until-url or --until-text for now. Only a real browserbear process
// counts (`node …/browserbear …`, as its shebang runs it), never a shell
// whose -c text merely mentions it: `false && browserbear handoff …; while
// true; do sleep 3; done` must still be judged. The flags are read the way
// browserbear's own parse() reads them: anywhere, the last one winning.
export function waitsOnUserMs(commandLine: string): number | undefined {
  const words = commandLine.trim().split(/\s+/);
  const isScript = (w: string) => /(^|\/)browserbear(\/cli\.mjs)?$/.test(w);
  let at = isScript(words[0] ?? '') ? 0 : -1;
  if (at < 0 && /(^|\/)node$/.test(words[0] ?? '')) {
    // Node's script is its first argument that isn't an option; inline
    // code (-e, -p) that only mentions browserbear doesn't count.
    const i = words.findIndex((w, i) => i > 0 && !w.startsWith('-'));
    if (i > 0 && isScript(words[i]) && !words.slice(1, i).some((w) => /^(-e|-p|--eval|--print)(=|$)/.test(w))) at = i;
  }
  if (at < 0) return undefined;
  const flags = new Map<string, string | true>();
  const pos: string[] = [];
  const argv = words.slice(at + 1);
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a.startsWith('--')) {
      const key = a.slice(2);
      const next = argv[i + 1];
      if (next !== undefined && !next.startsWith('--') && BROWSERBEAR_VALUE_FLAGS.has(key)) flags.set(key, argv[++i]);
      else flags.set(key, true);
    } else pos.push(a);
  }
  // ps joins the arguments with spaces, so a flag's value with spaces in it
  // (--until-text "Payment complete") spills into the positionals: look for
  // the command among them rather than only first.
  if (!pos.includes('handoff') || !(flags.get('wait') || flags.get('until-url') || flags.get('until-text'))) return undefined;
  const t = flags.get('timeout');
  const secs = t === undefined ? HANDOFF_DEFAULT_WAIT_S : Number(t);
  return (Number.isFinite(secs) && secs > 0 ? secs : HANDOFF_DEFAULT_WAIT_S) * 1000;
}

// How long a `tb land` is left to wait before it counts as stuck after all.
// It sits silent with no CPU by design: queued for the landing lock behind
// other threads' checks, or with its own check running on another machine
// (docs/thread-worktrees.md, "Landing together"). On 13 Nov, with 20
// landings queued and waits past 30 minutes, this check told a dozen
// threads their `tb land` looked stuck. Agents stopped it and ran it again,
// which sent them to the back of the line, and when the one stopped was
// running the group's check, that check went with it: three checks of 11,
// 14 and 13 branches were thrown away in half an hour, each making the
// waits longer and the next nudge surer, and nothing landed for 35 minutes.
// Three hours is how long a check waits for a host of its own accord
// (scripts/check-remote.sh); git commands under it give up after five minutes.
export const LAND_WAIT_MS = 3 * 60 * 60_000;

// Whether a process is a real `tb land`: `node …/tb land`, as tb's shebang
// runs it, at whatever priority (taskpolicy and nice exec it). Never a
// shell whose text merely mentions it, nor node's inline code, as for
// waitsOnUserMs: `tb land; while true; do sleep 3; done` is still judged
// once the landing has gone. A dry run counts too: it waits in the same line.
//
// ps joins the arguments with spaces and the installed tb's path has one
// (…/Application Support/Threadbear/runner/releases/<id>/bin/tb), so the
// script is the words up to the first that ends in /tb, as long as none
// before it is a script of its own (node other.mjs "x/tb land"). A script
// with no extension and such arguments can't be told from a path with a
// space; all it would get is the same three hours.
const isFileOnDisk = (p: string): boolean => {
  try {
    return fs.statSync(p).isFile();
  } catch {
    return false;
  }
};

export function isLanding(commandLine: string, isFile: (path: string) => boolean = isFileOnDisk): boolean {
  const words = commandLine.trim().split(/\s+/);
  // Node's own path can have a space too (/Users/m/My Tools/bin/node): the
  // words up to the first that ends in /node, when the line starts with a
  // path and none of them is a flag.
  const node = words.findIndex((w) => /(^|\/)node$/.test(w));
  // And not when the first word is a program by itself, which makes the
  // rest its arguments: `/bin/bash /tmp/node /tmp/tb land` is bash running
  // a script that happens to be called node (Codex's second look, 13 Nov).
  if (node < 0 || (node > 0 && (!words[0].startsWith('/') || isFile(words[0]) || words.slice(0, node).some((w) => w.startsWith('-'))))) return false;
  const first = words.findIndex((w, i) => i > node && !w.startsWith('-'));
  if (first < 0) return false;
  // Inline code in any of node's spellings, -eCODE included.
  if (words.slice(node + 1, first).some((w) => /^(-[ep]|--eval\b|--print\b)/.test(w))) return false;
  const script = words.findIndex((w, i) => i >= first && /(^|\/)tb$/.test(w));
  if (script < 0) return false;
  if (words.slice(first, script).some((w) => w.startsWith('-') || /\.[cm]?[jt]s$/.test(w))) return false;
  // tb's command is its first word that isn't a flag, and `tb land` takes
  // only flags (--dry-run, --despite-red).
  return words.slice(script + 1).find((w) => !w.startsWith('-')) === 'land';
}

// When the landing running under a shell now started, the earliest if
// there are several, or undefined with none. Asked at every look rather
// than kept like a handoff's deadline: a landing that has ended leaves
// whatever is still there (a wait loop, say) to be judged as usual, and a
// handoff earlier in the same shell doesn't cut a landing's time short
// (Codex's review, 13 Nov).
function landingStarted(processes: ShellProcess[]): number | undefined {
  let out: number | undefined;
  for (const p of processes) if (isLanding(p.command)) out = Math.min(out ?? Infinity, p.startedAt);
  return out;
}

// The latest a shell's waiting processes said they'd wait until, plus the grace.
function waitDeadline(processes: ShellProcess[]): number | undefined {
  let out: number | undefined;
  for (const p of processes) {
    const wait = waitsOnUserMs(p.command);
    if (wait !== undefined) out = Math.max(out ?? 0, p.startedAt + wait + WAIT_GRACE_MS);
  }
  return out;
}

export interface Probe {
  taskFile(sessionId: string, taskId: string): Promise<TaskFile | undefined>;
  shells(sessionIds: string[]): Promise<Map<string, BackgroundShell[]>>;
}

type Host = {
  idleBackground(): IdleBackground[];
  send(threadId: string, text: string, images: never[], opts: { origin: 'runner' }): unknown;
};

type Deps = { host: Host; clock: Clock; log: Logger; home: string; probe?: Probe };

// Jobs already nudged about, with when. Kept a day: a thread that's busy
// isn't in the idle list, so whether its job has ended can't be told from
// that, and a job still going a day later is worth another word anyway.
type Saved = { nudged: Record<string, string> };
const NUDGE_MEMORY_MS = 24 * 60 * 60_000;

// landing: another shell of the same thread has a tb land going, so this
// one may only be waiting for it (a loop polling its log, say).
type Stuck = { task: BackgroundTask; minutes: number; commands?: string[]; landing?: boolean };

export function stuckJobsPath(home: string): string {
  return path.join(home, 'stuck-jobs.json');
}

export class StuckJobs {
  private deps: Deps;
  private probe: Probe;
  private timer?: TimerHandle;
  private sweeping = false;
  // CPU seconds each shell had used at the last look, by thread/task.
  private cpu = new Map<string, number>();
  // When a job waiting on Mike (waitsOnUserMs) stops being left alone, by thread/task.
  private waitDeadline = new Map<string, number>();
  // When the first landing seen under a job started, by thread/task: its
  // three hours count from there, so a loop that keeps starting new
  // landings can't put the check off for good.
  private landingSince = new Map<string, number>();

  constructor(deps: Deps) {
    this.deps = deps;
    this.probe = deps.probe ?? realProbe();
  }

  start(): void {
    this.arm();
  }

  stop(): void {
    this.timer?.cancel();
    this.timer = undefined;
  }

  private arm(): void {
    this.timer = this.deps.clock.setTimeout(() => {
      void this.sweep().finally(() => this.timer && this.arm());
    }, SWEEP_MS);
  }

  async sweep(): Promise<void> {
    if (this.sweeping) return;
    this.sweeping = true;
    try {
      await this.look();
    } catch (e) {
      this.deps.log.warn('stuck-job check failed', { error: (e as Error).message });
    } finally {
      this.sweeping = false;
    }
  }

  private async look(): Promise<void> {
    const { host, clock } = this.deps;
    const now = clock.now();
    const threads = host.idleBackground();
    const saved = this.read();
    const live = new Set(threads.flatMap((t) => t.tasks.map((task) => key(t.threadId, task.id))));
    for (const k of this.cpu.keys()) if (!live.has(k)) this.cpu.delete(k);
    for (const k of this.waitDeadline.keys()) if (!live.has(k)) this.waitDeadline.delete(k);
    for (const k of this.landingSince.keys()) if (!live.has(k)) this.landingSince.delete(k);
    const nudged: Record<string, string> = {};
    for (const [k, at] of Object.entries(saved.nudged)) if (now - Date.parse(at) < NUDGE_MEMORY_MS) nudged[k] = at;

    // Jobs that have been going and quiet long enough, before any ps.
    const quiet = new Map<string, { thread: IdleBackground; task: BackgroundTask; minutes: number }[]>();
    for (const thread of threads) {
      for (const task of thread.tasks) {
        // Monitors sit quiet by design; other kinds leave no file to judge.
        if (task.kind !== 'shell' && task.kind !== 'agent') continue;
        if (nudged[key(thread.threadId, task.id)]) continue;
        const file = await this.probe.taskFile(thread.sessionId, task.id);
        if (!file || now - file.startedAt < QUIET_MS || now - file.lastOutputAt < QUIET_MS) continue;
        const list = quiet.get(thread.sessionId) ?? [];
        list.push({ thread, task, minutes: Math.round((now - file.startedAt) / 60_000) });
        quiet.set(thread.sessionId, list);
      }
    }

    const needShells = [...quiet.entries()].filter(([, l]) => l.some((q) => q.task.kind === 'shell')).map(([s]) => s);
    const shells = needShells.length ? await this.probe.shells(needShells) : new Map<string, BackgroundShell[]>();

    const stuck = new Map<string, Stuck[]>();
    for (const [sessionId, list] of quiet) {
      for (const { thread, task, minutes } of list) {
        const k = key(thread.threadId, task.id);
        let commands: string[] | undefined;
        if (task.kind === 'shell') {
          // Matched to its process by the output file. Not found: it has
          // exited and the CLI will say so, or it isn't ours to judge.
          const shell = shells.get(sessionId)?.find((s) => s.taskId === task.id);
          if (!shell) continue;
          // A landing waiting its turn, or on its check elsewhere: left
          // alone while one is running under it, for LAND_WAIT_MS from the
          // first one seen.
          const landing = landingStarted(shell.processes ?? []);
          if (landing !== undefined) {
            const since = this.landingSince.get(k) ?? landing;
            this.landingSince.set(k, since);
            if (now < since + LAND_WAIT_MS + WAIT_GRACE_MS) continue;
          }
          // Waiting on Mike, and still within the time it said it would,
          // counted from when that process started (a script may sleep
          // first). The first wait seen sets the job's deadline, so a loop
          // that keeps starting new handoffs can't put it off for good.
          const deadline = this.waitDeadline.get(k) ?? waitDeadline(shell.processes ?? []);
          if (deadline !== undefined) {
            this.waitDeadline.set(k, deadline);
            if (now < deadline) continue;
          }
          const before = this.cpu.get(k);
          this.cpu.set(k, shell.cpuSeconds);
          // Two looks are needed to see how much CPU it's using.
          if (before === undefined || shell.cpuSeconds - before >= IDLE_CPU_SECONDS) continue;
          commands = shell.commands;
        }
        // Another shell's, never this one's own: a landing past its three
        // hours is the job being told about, and mustn't be told to stay.
        const landing = (shells.get(sessionId) ?? []).some((sh) => sh.taskId !== task.id && landingStarted(sh.processes ?? []) !== undefined);
        const l = stuck.get(thread.threadId) ?? [];
        l.push({ task, minutes, commands, ...(landing ? { landing } : {}) });
        stuck.set(thread.threadId, l);
      }
    }

    for (const [threadId, jobs] of stuck) {
      try {
        host.send(threadId, message(jobs), [], { origin: 'runner' });
        for (const j of jobs) nudged[key(threadId, j.task.id)] = new Date(now).toISOString();
        this.deps.log.info('nudged a thread about stuck background jobs', { threadId, jobs: jobs.map((j) => j.task.description) });
      } catch (e) {
        this.deps.log.warn('could not nudge a thread about stuck jobs', { threadId, error: (e as Error).message });
      }
    }
    if (JSON.stringify(nudged) !== JSON.stringify(saved.nudged)) this.write({ nudged });
  }

  private read(): Saved {
    const s = readJsonSync<Saved>(stuckJobsPath(this.deps.home));
    return { nudged: s?.nudged && typeof s.nudged === 'object' && !Array.isArray(s.nudged) ? s.nudged : {} };
  }

  private write(s: Saved): void {
    atomicWriteFileSync(stuckJobsPath(this.deps.home), JSON.stringify(s, null, 2));
  }
}

function key(threadId: string, taskId: string): string {
  return `${threadId}/${taskId}`;
}

export function message(jobs: Stuck[]): string {
  const lines = jobs.map((j) => {
    const what = j.task.kind === 'agent' ? 'sub-agent' : j.task.kind === 'shell' ? 'shell' : 'job';
    const running = j.commands?.length ? `, running ${[...new Set(j.commands)].join(', ')}` : '';
    return `- ${j.task.description || 'a background task'} (${what}, ${j.minutes} min${running})`;
  });
  const one = jobs.length === 1;
  return [
    `Threadbear's stuck-job check: ${one ? 'this background job has' : 'these background jobs have'} been going for over ${QUIET_MS / 60_000} minutes without writing any output${jobs.some((j) => j.commands) ? ', and the shells have used next to no CPU since the last check' : ''}:`,
    '',
    ...lines,
    '',
    `Check whether ${one ? "it's" : "they're"} still doing anything useful. Stop any that are stuck (a wait loop that can never end, say) and carry on from there. If you can't tell, ask the user in end_turn.`,
    // The probe can't tell that a quiet loop is waiting on the landing in
    // another shell, but the agent can, and stopping the landing is the
    // costly mistake.
    ...(jobs.some((j) => j.landing)
      ? ['', "A tb land in another of this thread's shells is still going: it's waiting its turn in the landing line, or on its check on another machine. Leave it running, and anything that's only waiting for it. Stopping a tb land sends it to the back of the line, and loses the check for every thread it was being checked with."]
      : []),
  ].join('\n');
}

// The real look at the Mac: output files under /tmp/claude-<uid>, one ps
// for the process tree and one lsof for the shells' stdout. All async, so
// a busy Mac never stalls the runner.
export function realProbe(root = path.join('/tmp', `claude-${process.getuid?.() ?? 0}`)): Probe {
  const dirs = new Map<string, string>();
  const sessionDir = (sessionId: string): string | undefined => {
    const known = dirs.get(sessionId);
    if (known && fs.existsSync(known)) return known;
    let projects: string[];
    try {
      projects = fs.readdirSync(root);
    } catch {
      return undefined;
    }
    for (const p of projects) {
      const dir = path.join(root, p, sessionId);
      if (fs.existsSync(dir)) {
        dirs.set(sessionId, dir);
        return dir;
      }
    }
    return undefined;
  };
  return {
    async taskFile(sessionId, taskId) {
      const dir = sessionDir(sessionId);
      if (!dir) return undefined;
      const file = path.join(dir, 'tasks', `${taskId}.output`);
      try {
        const own = fs.lstatSync(file);
        // A sub-agent's file links to its transcript, written as it works.
        let written = own.mtimeMs;
        try {
          written = Math.max(written, fs.statSync(file).mtimeMs);
        } catch {
          // a dangling link: the link's own time
        }
        return { startedAt: own.birthtimeMs || own.mtimeMs, lastOutputAt: written };
      } catch {
        return undefined;
      }
    },
    async shells(sessionIds) {
      const out = new Map<string, BackgroundShell[]>();
      const procs = parsePs(await run('ps', ['-axo', 'pid=,ppid=,time=,etime=,command=']));
      const lookedAt = Date.now();
      const kids = new Map<number, Proc[]>();
      for (const p of procs) kids.set(p.ppid, [...(kids.get(p.ppid) ?? []), p]);
      const roots = new Map<string, Proc[]>();
      for (const sessionId of sessionIds) {
        const cli = procs.find((p) => isCliFor(p.command, sessionId));
        if (!cli) continue;
        // The Bash tool's shells source a snapshot of the login shell first.
        roots.set(sessionId, (kids.get(cli.pid) ?? []).filter((p) => p.command.includes('/shell-snapshots/snapshot-')));
      }
      const pids = [...roots.values()].flat().map((p) => p.pid);
      // lsof exits 1 when some pid has gone; what it printed still counts.
      const stdout = pids.length ? parseLsof(await run('lsof', ['-a', '-p', pids.join(','), '-d', '1', '-Fpn', '-n', '-P'], true)) : new Map<number, string>();
      for (const [sessionId, shells] of roots) {
        out.set(
          sessionId,
          shells.map((s) => {
            const tree = subtree(s, kids);
            const name = stdout.get(s.pid);
            return {
              taskId: name?.endsWith('.output') ? path.basename(name, '.output') : undefined,
              cpuSeconds: tree.reduce((sum, p) => sum + p.cpuSeconds, 0),
              commands: tree.map((p) => commandName(p.command)),
              processes: tree.map((p) => ({ command: p.command, startedAt: lookedAt - p.ageSeconds * 1000 })),
            };
          }),
        );
      }
      return out;
    },
  };
}

// ageSeconds: how long ago it started (ps's etime).
type Proc = { pid: number; ppid: number; cpuSeconds: number; ageSeconds: number; command: string };

export function parsePs(text: string): Proc[] {
  const out: Proc[] = [];
  for (const line of text.split('\n')) {
    const m = /^\s*(\d+)\s+(\d+)\s+(\S+)\s+(\S+)\s+(.*)$/.exec(line);
    if (m) out.push({ pid: Number(m[1]), ppid: Number(m[2]), cpuSeconds: cpuTime(m[3]), ageSeconds: cpuTime(m[4]), command: m[5] });
  }
  return out;
}

// ps's TIME and ETIME: [[dd-]hh:]mm:ss[.ss]
export function cpuTime(s: string): number {
  const [days, rest] = s.includes('-') ? s.split('-') : ['0', s];
  const parts = rest.split(':').map(Number);
  let secs = 0;
  for (const p of parts) secs = secs * 60 + (Number.isFinite(p) ? p : 0);
  return Number(days) * 86_400 + secs;
}

// The CLI the runner started for this session: its own arguments name it
// (--resume=<id> or --session-id=<id>). A background shell can mention
// the id too (reading another task's output), so the flag is required.
export function isCliFor(command: string, sessionId: string): boolean {
  return new RegExp(`--(?:resume|session-id)[= ]${sessionId}(?:\\s|$)`).test(command) && !command.includes('/shell-snapshots/');
}

export function parseLsof(text: string): Map<number, string> {
  const out = new Map<number, string>();
  let pid = 0;
  for (const line of text.split('\n')) {
    if (line.startsWith('p')) pid = Number(line.slice(1));
    else if (line.startsWith('n') && pid) out.set(pid, line.slice(1));
  }
  return out;
}

function subtree(root: Proc, kids: Map<number, Proc[]>): Proc[] {
  const out: Proc[] = [];
  const stack = [root];
  while (stack.length) {
    const p = stack.pop()!;
    out.push(p);
    stack.push(...(kids.get(p.pid) ?? []));
  }
  return out;
}

function commandName(command: string): string {
  return path.basename(command.split(/\s+/)[0] ?? '');
}

function run(cmd: string, args: string[], tolerateFailure = false): Promise<string> {
  return new Promise((resolve, reject) => {
    execFile(cmd, args, { maxBuffer: 32 * 1024 * 1024, timeout: 60_000 }, (err, stdout) => {
      if (err && !(tolerateFailure && stdout)) {
        if (tolerateFailure) resolve('');
        else reject(err);
      } else resolve(stdout);
    });
  });
}
