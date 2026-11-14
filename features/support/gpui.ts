// The GPUI app: the debug build with the `automation` feature, on the mock
// runner, driven over a local socket that only that build opens. On a Linux
// server with no display it gets a virtual one.
import { spawn, type ChildProcess } from 'node:child_process';
import fs from 'node:fs';
import net from 'node:net';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { sleep, type Driver, type Dump } from './driver.ts';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const exe = process.platform === 'win32' ? 'threadbear-gpui.exe' : 'threadbear-gpui';
// TB_GPUI_PROFILE=release for a release build (the perf tools).
const profile = process.env.TB_GPUI_PROFILE ?? 'debug';
const binary = process.env.TB_GPUI_BIN ?? path.join(process.env.CARGO_TARGET_DIR ?? path.join(root, 'desktop/target'), profile, exe);

// The Terminal's shell (terminal.feature) is the login shell, $SHELL.
// GitHub's Macs give their runner macOS's own bash 3.2, which never asks for
// pastes to be marked, so there the app rightly copies every command a reply
// would put at the prompt, and the scenarios about the prompt can't happen
// (11 Nov). Every Mac since Catalina starts people in zsh, as Mike's and
// Tinker Desk's do, so on a Mac the specs' shell is zsh, with a config of its
// own (ZDOTDIR) rather than whoever's running them: a short prompt, so a
// command fits on its line beside it, and no first-run questions.
let zdotdir: string | undefined;
function specShell(): NodeJS.ProcessEnv {
  if (process.platform !== 'darwin' || !fs.existsSync('/bin/zsh')) return {};
  if (!zdotdir) {
    const dir = fs.mkdtempSync(path.join(fs.realpathSync(os.tmpdir()), 'tb-spec-zsh-'));
    fs.writeFileSync(path.join(dir, '.zshrc'), "PROMPT='%1~ %# '\n");
    process.on('exit', () => fs.rmSync(dir, { recursive: true, force: true }));
    zdotdir = dir;
  }
  return { SHELL: '/bin/zsh', ZDOTDIR: zdotdir };
}

/** The folders `keepEvidence` makes: its ISO time, then mkdtemp's six characters. */
const KEPT_NAME = /^\d{4}-\d\d-\d\dT\d\d-\d\d-\d\d-\d{3}Z-[A-Za-z0-9]{6}$/;

/**
 * What the app printed, less each panic's message. Rust's own panic hook,
 * which runs after the app's (crash_reporting.rs), prints the message as it
 * was, and it can hold any text the app had; the kept record's is scrubbed,
 * and `evidence` shows that one instead.
 */
export function printed(output: string): string {
  return output.replace(/(panicked at [^\n]*\n)[\s\S]*?(?=\nnote: |\n\n|\nthreadbear: |$)/g, '$1(its message is left out here: see the panic record)');
}

async function waitForFile(file: string, what: string, ms: number): Promise<void> {
  const until = Date.now() + ms;
  while (!fs.existsSync(file)) {
    if (Date.now() > until) throw new Error(`${what} didn't start`);
    await sleep(50);
  }
}

export class GpuiDriver implements Driver {
  readonly name = 'gpui' as const;
  private home = '';
  private children: ChildProcess[] = [];
  private commands = new WeakMap<ChildProcess, string>();
  private socket?: net.Socket;
  private token = '';
  private buffer = '';
  private waiting: { reply: (line: string) => void; fail: (e: Error) => void }[] = [];
  /** The folders `evidence` kept copies in, one per failure worth keeping. */
  readonly keptEvidence: string[] = [];
  private log = '';

  private appProcess?: ChildProcess;
  private display?: string;

  private child(cmd: string, args: string[], env: NodeJS.ProcessEnv): ChildProcess {
    // A process group of its own: `dbus-run-session`'s bus and the portals it
    // starts are grandchildren that outlive a plain kill, and the cloud box
    // ended up with thousands of them (9 Nov, "can't spawn worker thread").
    // Register before exec, so even KILL of the measurement worker leaves the
    // perf reaper enough to stop an app that has been reparented.
    const registry = process.platform === 'win32' ? undefined : process.env.THREADBEAR_PERF_CHILDREN;
    const c = spawn(registry ? 'perl' : cmd,
      registry ? [path.join(root, 'scripts/perf-children.pl'), '--child', registry, cmd, ...args] : args,
      { env: { ...process.env, ...env }, stdio: ['ignore', 'pipe', 'pipe'], detached: process.platform !== 'win32' });
    this.commands.set(c, cmd);
    c.stdout?.on('data', (d) => (this.log += d));
    c.stderr?.on('data', (d) => (this.log += d));
    this.children.push(c);
    return c;
  }

  /** Stops a child and everything it started: the whole process group it leads (see `child`). */
  private async killChild(c: ChildProcess): Promise<void> {
    if (process.platform === 'win32') {
      // Killing just the app leaves its ConPTY shell holding its working
      // directory open. This is our own child's tree, never another app's.
      if (c.pid && c.exitCode === null && c.signalCode === null) {
        await new Promise<void>((resolve, reject) => {
          const kill = spawn('taskkill', ['/PID', String(c.pid), '/T', '/F'], { stdio: 'ignore' });
          kill.once('error', reject);
          kill.once('exit', () => resolve());
        });
      }
      return;
    }
    if (c.pid && process.platform !== 'win32') {
      try {
        process.kill(-c.pid, 'SIGTERM');
        return;
      } catch {
        // Already gone.
      }
    }
    c.kill();
  }

  /** Starts the app itself and connects to its automation socket. */
  private async launchApp(): Promise<void> {
    const file = path.join(this.home, `automation-${process.pid}.json`);
    fs.rmSync(file, { force: true });
    const env: NodeJS.ProcessEnv = {
      THREADBEAR_HOME: this.home,
      THREADBEAR_AUTOMATION: file,
      // Motion plays whatever the machine's Reduce Motion switch says:
      // GitHub's Macs have it on, and the scenarios about the panels gliding
      // and the archive flight failed only there (12 Nov). TB_REDUCE_MOTION=on
      // to see the reduced versions.
      THREADBEAR_REDUCE_MOTION: process.env.TB_REDUCE_MOTION ?? 'off',
      ...specShell(),
    };
    if (process.platform === 'linux' && !process.env.DISPLAY && !process.env.WAYLAND_DISPLAY) {
      // A server: a virtual screen, and a session bus of its own for the
      // desktop settings and portals the app asks for.
      this.display ??= `:${100 + (process.pid % 400)}`;
      if (!this.children.some((c) => this.commands.get(c) === 'Xvfb')) {
        this.child('Xvfb', [this.display, '-screen', '0', '1400x900x24'], {});
        await sleep(400);
      }
      this.appProcess = this.child('dbus-run-session', ['--', binary], { ...env, DISPLAY: this.display, NO_AT_BRIDGE: '1', LIBGL_ALWAYS_SOFTWARE: '1' });
    } else {
      this.appProcess = this.child(binary, [], env);
    }
    try {
      await waitForFile(file, 'The app', 30_000);
    } catch (e) {
      throw new Error(`${(e as Error).message}. It said:\n${this.log.slice(-4000)}`);
    }
    const { port, token } = JSON.parse(fs.readFileSync(file, 'utf8')) as { port: number; token: string };
    this.token = token;
    this.socket?.destroy();
    this.waiting = [];
    this.buffer = '';
    const socket = await new Promise<net.Socket>((resolve, reject) => {
      const s = net.connect(port, '127.0.0.1', () => resolve(s));
      s.once('error', reject);
    });
    this.socket = socket;
    socket.setEncoding('utf8');
    // An app that quits (a scenario's `quit`) closes its end: for the step
    // waiting on it to notice, not a crash of the whole run.
    socket.on('error', () => {});
    socket.on('data', (chunk: string) => {
      this.buffer += chunk;
      let at: number;
      while ((at = this.buffer.indexOf('\n')) >= 0) {
        const line = this.buffer.slice(0, at);
        this.buffer = this.buffer.slice(at + 1);
        this.waiting.shift()?.reply(line);
      }
    });
    // A command waiting when the app's end closes gets no answer ever: fail
    // it now, with what's known, rather than at its timeout. Only this
    // socket's: a relaunch's old one closes after the new one is up.
    socket.on('close', () => {
      if (this.socket !== socket) return;
      for (const w of this.waiting.splice(0)) w.fail(new Error('the app closed its automation connection without answering'));
    });
  }

  /** Beside the automation file, where a scripted app writes its log (app_log.rs). */
  async appLog(): Promise<string> {
    try {
      return fs.readFileSync(path.join(this.home, 'app.log'), 'utf8');
    } catch {
      return '';
    }
  }

  /** Quits the app and opens it again on the same runner; `between` runs
   * while it's closed (a spec changing what it remembers, say). */
  async relaunch(between?: () => void): Promise<void> {
    this.socket?.destroy();
    this.socket = undefined;
    const old = this.appProcess;
    if (old) {
      const gone = new Promise<void>((done) => old.once('exit', () => done()));
      await this.killChild(old);
      await Promise.race([gone, sleep(4000)]);
      this.children = this.children.filter((c) => c !== old);
    }
    between?.();
    await this.launchApp();
  }

  async start(): Promise<void> {
    if (!fs.existsSync(binary)) throw new Error(`No GPUI app at ${binary}: cargo build -p threadbear-gpui --features automation`);
    // TB_RUNNER_HOME points at a real runner's data folder (a dev runner's), for
    // a one-off check that the app works with the real thing and not just the mock.
    this.home = process.env.TB_RUNNER_HOME ?? fs.mkdtempSync(path.join(fs.realpathSync(os.tmpdir()), 'tb-spec-'));
    const ready = path.join(this.home, 'mock-ready');
    // TB_MOCK_FIXTURES swaps the demo threads for that folder's, for one-off checks.
    // TB_MOCK_REMOTE: a scenario about other machines (@remote-threads).
    const fixtures = process.env.TB_MOCK_FIXTURES ? ['--fixtures', process.env.TB_MOCK_FIXTURES] : [];
    if (!process.env.TB_RUNNER_HOME) {
      this.child(process.execPath, [path.join(root, 'app/Tools/mock-runner/server.mjs'), '--data-dir', this.home, '--port', '0', '--quiet', '--ready-file', ready, ...fixtures], { TB_MOCK_NO_REMOTE: process.env.TB_MOCK_REMOTE ? '' : '1' });
      await waitForFile(ready, 'The mock runner', 15_000);
    }
    await this.launchApp();
  }

  /** The app's process, for tools that need to find its window. */
  get pid(): number | undefined {
    return this.appProcess?.pid;
  }

  /** One command's answer. A command that can't be answered (no connection,
   * the connection closing, the time running out) rejects with what the app
   * left behind: see `evidence`. */
  private async ask(body: Record<string, unknown>, timeoutSeconds: number): Promise<Record<string, unknown>> {
    try {
      return await new Promise((resolve, reject) => {
        const socket = this.socket;
        if (!socket || socket.destroyed) return reject(new Error('there is no connection to the app'));
        const timer = setTimeout(() => reject(new Error(`timed out after ${timeoutSeconds}s`)), timeoutSeconds * 1000);
        this.waiting.push({
          reply: (line) => {
            clearTimeout(timer);
            resolve(JSON.parse(line) as Record<string, unknown>);
          },
          fail: (e) => {
            clearTimeout(timer);
            reject(e);
          },
        });
        socket.write(`${JSON.stringify({ ...body, token: this.token })}\n`);
      });
    } catch (e) {
      throw new Error(`${body.cmd}: ${(e as Error).message}\n\n${await this.evidence(true)}`);
    }
  }

  async do(cmd: Record<string, unknown>, timeoutSeconds = 30): Promise<string> {
    if (cmd.cmd === 'click' || cmd.cmd === 'label-point') await this.settle();
    const r = await this.ask(cmd, timeoutSeconds);
    if (r.error) throw new Error(`${cmd.cmd}: ${r.error as string}${await this.evidenceFor(r)}`);
    return typeof r.result === 'string' ? r.result : JSON.stringify(r.result ?? '');
  }

  /**
   * What goes under an error the app answered with. One that says it never
   * ran the command (`unanswered`, or an older build's "the app stopped") gets
   * everything, kept; an ordinary refusal ("no button called X", which specs
   * poll for) only the panics, if the app has kept any.
   */
  private async evidenceFor(r: Record<string, unknown>): Promise<string> {
    if (r.unanswered || r.error === 'the app stopped') return `\n\n${await this.evidence(true)}`;
    return this.panics().length ? `\n\n${await this.evidence(false)}` : '';
  }

  /** The panics the app's hook kept beside its automation file (crash_reporting.rs), oldest first. */
  private panics(): string[] {
    const dir = path.join(this.home, 'crashes');
    try {
      return fs.readdirSync(dir).filter((n) => n.startsWith('panic-') && n.endsWith('.json')).sort().map((n) => path.join(dir, n));
    } catch {
      return [];
    }
  }

  /**
   * Why a command failed, as far as the driver can see, bounded so a failure
   * message stays readable: whether the app's process is still running or how
   * it ended, the newest panics its hook kept (already scrubbed of the app's
   * text, crash_report.rs), the end of its log (command names, never their
   * words) and of what it printed. The spec's temp folder goes at teardown,
   * panics and log with it, so `keep` copies them to a private folder first
   * and names it (TB_SPEC_EVIDENCE, or tb-spec-evidence in the temp folder).
   * A terminal test on the cloud box once got only "the app stopped" (13 Nov).
   */
  async evidence(keep: boolean): Promise<string> {
    const p = this.appProcess;
    const state = !p ? 'The app never started.'
      : p.exitCode !== null ? `The app (pid ${p.pid}) exited with code ${p.exitCode}.`
      : p.signalCode !== null ? `The app (pid ${p.pid}) was killed by ${p.signalCode}.`
      : `The app (pid ${p.pid}) is still running.`;
    const parts = [state];
    const panics = this.panics();
    for (const file of panics.slice(-3)) {
      try {
        const r = JSON.parse(fs.readFileSync(file, 'utf8')) as { message?: string; location?: string; main_thread?: boolean; frames?: unknown[] };
        const frames = (r.frames ?? []).slice(0, 8).map((f) => `    ${typeof f === 'string' ? f : JSON.stringify(f)}`.slice(0, 200));
        parts.push([`It panicked${r.main_thread ? ' on its main thread' : ''} at ${r.location || 'an unknown place'}: ${(r.message ?? '').slice(0, 500)}`, ...frames].join('\n'));
      } catch {
        parts.push(`A panic record it kept couldn't be read: ${path.basename(file)}`);
      }
    }
    if (panics.length > 3) parts.push(`(and ${panics.length - 3} older panics)`);
    const log = (await this.appLog()).trimEnd().split('\n').slice(-25).join('\n').slice(-2500);
    if (log) parts.push(`The end of its log:\n${log}`);
    const said = printed(this.log).trimEnd().slice(-2500);
    if (said) parts.push(`The end of what it printed:\n${said}`);
    if (keep && this.home) {
      const dir = this.keepEvidence(panics);
      if (dir) parts.push(`Kept in ${dir}`);
    }
    return parts.join('\n\n');
  }

  /** Copies the panics, app log and output out of the temp folder that teardown removes. Keeps the newest 20 runs' worth. */
  private keepEvidence(panics: string[]): string | undefined {
    try {
      const root = process.env.TB_SPEC_EVIDENCE ?? path.join(fs.realpathSync(os.tmpdir()), 'tb-spec-evidence');
      fs.mkdirSync(root, { recursive: true, mode: 0o700 });
      const dir = fs.mkdtempSync(path.join(root, `${new Date().toISOString().replace(/[:.]/g, '-')}-`));
      if (panics.length) fs.mkdirSync(path.join(dir, 'crashes'), { mode: 0o700 });
      for (const file of panics) fs.copyFileSync(file, path.join(dir, 'crashes', path.basename(file)));
      const log = path.join(this.home, 'app.log');
      if (fs.existsSync(log)) fs.copyFileSync(log, path.join(dir, 'app.log'));
      fs.writeFileSync(path.join(dir, 'output.txt'), printed(this.log).slice(-64_000), { mode: 0o600 });
      this.keptEvidence.push(dir);
      // Only this driver's folders: TB_SPEC_EVIDENCE may be a folder with other things in it.
      const runs = fs.readdirSync(root).filter((n) => KEPT_NAME.test(n)).sort();
      for (const old of runs.slice(0, Math.max(0, runs.length - 20))) fs.rmSync(path.join(root, old), { recursive: true, force: true });
      return dir;
    } catch {
      return undefined;
    }
  }

  /**
   * Waits for the panels' glide and any archive flight to finish before a
   * click reads where its control is, as a person waits for things to stop
   * moving. A click mid-glide read "Go to it" where it was a frame before and
   * landed on "Show it here" as the note slid over (12 Nov). One small
   * question while nothing's moving; at most 3 seconds, then it clicks
   * anyway. An app without the `motion` command says so and isn't waited for.
   */
  private async settle(): Promise<void> {
    const until = Date.now() + 3000;
    while (Date.now() < until) {
      const r = await this.ask({ cmd: 'motion' }, 5);
      if (r.error || !(r.result as { moving?: boolean } | undefined)?.moving) return;
      await sleep(30);
    }
  }

  async dump(): Promise<Dump> {
    const r = await this.ask({ cmd: 'dump' }, 30);
    if (r.error) throw new Error(`dump: ${r.error as string}${(await this.evidenceFor(r)) || `\n${this.log.slice(-3000)}`}`);
    return r.result as Dump;
  }

  async shot(file: string): Promise<void> {
    await this.do({ cmd: 'screenshot', path: file });
  }

  async stop(): Promise<void> {
    if (process.platform === 'win32' && this.socket && !this.socket.destroyed && this.appProcess?.exitCode === null && this.appProcess?.signalCode === null) {
      const gone = new Promise<void>((resolve) => this.appProcess!.once('exit', () => resolve()));
      // Let the app close and wait for its ConPTY readers before forcing its
      // tree down. Also covers scenarios that quit the app before teardown.
      await this.do({ cmd: 'quit' }, 5).catch(() => {});
      await Promise.race([gone, sleep(7000)]);
    }
    this.socket?.destroy();
    for (const c of this.children.splice(0).reverse()) {
      const exited = c.exitCode !== null || c.signalCode !== null ? Promise.resolve() : new Promise<void>((resolve) => c.once('exit', () => resolve()));
      await this.killChild(c);
      await Promise.race([exited, sleep(5000)]);
      // dbus-run-session's children can outlive it holding these pipes open,
      // and Node then waits on them for ever.
      c.stdout?.destroy();
      c.stderr?.destroy();
    }
    await sleep(100);
    if (this.home && !process.env.TB_RUNNER_HOME) await fs.promises.rm(this.home, { recursive: true, force: true, maxRetries: 10, retryDelay: 100 });
  }
}
