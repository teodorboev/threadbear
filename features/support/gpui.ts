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
  private socket?: net.Socket;
  private token = '';
  private buffer = '';
  private waiting: ((line: string) => void)[] = [];
  private log = '';

  private appProcess?: ChildProcess;
  private display?: string;

  private child(cmd: string, args: string[], env: NodeJS.ProcessEnv): ChildProcess {
    // A process group of its own: `dbus-run-session`'s bus and the portals it
    // starts are grandchildren that outlive a plain kill, and the cloud box
    // ended up with thousands of them (9 Nov, "can't spawn worker thread").
    const c = spawn(cmd, args, { env: { ...process.env, ...env }, stdio: ['ignore', 'pipe', 'pipe'], detached: process.platform !== 'win32' });
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
      if (!this.children.some((c) => c.spawnargs[0] === 'Xvfb')) {
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
    this.socket = await new Promise<net.Socket>((resolve, reject) => {
      const s = net.connect(port, '127.0.0.1', () => resolve(s));
      s.once('error', reject);
    });
    this.socket.setEncoding('utf8');
    // An app that quits (a scenario's `quit`) closes its end: for the step
    // waiting on it to notice, not a crash of the whole run.
    this.socket.on('error', () => {});
    this.socket.on('data', (chunk: string) => {
      this.buffer += chunk;
      let at: number;
      while ((at = this.buffer.indexOf('\n')) >= 0) {
        const line = this.buffer.slice(0, at);
        this.buffer = this.buffer.slice(at + 1);
        this.waiting.shift()?.(line);
      }
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

  async relaunch(): Promise<void> {
    this.socket?.destroy();
    this.socket = undefined;
    const old = this.appProcess;
    if (old) {
      const gone = new Promise<void>((done) => old.once('exit', () => done()));
      await this.killChild(old);
      await Promise.race([gone, sleep(4000)]);
      this.children = this.children.filter((c) => c !== old);
    }
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

  private ask(body: Record<string, unknown>, timeoutSeconds: number): Promise<Record<string, unknown>> {
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => reject(new Error(`${body.cmd} timed out. The app said:\n${this.log.slice(-2000)}`)), timeoutSeconds * 1000);
      this.waiting.push((line) => {
        clearTimeout(timer);
        resolve(JSON.parse(line) as Record<string, unknown>);
      });
      this.socket?.write(`${JSON.stringify({ ...body, token: this.token })}\n`);
    });
  }

  async do(cmd: Record<string, unknown>, timeoutSeconds = 30): Promise<string> {
    if (cmd.cmd === 'click' || cmd.cmd === 'label-point') await this.settle();
    const r = await this.ask(cmd, timeoutSeconds);
    if (r.error) throw new Error(`${cmd.cmd}: ${r.error as string}`);
    return typeof r.result === 'string' ? r.result : JSON.stringify(r.result ?? '');
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
    if (r.error) throw new Error(`dump: ${r.error as string}\n${this.log.slice(-3000)}`);
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
