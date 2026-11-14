// The runner reinstalling its own LaunchAgent: install-runner.sh, run for
// it, when launch.mjs, the plist or Threadbear Runner.app is behind main's
// (launchAgentBehind in update.ts). Releases never change those, so until
// 14 Nov 2026 the Updates panel showed the command for Mike to copy into a
// terminal, with a path that wasn't even his checkout's. Now the Updates
// panel's Reinstall now (runner.reinstall) does it, and with the
// selfReinstall experiment on, the runner does it by itself once no
// thread is busy.
//
// install-runner.sh stops this runner's launchd job, and launchd stops the
// job's whole process group with it, so the script can't run as a child of
// the runner. It runs as a launchd job of its own instead (a one-shot
// plist under <home>/runner/reinstall, bootstrapped into the same GUI
// domain), or a transient systemd unit on Linux. Its output goes to
// reinstall.log and its exit code to `result`, which this runner (or the
// one it started) reads. The script does its own checks: busy threads
// (exit 75, tried again once they finish), main's tip, the new runner
// coming up, and putting the old setup back if it doesn't.
import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import type { Clock, TimerHandle } from './clock.ts';
import type { Logger } from './log.ts';

export const DEFAULT_LABEL = 'com.mikerosoft.threadbear.runner';
// install-runner.sh's exit for "threads are busy, nothing changed".
export const BUSY_EXIT = 75;
// How long a reinstall with no result yet counts as running when whether
// its job is still going can't be asked (jobAlive). Otherwise it runs as
// long as it runs: a check host can take hours, and a Mac asleep halfway
// would pass any limit.
export const RUNNING_LIMIT_MS = 30 * 60_000;
// Right after it's started, launchd may not say it's running yet.
export const START_GRACE_MS = 15_000;
// How often a running reinstall's result is looked for, and how often one
// waiting for busy threads checks whether they've finished.
export const POLL_MS = 5_000;
export const WAIT_POLL_MS = 60_000;

// runner.updates' `reinstall` (docs/protocol.md, "Runner updates").
export type ReinstallStatus = {
  // waiting: for busy threads to finish; running: install-runner.sh is
  // going; failed: it stopped, `reason` says why.
  state: 'waiting' | 'running' | 'failed';
  reason?: string;
  logFile?: string;
  // The runner does it by itself (the selfReinstall experiment), rather
  // than because someone pressed Reinstall now.
  auto?: true;
};

// What's kept in <home>/runner/reinstall/state.json between runners.
type Saved = {
  // launcherKey of what it was reinstalling for, so a failed one isn't
  // tried again by itself until main's launcher changes.
  key?: string;
  // Reinstall now was pressed, so it goes ahead once threads finish even
  // with the experiment off.
  requested?: boolean;
  startedAt?: string;
  // The job's label, for asking whether it's still going.
  label?: string;
  // Set once the result has been read.
  finished?: { code: number; at: string; reason?: string };
};

// The job to start, for spawnJob (tests replace it).
export type JobSpec = {
  label: string;
  args: string[];
  env: Record<string, string>;
  logFile: string;
  resultFile: string;
  dir: string;
};

export type ReinstallDeps = {
  home: string;
  // The checkout this release was built from (release.checkout), whose
  // install-runner.sh runs. Undefined: a runner from a checkout, which
  // doesn't run releases, so nothing to do.
  checkout?: string;
  clock: Clock;
  log: Logger;
  // Why the LaunchAgent is behind main, and a key for what main has
  // (updater.status()), or undefined when it isn't.
  behind: () => { reason: string; key: string } | undefined;
  // No thread busy, as a restart would wait for (Restarter.busyThreads).
  idle: () => boolean;
  // The selfReinstall experiment.
  auto: () => boolean;
  onChange?: () => void;
  env?: NodeJS.ProcessEnv;
  platform?: NodeJS.Platform;
  // Starts the job; an error message when it couldn't.
  spawnJob?: (job: JobSpec) => string | undefined;
  // Whether the job is still going; undefined when it can't be told.
  jobAlive?: (label: string) => boolean | undefined;
};

export function reinstallDir(home: string): string {
  return path.join(home, 'runner', 'reinstall');
}

export function reinstallLogPath(home: string): string {
  return path.join(home, 'runner', 'reinstall.log');
}

// A key for main's launcher: launch.mjs and the launcher app's sources.
export function launcherKey(mainRunner: string): string {
  const h = crypto.createHash('sha256');
  for (const f of ['launch.mjs', 'launcher/main.swift', 'launcher/Info.plist', 'launcher/build-launcher.sh']) {
    try {
      h.update(fs.readFileSync(path.join(mainRunner, f)));
    } catch {
      h.update('-');
    }
    h.update('\0');
  }
  return h.digest('hex').slice(0, 16);
}

// install-runner.sh's arguments for this runner: a sandbox (a data dir
// other than the default) names its own and its label; the app is the one
// the plist runs now, so a reinstall rebuilds that one.
export function installArgs(checkout: string, home: string, env: NodeJS.ProcessEnv, platform: NodeJS.Platform): string[] {
  const args = [path.join(checkout, 'runner', 'scripts', 'install-runner.sh')];
  const defaultHome = path.join(env.HOME ?? '', 'Library', 'Application Support', 'Threadbear');
  const label = jobLabel(env, platform);
  if (path.resolve(home) !== path.resolve(defaultHome) || label !== DEFAULT_LABEL) args.push('--home', home, '--label', label);
  if (env.THREADBEAR_RUNNER_APP) args.push('--runner-app', env.THREADBEAR_RUNNER_APP);
  if (env.THREADBEAR_PORT) args.push('--port', env.THREADBEAR_PORT);
  return args;
}

// This runner's launchd label: launchd sets XPC_SERVICE_NAME to it for
// its jobs and their children. On Linux, the systemd unit it runs in, from
// its cgroup (install-runner.sh names the unit <label>.service).
export function jobLabel(env: NodeJS.ProcessEnv, platform: NodeJS.Platform, cgroup: () => string = readCgroup): string {
  const ok = (s: string | undefined): s is string => Boolean(s && /^[A-Za-z0-9._-]+$/.test(s) && s !== '0');
  if (platform === 'darwin') return ok(env.XPC_SERVICE_NAME) ? env.XPC_SERVICE_NAME : DEFAULT_LABEL;
  if (platform === 'linux') {
    const unit = /\/([^/\s]+)\.service\s*$/m.exec(cgroup())?.[1];
    if (ok(unit)) return unit;
  }
  return DEFAULT_LABEL;
}

function readCgroup(): string {
  try {
    return fs.readFileSync('/proc/self/cgroup', 'utf8');
  } catch {
    return '';
  }
}

// The environment install-runner.sh gets: enough to find Node, git, swiftc
// and the login shell's PATH, and nothing of a thread's (it refuses to run
// from inside one) or a dev runner's THREADBEAR_HOME.
export function jobEnv(env: NodeJS.ProcessEnv, resultFile: string): Record<string, string> {
  // No THREADBEAR_NODE: process.execPath is Node's real path, a Homebrew
  // Cellar folder that `brew upgrade` deletes while this runner keeps
  // running from it. The plist's PATH starts with the folder of the Node it
  // pins (the opt/ link for Homebrew's), so install-runner.sh finds that.
  const out: Record<string, string> = { THREADBEAR_INSTALL_QUIET: '1', TB_REINSTALL_RESULT: resultFile };
  for (const k of ['HOME', 'USER', 'LOGNAME', 'SHELL', 'PATH', 'LANG', 'TMPDIR', 'THREADBEAR_SIGN_IDENTITY']) {
    const v = env[k];
    if (v) out[k] = v;
  }
  out.LANG ??= 'en_US.UTF-8';
  return out;
}

const xml = (s: string) => s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');

// The one-shot plist: runs the script once, writes its exit code to
// TB_REINSTALL_RESULT, and isn't started again.
export function jobPlist(job: JobSpec): string {
  const wrap = '"$@"; echo $? > "$TB_REINSTALL_RESULT"';
  const args = ['/bin/bash', '-c', wrap, 'reinstall', '/bin/bash', ...job.args];
  const env = Object.entries(job.env).map(([k, v]) => `\t\t<key>${xml(k)}</key>\n\t\t<string>${xml(v)}</string>`).join('\n');
  return `<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
\t<key>Label</key>
\t<string>${xml(job.label)}</string>
\t<key>ProgramArguments</key>
\t<array>
${args.map((a) => `\t\t<string>${xml(a)}</string>`).join('\n')}
\t</array>
\t<key>WorkingDirectory</key>
\t<string>${xml(job.dir)}</string>
\t<key>EnvironmentVariables</key>
\t<dict>
${env}
\t</dict>
\t<key>RunAtLoad</key>
\t<true/>
\t<key>KeepAlive</key>
\t<false/>
\t<key>StandardOutPath</key>
\t<string>${xml(job.logFile)}</string>
\t<key>StandardErrorPath</key>
\t<string>${xml(job.logFile)}</string>
</dict>
</plist>
`;
}

// Starts the job with launchd (or systemd-run on Linux). A job by the same
// label from an earlier reinstall is booted out first: it has exited, but
// stays loaded.
export function spawnJob(job: JobSpec, platform: NodeJS.Platform = process.platform): string | undefined {
  const run = (cmd: string, args: string[]) => spawnSync(cmd, args, { encoding: 'utf8', timeout: 20_000 });
  if (platform === 'linux') {
    const unit = `${job.label}.service`;
    run('systemctl', ['--user', 'reset-failed', unit]);
    const props = Object.entries(job.env).map(([k, v]) => `--setenv=${k}=${v}`);
    const r = run('systemd-run', ['--user', '--collect', `--unit=${unit}`, `--working-directory=${job.dir}`, `--property=StandardOutput=append:${job.logFile}`, `--property=StandardError=append:${job.logFile}`, ...props, '/bin/bash', '-c', '"$@"; echo $? > "$TB_REINSTALL_RESULT"', 'reinstall', '/bin/bash', ...job.args]);
    return r.status === 0 ? undefined : (r.stderr || r.error?.message || `systemd-run exited ${r.status}`).trim();
  }
  if (platform !== 'darwin') return `reinstalling by itself isn't done on ${platform}`;
  const plist = path.join(job.dir, `${job.label}.plist`);
  fs.writeFileSync(plist, jobPlist(job));
  const domain = `gui/${process.getuid?.() ?? 501}`;
  run('launchctl', ['bootout', `${domain}/${job.label}`]);
  // bootout returns before the old job is gone, so bootstrap gets a few tries.
  let r = run('launchctl', ['bootstrap', domain, plist]);
  for (let i = 0; r.status !== 0 && i < 10; i++) {
    spawnSync('sleep', ['0.5']);
    r = run('launchctl', ['bootstrap', domain, plist]);
  }
  return r.status === 0 ? undefined : (r.stderr || r.error?.message || `launchctl bootstrap exited ${r.status}`).trim();
}

// Whether the job is still going: launchd's `state = running` (or about to
// be), systemd's active. Undefined when neither can be asked.
export function jobAlive(label: string, platform: NodeJS.Platform = process.platform): boolean | undefined {
  if (platform === 'darwin') {
    const r = spawnSync('launchctl', ['print', `gui/${process.getuid?.() ?? 501}/${label}`], { encoding: 'utf8', timeout: 5_000 });
    if (r.error) return undefined;
    if (r.status !== 0) return false; // not loaded at all
    const state = /^\s*state = (.+)$/m.exec(r.stdout ?? '')?.[1]?.trim();
    return state === undefined ? undefined : state !== 'not running';
  }
  if (platform === 'linux') {
    const r = spawnSync('systemctl', ['--user', 'is-active', `${label}.service`], { encoding: 'utf8', timeout: 5_000 });
    if (r.error) return undefined;
    const said = (r.stdout ?? '').trim();
    return said === 'active' || said === 'activating' || said === 'reloading';
  }
  return undefined;
}

// The last lines of the log that say something, for a failure's reason.
export function lastWords(logFile: string, lines = 3): string | undefined {
  let text = '';
  try {
    const fd = fs.openSync(logFile, 'r');
    try {
      const size = fs.fstatSync(fd).size;
      const len = Math.min(size, 8192);
      const buf = Buffer.alloc(len);
      fs.readSync(fd, buf, 0, len, size - len);
      text = buf.toString('utf8');
    } finally {
      fs.closeSync(fd);
    }
  } catch {
    return undefined;
  }
  const said = text.split('\n').map((l) => l.trim()).filter(Boolean);
  return said.slice(-lines).join(' ') || undefined;
}

export class Reinstaller {
  private deps: ReinstallDeps;
  private saved: Saved;
  private timer?: TimerHandle;
  private stopped = false;
  private lastKey?: string;

  constructor(deps: ReinstallDeps) {
    this.deps = deps;
    this.saved = this.read();
  }

  private get dir(): string {
    return reinstallDir(this.deps.home);
  }

  private get resultFile(): string {
    return path.join(this.dir, 'result');
  }

  private read(): Saved {
    try {
      const v = JSON.parse(fs.readFileSync(path.join(this.dir, 'state.json'), 'utf8'));
      return v && typeof v === 'object' ? v : {};
    } catch {
      return {};
    }
  }

  private write(): void {
    fs.mkdirSync(this.dir, { recursive: true });
    const tmp = path.join(this.dir, `state.json.${process.pid}`);
    fs.writeFileSync(tmp, JSON.stringify(this.saved, null, 2));
    fs.renameSync(tmp, path.join(this.dir, 'state.json'));
  }

  private changed(): void {
    this.deps.onChange?.();
  }

  // Started and not finished: no result yet, and not so long ago that the
  // job must have died.
  // Started and not finished: the job is still going, or (when that can't
  // be asked, or it was only just started) it isn't long since it started.
  private running(): boolean {
    const s = this.saved;
    if (!s.startedAt || s.finished) return false;
    const since = this.deps.clock.now() - Date.parse(s.startedAt);
    if (since < START_GRACE_MS) return true;
    const alive = s.label ? this.alive(s.label) : undefined;
    return alive ?? since < RUNNING_LIMIT_MS;
  }

  // jobAlive, asked at most every couple of seconds (status() is read on
  // every runner.updates).
  private aliveCache?: { label: string; at: number; alive: boolean | undefined };
  private alive(label: string): boolean | undefined {
    const now = this.deps.clock.now();
    const c = this.aliveCache;
    if (c && c.label === label && now - c.at < 2_000) return c.alive;
    const platform = this.deps.platform ?? process.platform;
    const alive = this.deps.jobAlive ? this.deps.jobAlive(label) : jobAlive(label, platform);
    this.aliveCache = { label, at: now, alive };
    return alive;
  }

  private readResult(): number | undefined {
    try {
      const text = fs.readFileSync(this.resultFile, 'utf8').trim();
      return /^\d+$/.test(text) ? Number(text) : undefined;
    } catch {
      return undefined; // not yet
    }
  }

  // Reads the job's result once it's there, or calls a job that's been
  // quiet too long dead. Public for tests.
  collect(): void {
    const s = this.saved;
    if (!s.startedAt || s.finished) return;
    let code = this.readResult();
    const log = reinstallLogPath(this.deps.home);
    if (code === undefined && this.running()) return;
    // It may have written its result and exited since the read above.
    if (code === undefined) code = this.readResult();
    if (code === undefined) {
      s.finished = { code: -1, at: new Date(this.deps.clock.now()).toISOString(), reason: `install-runner.sh stopped without finishing: ${lastWords(log) ?? 'it said nothing'}` };
    } else if (code === BUSY_EXIT) {
      // Threads got busy: nothing changed, so it goes again once they're
      // done (requested stays as it was; the experiment tries again anyway).
      this.deps.log.info('reinstall waited for busy threads');
      s.startedAt = undefined;
      s.finished = undefined;
    } else {
      s.finished = { code, at: new Date(this.deps.clock.now()).toISOString(), reason: code === 0 ? undefined : (lastWords(log) ?? `install-runner.sh exited ${code}`) };
      this.deps.log[code === 0 ? 'info' : 'error']('reinstall finished', { code, reason: s.finished.reason });
    }
    // Done, one way or the other: a failure waits for Try again (or main's
    // launcher changing) rather than going round again.
    if (s.finished) s.requested = false;
    this.write();
    this.changed();
  }

  status(): ReinstallStatus | undefined {
    const behind = this.deps.behind();
    if (!behind || !this.deps.checkout) return undefined;
    const s = this.saved;
    const logFile = reinstallLogPath(this.deps.home);
    if (this.running()) return { state: 'running', logFile };
    if (s.finished && s.key === behind.key) {
      // It ran for what main has now and it's still behind.
      const reason = s.finished.code === 0 ? `install-runner.sh finished, but ${behind.reason.charAt(0).toLowerCase()}${behind.reason.slice(1)}` : s.finished.reason;
      if (!s.requested) return { state: 'failed', reason, logFile };
    }
    if (s.requested) return { state: 'waiting' };
    if (this.deps.auto()) return { state: 'waiting', auto: true };
    return undefined;
  }

  // Reinstall now (runner.reinstall): straight away when no thread is busy,
  // else once they've finished.
  request(): ReinstallStatus | undefined {
    if (!this.deps.checkout) throw new Error("This runner isn't running a release, so there's no LaunchAgent to reinstall.");
    if (!this.deps.behind()) return undefined;
    if (!this.running()) {
      // The last run's result goes, or it would be read as this one's.
      this.saved = { key: this.saved.key, requested: true };
      fs.rmSync(this.resultFile, { force: true });
      this.write();
      this.consider();
    }
    this.changed();
    return this.status();
  }

  // Starts a reinstall when one is due and nothing is busy, and keeps
  // looking while one is running or waiting. Called after each update
  // check, and by its own timer.
  consider(): void {
    if (this.stopped) return;
    this.collect();
    this.timer?.cancel();
    this.timer = undefined;
    // Before anything else: the runner a reinstall started comes up before
    // the script has finished (it waits for this one to be marked good),
    // and before its updater has looked at main.
    if (this.running()) {
      this.timer = this.deps.clock.setTimeout(() => this.consider(), POLL_MS);
      return;
    }
    const behind = this.deps.behind();
    if (behind?.key !== this.lastKey) {
      this.lastKey = behind?.key;
      this.changed();
    }
    if (!behind || !this.deps.checkout) return;
    const s = this.saved;
    const triedThis = s.key === behind.key && s.finished !== undefined;
    const due = s.requested || (this.deps.auto() && !triedThis);
    if (!due) return;
    if (!this.deps.idle()) {
      this.timer = this.deps.clock.setTimeout(() => this.consider(), WAIT_POLL_MS);
      return;
    }
    this.begin(behind.key);
    this.timer = this.deps.clock.setTimeout(() => this.consider(), POLL_MS);
  }

  private begin(key: string): void {
    const { home, checkout, log } = this.deps;
    if (!checkout) return;
    const env = this.deps.env ?? process.env;
    const platform = this.deps.platform ?? process.platform;
    fs.mkdirSync(this.dir, { recursive: true });
    fs.rmSync(this.resultFile, { force: true });
    const logFile = reinstallLogPath(home);
    fs.appendFileSync(logFile, `\n--- ${new Date(this.deps.clock.now()).toISOString()} reinstalling the LaunchAgent\n`);
    const job: JobSpec = { label: `${jobLabel(env, platform)}.reinstall`, args: installArgs(checkout, home, env, platform), env: jobEnv(env, this.resultFile), logFile, resultFile: this.resultFile, dir: this.dir };
    this.saved = { key, requested: this.saved.requested, startedAt: new Date(this.deps.clock.now()).toISOString(), label: job.label };
    this.aliveCache = undefined;
    const err = (this.deps.spawnJob ?? spawnJob)(job, platform);
    if (err) {
      this.saved.finished = { code: -1, at: this.saved.startedAt!, reason: `couldn't start install-runner.sh: ${err}` };
      this.saved.requested = false;
      log.error('could not start the reinstall', { error: err });
    } else {
      log.info('reinstalling the LaunchAgent', { label: job.label, args: job.args });
    }
    this.write();
    this.changed();
  }

  stop(): void {
    this.stopped = true;
    this.timer?.cancel();
    this.timer = undefined;
  }
}
