// The GPUI app for an agent to look at and drive (desktop/scripts/dev-app.sh
// runs this). The Swift app's app/Tools/dev-app.sh, for the app Mike uses now.
//
//   desktop/scripts/dev-app.sh start [--mock [--fixtures DIR]...] [--home DIR] [--live]
//                                    [--appearance light|dark] [--size WxH] [--id NAME]
//                                    [--no-build] [--build-here] [--visible]
//   desktop/scripts/dev-app.sh do '<json>'... [--timeout S]
//   desktop/scripts/dev-app.sh dump [--json]
//   desktop/scripts/dev-app.sh shot <file.png>
//   desktop/scripts/dev-app.sh rebuild [--no-build] [--build-here]
//   desktop/scripts/dev-app.sh log [-f] [-n N] [--mock]
//   desktop/scripts/dev-app.sh stop [--all | --everyone]
//   desktop/scripts/dev-app.sh status
//   (every command but start and status takes --id NAME)
//
// start    gets a debug build with the scripted control (`automation`), starts
//          the mock runner (its demo threads, or only the --fixtures folders:
//          `tb export` puts a thread in app/.fixtures/threads/<id>) or points at
//          a dev runner's --home (runner/scripts/dev-runner.sh), launches the
//          app headless: out of the Dock and the app switcher, its windows
//          fully transparent, letting clicks through and at the back, so
//          nobody sees one (a fork's owner found one behind their windows
//          and took the demo threads for a leak, 15 Nov). It still draws, so
//          dump and shot work. --visible is for a test that needs the window
//          on screen: behind everything, with "test copy" in its title and an
//          orange strip across its top saying what it is (not in shots). It
//          waits until the app's connected with its thread list, and prints
//          the id. In ps and Activity Monitor the app is "Threadbear Headless
//          Tester" (a clone of the build in the instance's folder) and the
//          mock "threadbear-test-mock-runner".
//          The build is heavy, so it goes to the first of runner.json's
//          updateBuildHosts that's a Mac and answers (remote.sh; those are
//          the Macs trusted with what this one runs, so checkHosts don't
//          count), and the binaries come back; it says where. With none it
//          builds here, one build at a time (scripts/one-build.sh) at
//          background priority. --build-here builds here anyway; --no-build
//          uses the last one (or TB_GPUI_BIN).
// do       sends each command, waits for it and prints "<cmd> <result>"; exits
//          1 on an error or a timeout. The commands are the Swift app's
//          (app/README.md, "Seeing and driving it from a script") as far as
//          the GPUI app has them (desktop/gpui/src/automation.rs), with
//          waitFor and click-at among them.
// dump     what the window shows, as text (--json: the whole dump).
// shot     the window as a PNG, drawn by the app itself, so it works behind
//          other windows and with the screen locked.
// rebuild  builds again and restarts the app on the same runner (the mock
//          keeps its state), with the same thread open. A build that fails
//          leaves the running app alone.
// log      the app's own log (connection, RPC errors, automation commands);
//          --mock the mock runner's.
// stop     quits the app and its mock and deletes the instance's files.
//
// What keeps it off Mike's own things: the app refuses the real data folder
// and its live runner unless --live is given (THREADBEAR_AUTOMATION_LIVE),
// which is for Mike to decide; anything it copies goes to a pasteboard of its
// own, never his clipboard; it never posts to his Sentry (a scripted app
// doesn't); and the installed app is never touched.
//
// Instances belong to whoever started them: the Threadbear thread
// (THREADBEAR_THREAD_ID) and this checkout. Without --id a command means your
// newest running instance, never another thread's or another checkout's;
// stop --all stops all of yours (--everyone: every instance on this Mac);
// status lists them all with whose they are, and an id someone else has is
// refused at start. An app quits after 60 minutes with no command
// (DEV_APP_IDLE_QUIT_SECONDS), and its mock with it.
import { spawn, spawnSync, type ChildProcess } from 'node:child_process';
import fs from 'node:fs';
import net from 'node:net';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
export const CHECKOUT = fs.realpathSync(path.join(here, '../..'));
const exe = process.platform === 'win32' ? 'threadbear-gpui.exe' : 'threadbear-gpui';
export const REAL_HOME = path.join(os.homedir(), 'Library', 'Application Support', 'Threadbear');
const IDLE_QUIT_SECONDS = Number(process.env.DEV_APP_IDLE_QUIT_SECONDS ?? 3600);

/** An instance's id, and so its folder's name: never a path (`..` or a `/`). */
export const INSTANCE_ID = /^[A-Za-z0-9_-]{1,40}$/;

/**
 * Every instance's folder: under TMPDIR, so nothing here is in Mike's own folders. A folder only
 * this account can use, or none: it's made here (0700) or was, isn't a symlink, is this user's and
 * no one else can get into it. On a TMPDIR other accounts can write to (Linux's /tmp) the name
 * has the uid in it, and one there already that's anyone else's is refused, never used: another
 * account could make it first and put an instance.json in it whose id reached outside, and the
 * reap deleted that (Daybreak, 13 Nov, finding 116). macOS's TMPDIR is already the user's own.
 */
export function stateRoot(tmp: string = process.env.TMPDIR ?? os.tmpdir()): string {
  const parent = fs.realpathSync(tmp);
  const uid = process.getuid?.();
  // Windows has no uids, and its temp folder is the user's own.
  if (uid === undefined) {
    const dir = path.join(parent, 'threadbear-gpui-dev-app');
    fs.mkdirSync(dir, { recursive: true });
    return dir;
  }
  const up = fs.statSync(parent);
  const shared = (up.mode & 0o022) !== 0;
  // Writable by others without the sticky bit, anyone could rename our folder away and put theirs in its place.
  if (shared && !(up.mode & 0o1000)) throw new Error(`${parent} (TMPDIR) can be changed by other accounts and isn't sticky: set TMPDIR to a folder of your own`);
  // A sticky folder's owner can still rename anyone's folder in it, so it has to be root's or ours.
  if (up.uid !== uid && up.uid !== 0) throw new Error(`${parent} (TMPDIR) belongs to someone else: set TMPDIR to a folder of your own`);
  const dir = path.join(parent, shared ? `threadbear-gpui-dev-app-${uid}` : 'threadbear-gpui-dev-app');
  try {
    fs.mkdirSync(dir, { mode: 0o700 });
  } catch (e) {
    if ((e as NodeJS.ErrnoException).code !== 'EEXIST') throw e;
  }
  const s = fs.lstatSync(dir);
  if (!s.isDirectory() || s.uid !== uid || (s.mode & 0o077) !== 0) {
    throw new Error(`${dir} isn't a folder of this account's own (mode 700), so it's not used: remove it if it's yours, or set TMPDIR to a folder of your own`);
  }
  return dir;
}

export type Instance = {
  id: string;
  thread: string;
  checkout: string;
  created: number;
  mock: boolean;
  fixtures: string[];
  home: string;
  live: boolean;
  appearance?: string;
  size?: string;
  /** --visible: the window on screen (behind everything, labelled), for a test that needs it seen. */
  visible?: boolean;
  binary: string;
  supervisor?: number;
  app?: number;
  /** Each one's start time as ps gives it, so a pid macOS has since given to another process is never signalled. */
  supervisorSince?: string;
  appSince?: string;
};

export type Me = { thread: string; checkout: string };
export const me = (): Me => ({ thread: process.env.THREADBEAR_THREAD_ID ?? process.env.THREADBARE_THREAD_ID ?? '', checkout: CHECKOUT });
export const mine = (i: Instance, who: Me): boolean => i.thread === who.thread && i.checkout === who.checkout;
export const whose = (i: Instance): string => `${i.thread ? `thread ${i.thread.slice(0, 8)}` : 'no thread'}, ${i.checkout}`;

/** Why `start` won't use this data folder: the real one has the live runner. */
export function refuseHome(home: string, real: string, live: boolean): string | null {
  if (live) return null;
  const resolved = (p: string) => {
    try {
      return fs.realpathSync(p);
    } catch {
      return path.resolve(p);
    }
  };
  if (resolved(home) !== resolved(real)) return null;
  return `${home} is the real data folder: its runner is the live one, with Mike's real threads. Use --mock or a dev runner's --home (runner/scripts/dev-runner.sh); --live only if Mike has said so.`;
}

/** An instance's folder: a direct child of the state folder, whatever the id came from. */
function dirOf(id: string): string {
  if (!INSTANCE_ID.test(id)) throw new Error(`not an instance id: ${JSON.stringify(id)}`);
  return path.join(stateRoot(), id);
}

/**
 * The instance in the folder named `id`, or null. Its id is the folder's name, never what the
 * file says, and the fields that become paths or pids have to be what start writes.
 */
export function readInstance(id: string): Instance | null {
  if (typeof id !== 'string' || !INSTANCE_ID.test(id)) return null;
  let i: Instance;
  try {
    i = JSON.parse(fs.readFileSync(path.join(dirOf(id), 'instance.json'), 'utf8')) as Instance;
  } catch {
    return null;
  }
  try {
    Object.assign(i, JSON.parse(fs.readFileSync(path.join(dirOf(id), 'app.json'), 'utf8')));
  } catch {}
  return validInstance(i, id) ? i : null;
}

export function validInstance(i: unknown, id: string): i is Instance {
  if (!i || typeof i !== 'object') return false;
  const r = i as Record<string, unknown>;
  const str = (k: string) => typeof r[k] === 'string';
  const pid = (k: string) => r[k] === undefined || (Number.isInteger(r[k]) && (r[k] as number) > 0);
  return (
    r.id === id &&
    INSTANCE_ID.test(id) &&
    str('thread') &&
    str('checkout') &&
    str('home') &&
    str('binary') &&
    Number.isFinite(r.created) &&
    Array.isArray(r.fixtures) &&
    (r.fixtures as unknown[]).every((f) => typeof f === 'string') &&
    pid('supervisor') &&
    pid('app')
  );
}

function writeInstance(i: Instance): void {
  const file = path.join(dirOf(i.id), 'instance.json');
  fs.writeFileSync(`${file}.tmp`, JSON.stringify(i, null, 2), { mode: 0o600 });
  fs.renameSync(`${file}.tmp`, file);
}

export function allInstances(): Instance[] {
  return fs
    .readdirSync(stateRoot())
    .map(readInstance)
    .filter((i): i is Instance => i !== null)
    .sort((a, b) => b.created - a.created);
}

/** When the process started, as ps says it (the same for the life of the process). */
export function since(pid?: number): string {
  if (!pid) return '';
  const r = spawnSync('ps', ['-p', String(pid), '-o', 'lstart='], { encoding: 'utf8' });
  return r.status === 0 ? r.stdout.trim() : '';
}

/** That process is still the one recorded, not another that has its pid now (Codex's review). */
export const alive = (pid?: number, started?: string) => !!pid && !!started && since(pid) === started;

/** The supervisor's running: it owns the app and the mock, and goes when they do. */
export const running = (i: Instance) => alive(i.supervisor, i.supervisorSince);

/**
 * The instance a command means: --id's, else the caller's newest running one.
 * Never another thread's or checkout's by default: two threads each driving
 * "their" app would otherwise end up driving one.
 */
export function pick(all: Instance[], who: Me, id?: string, isRunning: (i: Instance) => boolean = running): Instance {
  if (id) {
    const found = all.find((i) => i.id === id);
    if (!found) throw new Error(`no instance ${id} (status lists them)`);
    return found;
  }
  const ours = all.filter((i) => mine(i, who) && isRunning(i));
  if (ours[0]) return ours[0];
  const others = all.filter((i) => isRunning(i));
  throw new Error(others.length ? `none of yours is running (${others.map((i) => `${i.id}: ${whose(i)}`).join('; ')}); start one, or pass --id` : 'no instance is running; start one');
}

// A start's reserved folder, given back if it fails before its app is up.
let reserved = '';
function die(message: string): never {
  if (reserved) fs.rmSync(reserved, { recursive: true, force: true });
  console.error(`dev-app: ${message}`);
  process.exit(1);
}

// ------------------------------------------------------------------ building

function hostsIn(key: string): string[] {
  try {
    const v = JSON.parse(fs.readFileSync(path.join(REAL_HOME, 'runner.json'), 'utf8'))[key];
    const list = typeof v === 'string' ? [v] : Array.isArray(v) ? v : [];
    return list.filter((h): h is string => typeof h === 'string' && /^[A-Za-z0-9][A-Za-z0-9._@-]*$/.test(h));
  } catch {
    return [];
  }
}

function binDir(): string {
  // One per checkout, so two checkouts' builds never swap under each other.
  const key = Buffer.from(CHECKOUT).toString('base64url').slice(-24);
  const dir = path.join(stateRoot(), `bin-${key}`);
  fs.mkdirSync(dir, { recursive: true });
  return dir;
}

/** Builds and returns the app's path, or says why it couldn't. */
function build(opts: { noBuild: boolean; here: boolean; logName: string }): string {
  if (process.env.TB_GPUI_BIN) return process.env.TB_GPUI_BIN;
  const bins = binDir();
  const app = path.join(bins, exe);
  if (opts.noBuild) {
    if (fs.existsSync(app)) return app;
    const local = path.join(CHECKOUT, 'desktop/target/debug', exe);
    if (fs.existsSync(local)) return local;
    die(`no build to use: start without --no-build`);
  }
  const logs = path.join(stateRoot(), 'build-logs');
  fs.mkdirSync(logs, { recursive: true });
  const log = path.join(logs, `${opts.logName}.log`);
  const host = opts.here ? undefined : hostsIn('updateBuildHosts').find((h) => spawnSync('ssh', ['-o', 'BatchMode=yes', '-o', 'ConnectTimeout=8', h, 'uname', '-s'], { encoding: 'utf8' }).stdout?.trim() === 'Darwin');
  // An interrupted build may still hold the old file open.
  fs.rmSync(log, { force: true });
  let ok: boolean;
  if (host) {
    const fd = fs.openSync(log, 'w');
    console.error(`Building on ${host} (an updateBuildHosts Mac), so this Mac stays free; the binaries come back here. Log: ${log}`);
    let r;
    try {
      // Write both streams straight to the advertised file, as the local
      // build does, so even a long compile can be followed while it runs.
      r = spawnSync(path.join(CHECKOUT, 'desktop/scripts/remote.sh'), [host, '--', 'bash', '-c', 'cd desktop && nice cargo build -p threadbear-gpui --features automation && echo "BUILT_IN=$CARGO_TARGET_DIR"'], { stdio: ['ignore', fd, fd] });
    } finally {
      fs.closeSync(fd);
    }
    const builtIn = /^BUILT_IN=(\/.+)$/m.exec(fs.readFileSync(log, 'utf8'))?.[1];
    ok = r.status === 0 && !!builtIn;
    if (ok) {
      for (const bin of [exe, 'threadbear-web-helper']) {
        // Copied beside the old one and moved over it, so a running app's
        // file is never half written.
        const tmp = path.join(bins, `.${bin}.new`);
        const c = spawnSync('scp', ['-q', `${host}:${builtIn}/debug/${bin}`, tmp], { encoding: 'utf8' });
        if (c.status !== 0) {
          if (bin === exe) die(`couldn't bring the build back from ${host}: ${c.stderr}`);
          continue;
        }
        fs.chmodSync(tmp, 0o755);
        fs.renameSync(tmp, path.join(bins, bin));
      }
    }
  } else {
    console.error(`${opts.here ? '' : 'No updateBuildHosts Mac answered, so '}building here, one build at a time and at background priority. Log: ${log}`);
    const target = path.join(CHECKOUT, 'desktop/target');
    const cmd = [path.join(CHECKOUT, 'scripts/one-build.sh'), ...(process.platform === 'darwin' ? ['taskpolicy', '-b'] : []), 'cargo', 'build', '-p', 'threadbear-gpui', '--features', 'automation'];
    const fd = fs.openSync(log, 'w');
    const r = spawnSync(cmd[0], cmd.slice(1), { cwd: path.join(CHECKOUT, 'desktop'), stdio: ['ignore', fd, fd] });
    fs.closeSync(fd);
    ok = r.status === 0;
    if (ok) {
      for (const bin of [exe, 'threadbear-web-helper']) {
        const from = path.join(target, 'debug', bin);
        if (!fs.existsSync(from)) continue;
        fs.copyFileSync(from, path.join(bins, `.${bin}.new`));
        fs.renameSync(path.join(bins, `.${bin}.new`), path.join(bins, bin));
      }
    }
  }
  if (!ok) {
    const text = fs.readFileSync(log, 'utf8');
    const errors = text.split('\n').filter((l) => /^(error|warning: unused)|^\s+-->|^\s*\|/.test(l)).slice(-60);
    console.error((errors.length ? errors : text.split('\n').slice(-30)).join('\n'));
    die(`the build failed; the whole log: ${log}`);
  }
  fs.rmSync(log, { force: true });
  return app;
}

// ------------------------------------------------------------------ the app's socket

type Reply = { result?: unknown; error?: string };

async function connect(i: Instance): Promise<{ ask: (cmd: Record<string, unknown>, seconds: number) => Promise<Reply>; close: () => void }> {
  const file = path.join(dirOf(i.id), 'automation.json');
  const { port, token } = JSON.parse(fs.readFileSync(file, 'utf8')) as { port: number; token: string };
  const socket = await new Promise<net.Socket>((resolve, reject) => {
    const s = net.connect(port, '127.0.0.1', () => resolve(s));
    s.once('error', reject);
  });
  socket.setEncoding('utf8');
  let buffer = '';
  const waiting: ((line: string) => void)[] = [];
  socket.on('data', (chunk: string) => {
    buffer += chunk;
    let at: number;
    while ((at = buffer.indexOf('\n')) >= 0) {
      const line = buffer.slice(0, at);
      buffer = buffer.slice(at + 1);
      waiting.shift()?.(line);
    }
  });
  socket.on('error', () => {});
  // Every command counts as use, for the idle quit.
  fs.writeFileSync(path.join(dirOf(i.id), 'last-command'), String(Date.now()));
  return {
    ask: (cmd, seconds) =>
      new Promise((resolve) => {
        const timer = setTimeout(() => resolve({ error: `timed out after ${seconds} s` }), seconds * 1000);
        waiting.push((line) => {
          clearTimeout(timer);
          resolve(JSON.parse(line) as Reply);
        });
        socket.write(`${JSON.stringify({ ...cmd, token })}\n`);
      }),
    close: () => socket.destroy(),
  };
}

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

async function waitForFile(file: string, ms: number, still: () => boolean): Promise<boolean> {
  const until = Date.now() + ms;
  while (!fs.existsSync(file)) {
    if (Date.now() > until || !still()) return false;
    await sleep(100);
  }
  return true;
}

// ------------------------------------------------------------------ the supervisor

/**
 * Runs detached, in a session of its own (an agent's command's process group
 * is stopped when the live runner restarts, which used to leave the Swift
 * app with no mock): starts the mock and the app, starts the app again for a
 * rebuild, quits both after an hour with no command, and goes when the app
 * does.
 */
async function supervise(id: string): Promise<void> {
  const i = readInstance(id);
  if (!i) return;
  const dir = dirOf(id);
  const kids: ChildProcess[] = [];
  let mock: ChildProcess | undefined;
  let app: ChildProcess | undefined;
  // A stop deletes the folder as this goes, so a line that can't be written isn't a reason to stop short.
  const log = (line: string) => {
    try {
      fs.appendFileSync(path.join(dir, 'supervisor.log'), `${new Date().toISOString()} ${line}\n`);
    } catch {}
  };
  let ending = false;
  // Waits for the app and the mock to be gone (a KILL after 5 s), so a stop
  // that waits for this has really stopped them (Codex's second look).
  const end = async (why: string) => {
    if (ending) return;
    ending = true;
    const live = [app, mock].filter((c): c is ChildProcess => !!c?.pid && c.exitCode === null && c.signalCode === null);
    const gone = live.map((c) => new Promise<void>((r) => c.once('exit', () => r())));
    const signal = (sig: NodeJS.Signals) => {
      for (const c of live) {
        try {
          process.kill(-c.pid!, sig);
        } catch {
          c.kill(sig);
        }
      }
    };
    signal('SIGTERM');
    const done = Promise.all(gone);
    if ((await Promise.race([done.then(() => true), sleep(5000).then(() => false)])) === false) {
      signal('SIGKILL');
      await Promise.race([done, sleep(2000)]);
    }
    log(`stopped: ${why}`);
    process.exit(0);
  };
  process.on('SIGTERM', () => void end('asked'));
  process.on('SIGHUP', () => {});
  if (i.mock) {
    const ready = path.join(dir, 'mock-ready');
    const out = fs.openSync(path.join(dir, 'mock.log'), 'a');
    const fixtures = i.fixtures.flatMap((f) => ['--fixtures', f]);
    mock = spawn(process.execPath, [path.join(CHECKOUT, 'app/Tools/mock-runner/server.mjs'), '--data-dir', i.home, '--port', '0', '--ready-file', ready, ...fixtures], { stdio: ['ignore', out, out], detached: true, env: { ...process.env, TB_MOCK_NO_REMOTE: process.env.TB_MOCK_NO_REMOTE ?? '1' } });
    kids.push(mock);
    mock.on('exit', (code) => void end(`the mock runner exited (${code})`));
    if (!(await waitForFile(ready, 20_000, () => mock?.exitCode === null))) return end("the mock runner didn't start");
  }
  const launch = () => {
    const current = readInstance(id) ?? i;
    fs.rmSync(path.join(dir, 'automation.json'), { force: true });
    const out = fs.openSync(path.join(dir, 'app.stderr'), 'a');
    app = spawn(namedCopy(current.binary, dir), [], {
      stdio: ['ignore', out, out],
      detached: true,
      env: {
        ...process.env,
        THREADBEAR_HOME: current.home,
        THREADBEAR_AUTOMATION: path.join(dir, 'automation.json'),
        // Out of the Dock and the app switcher, and no window anyone can see
        // unless --visible (scripted.rs).
        THREADBEAR_KEEP_BEHIND: '1',
        ...(current.visible ? { THREADBEAR_SHOW_WINDOW: '1' } : {}),
        THREADBEAR_PASTEBOARD: `com.mikerosoft.threadbear.automation.gpui-${id}`,
        // Not the machine's web runtime in the real data folder, whose
        // Chromium profile has Mike's sign-ins and whose helpers the installed
        // app runs (Codex's review). One for every instance, so the 135 MB
        // download happens once.
        THREADBEAR_WEB_RUNTIME_DIR: process.env.THREADBEAR_WEB_RUNTIME_DIR || path.join(stateRoot(), 'web-runtime'),
        ...(current.live ? { THREADBEAR_AUTOMATION_LIVE: '1' } : {}),
      },
    });
    // A file of its own, so it never races start's write of instance.json.
    fs.writeFileSync(path.join(dir, 'app.json'), JSON.stringify({ app: app.pid, appSince: since(app.pid) }));
    log(`app ${app.pid} from ${current.binary}`);
    app.on('exit', (code, signal) => {
      const again = path.join(dir, 'relaunch');
      if (fs.existsSync(again)) {
        fs.rmSync(again, { force: true });
        launch();
        return;
      }
      void end(`the app exited (${signal ?? code})`);
    });
  };
  launch();
  // The idle quit: the last command's time, checked every few seconds.
  setInterval(
    () => {
      let last = i.created;
      try {
        last = Number(fs.readFileSync(path.join(dir, 'last-command'), 'utf8')) || last;
      } catch {}
      if (Date.now() - last > IDLE_QUIT_SECONDS * 1000) void end(`no command for ${IDLE_QUIT_SECONDS} s`);
    },
    Math.min(15_000, IDLE_QUIT_SECONDS * 250),
  );
}

// ------------------------------------------------------------------ commands

type Args = { flags: Map<string, string[]>; rest: string[] };
function parse(argv: string[], withValue: string[]): Args {
  const flags = new Map<string, string[]>();
  const rest: string[] = [];
  for (let n = 0; n < argv.length; n++) {
    const a = argv[n];
    if (a.startsWith('--') || /^-[nf]$/.test(a)) {
      const value = withValue.includes(a) ? argv[++n] : '';
      if (value === undefined) die(`${a} needs a value`);
      flags.set(a, [...(flags.get(a) ?? []), value]);
    } else rest.push(a);
  }
  return { flags, rest };
}
const flag = (a: Args, name: string) => a.flags.get(name)?.[0];
const has = (a: Args, name: string) => a.flags.has(name);

async function stopOne(i: Instance): Promise<void> {
  if (running(i)) {
    try {
      process.kill(i.supervisor!, 'SIGTERM');
    } catch {}
    // It stops the app and the mock; the folder goes once it has.
    const until = Date.now() + 10_000;
    while (running(i) && Date.now() < until) await sleep(100);
  }
  // The app's own group too, if the supervisor went first.
  if (alive(i.app, i.appSince)) {
    try {
      process.kill(-i.app!, 'SIGTERM');
    } catch {}
  }
  fs.rmSync(dirOf(i.id), { recursive: true, force: true });
}

/** Instances whose supervisor has gone: theirs after 30 minutes quiet (their log may be wanted), ours at once. */
/** A start still under way there: its dev-app.sh is running (the folder's `starting` says which). */
function starting(id: string): boolean {
  try {
    const { pid, since: started } = JSON.parse(fs.readFileSync(path.join(dirOf(id), 'starting'), 'utf8')) as { pid: number; since: string };
    return alive(pid, started);
  } catch {
    return false;
  }
}

function reap(): void {
  const who = me();
  // A start's folder whose dev-app.sh has gone without finishing, unless it
  // got as far as a supervisor that's running (Codex's second look).
  for (const name of fs.readdirSync(stateRoot())) {
    if (!INSTANCE_ID.test(name) || !fs.existsSync(path.join(dirOf(name), 'starting')) || starting(name)) continue;
    const i = readInstance(name);
    if (i && running(i)) fs.rmSync(path.join(dirOf(name), 'starting'), { force: true });
    else fs.rmSync(dirOf(name), { recursive: true, force: true });
  }
  for (const i of allInstances()) {
    // A start between writing instance.json and recording its supervisor.
    if (running(i) || starting(i.id)) continue;
    let quiet = Date.now() - i.created;
    try {
      quiet = Date.now() - fs.statSync(path.join(dirOf(i.id), 'supervisor.log')).mtimeMs;
    } catch {}
    if (mine(i, who) || quiet > 30 * 60_000) fs.rmSync(dirOf(i.id), { recursive: true, force: true });
  }
}

/** What the app runs as (`ps`, Activity Monitor): anyone wondering what this
 * process is sees it's a test copy, not their own Threadbear. */
export const APP_PROCESS_NAME = 'Threadbear Headless Tester';

/** The build, as a copy named APP_PROCESS_NAME in the instance's folder (a
 * clone on APFS, so no space or time), with the web helper it looks for
 * beside itself. The build itself if copying fails. */
export function namedCopy(binary: string, dir: string): string {
  try {
    const bin = path.join(dir, 'bin');
    fs.mkdirSync(bin, { recursive: true });
    const named = path.join(bin, process.platform === 'win32' ? `${APP_PROCESS_NAME}.exe` : APP_PROCESS_NAME);
    fs.rmSync(named, { force: true });
    fs.copyFileSync(binary, named, fs.constants.COPYFILE_FICLONE);
    fs.chmodSync(named, 0o755);
    const helper = path.join(path.dirname(binary), 'threadbear-web-helper');
    if (fs.existsSync(helper)) {
      fs.rmSync(path.join(bin, 'threadbear-web-helper'), { force: true });
      fs.copyFileSync(helper, path.join(bin, 'threadbear-web-helper'), fs.constants.COPYFILE_FICLONE);
      fs.chmodSync(path.join(bin, 'threadbear-web-helper'), 0o755);
    }
    return named;
  } catch {
    return binary;
  }
}

async function start(a: Args): Promise<void> {
  const who = me();
  reap();
  const home = flag(a, '--home');
  const useMock = has(a, '--mock') || !home;
  if (home && has(a, '--mock')) die('--mock or --home, not both');
  const live = has(a, '--live');
  if (home) {
    const why = refuseHome(home, REAL_HOME, live);
    if (why) die(why);
  }
  const id = flag(a, '--id') ?? `g${Date.now().toString(36).slice(-5)}`;
  if (!/^[A-Za-z0-9_-]{1,40}$/.test(id)) die(`--id is letters, digits, - and _`);
  const existing = readInstance(id);
  if (existing && running(existing)) die(mine(existing, who) ? `${id} is already running (stop it first)` : `${id} is someone else's (${whose(existing)})`);
  if (existing) fs.rmSync(dirOf(id), { recursive: true, force: true });
  const dir = dirOf(id);
  // Taken before the build, and only by one start: two at once with the same
  // --id would otherwise both build, then share one folder (Codex's review).
  try {
    fs.mkdirSync(dir, { mode: 0o700 });
  } catch {
    die(`${id} is being started by another dev-app.sh (status lists it once it's up)`);
  }
  reserved = dir;
  fs.writeFileSync(path.join(dir, 'starting'), JSON.stringify({ pid: process.pid, since: since(process.pid) }));
  const binary = build({ noBuild: has(a, '--no-build'), here: has(a, '--build-here'), logName: id });
  const appearance = flag(a, '--appearance');
  if (appearance && !['light', 'dark'].includes(appearance)) die('--appearance is light or dark');
  const size = flag(a, '--size');
  if (size && !/^\d+x\d+$/.test(size)) die('--size is WxH, like 1240x820');
  const i: Instance = {
    id,
    ...who,
    created: Date.now(),
    mock: useMock,
    fixtures: (a.flags.get('--fixtures') ?? []).map((f) => path.resolve(f)),
    home: useMock ? path.join(dir, 'home') : path.resolve(home!),
    live,
    appearance,
    size,
    visible: has(a, '--visible'),
    binary,
  };
  if (useMock) fs.mkdirSync(i.home, { recursive: true, mode: 0o700 });
  writeInstance(i);
  fs.writeFileSync(path.join(dir, 'last-command'), String(Date.now()));
  const sup = spawn(process.execPath, [fileURLToPath(import.meta.url), 'supervise', id], { detached: true, stdio: 'ignore' });
  sup.unref();
  writeInstance({ ...i, supervisor: sup.pid, supervisorSince: since(sup.pid) });
  // From here a failed start is stopped by ready(), not just deleted.
  reserved = '';
  fs.rmSync(path.join(dir, 'starting'), { force: true });
  await ready(i, 'start');
  console.log(id);
}

/** Waits for the app to take commands and have its thread list, then sets it up as asked. */
async function ready(i: Instance, what: string): Promise<void> {
  const dir = dirOf(i.id);
  const fresh = () => readInstance(i.id);
  const ok = await waitForFile(path.join(dir, 'automation.json'), 60_000, () => running(fresh() ?? i));
  if (!ok) {
    const said = ['supervisor.log', 'app.stderr', 'mock.log'].map((f) => {
      try {
        return `${f}:\n${fs.readFileSync(path.join(dir, f), 'utf8').split('\n').slice(-15).join('\n')}`;
      } catch {
        return '';
      }
    });
    await stopOne(fresh() ?? i);
    die(`the app didn't take commands at ${what}.\n${said.filter(Boolean).join('\n')}`);
  }
  const c = await connect(fresh() ?? i);
  try {
    const r = await c.ask({ cmd: 'waitFor', ready: true, timeout: 30 }, 35);
    if (r.error) die(`the app started but isn't ready: ${r.error} (log shows why)`);
    if (i.appearance) await c.ask({ cmd: 'appearance', mode: i.appearance }, 10);
    if (i.size) {
      const [w, h] = i.size.split('x').map(Number);
      await c.ask({ cmd: 'resize', w, h }, 10);
    }
  } finally {
    c.close();
  }
}

function resultText(r: unknown): string {
  return typeof r === 'string' ? r : JSON.stringify(r);
}

async function doCommands(i: Instance, a: Args): Promise<void> {
  if (!running(i)) die(`${i.id} isn't running (start again)`);
  const timeout = Number(flag(a, '--timeout') ?? 30);
  const c = await connect(i);
  let failed = false;
  try {
    for (const text of a.rest) {
      let cmd: Record<string, unknown>;
      try {
        cmd = JSON.parse(text) as Record<string, unknown>;
      } catch {
        die(`not JSON: ${text}`);
      }
      // A wait gets its own time and a little more.
      const seconds = cmd.cmd === 'waitFor' ? Number(cmd.timeout ?? 10) + 5 : timeout;
      const r = await c.ask(cmd, seconds);
      if (r.error) {
        console.log(`${cmd.cmd} error ${r.error}`);
        failed = true;
        break;
      }
      const out = resultText(r.result);
      console.log(out.startsWith(`${cmd.cmd} `) ? out : `${cmd.cmd} ${out}`);
    }
  } finally {
    c.close();
  }
  if (failed) process.exit(1);
}

type Row = { id: string; kind: string; text?: string; onScreen?: boolean; expanded?: boolean };
/** The dump as a page of text: what an agent usually wants to know. */
export function dumpText(d: Record<string, any>): string {
  const lines: string[] = [];
  const short = (s: unknown, n = 100) => String(s ?? '').replace(/\s+/g, ' ').slice(0, n);
  lines.push(`connection: ${d.connection}${d.banner?.text ? `   banner: ${short(d.banner.text)}` : ''}${d.error ? `   error: ${short(d.error)}` : ''}`);
  const sel = d.selection;
  lines.push(`selection: ${sel ? `${sel.id} (${sel.status}) ${short(sel.title, 60)}` : 'none'}`);
  const sb = d.sidebar ?? {};
  lines.push(`sidebar: ${sb.mode}${sb.loading ? ' (loading)' : ''}, ${sb.snoozed ?? 0} snoozed, ${sb.archived ?? 0} archived`);
  for (const s of sb.sections ?? []) {
    lines.push(`  ${s.name}${s.collapsed ? ' (folded)' : ''}`);
    for (const t of (s.threads ?? []).slice(0, 25)) lines.push(`    ${t.id === sel?.id ? '*' : ' '} ${t.id} ${t.status}${t.needsYou ? ' needs-you' : ''}${t.unread ? ' unread' : ''}  ${short(t.title, 60)}`);
  }
  const tr = d.transcript ?? {};
  lines.push(`transcript: ${tr.threadId ?? 'none'} ${tr.state ?? ''}${tr.atBottom ? ' at bottom' : ''}`);
  for (const r of (tr.rows ?? []) as Row[]) lines.push(`  ${r.onScreen ? '>' : ' '} ${r.id} ${r.kind}${r.expanded === false ? ' (closed)' : ''}  ${short(r.text, 90)}`);
  const c = d.composer ?? {};
  lines.push(`composer (${c.draft}): ${JSON.stringify(c.text ?? '')}${c.chip ? `  on ${c.chip}` : ''}${c.cwd ? `  in ${c.cwd}` : ''}${c.mode ? `  ${c.mode}` : ''}`);
  if (d.dock) lines.push(`dock: ${d.dock}`);
  return lines.join('\n');
}

async function main(argv: string[]): Promise<void> {
  const [command, ...rest] = argv;
  if (command === 'supervise') return supervise(rest[0]);
  if (!command || command === '--help' || command === '-h' || command === 'help') {
    const text = fs.readFileSync(fileURLToPath(import.meta.url), 'utf8').split('\n');
    console.log(text.slice(1, text.findIndex((l) => l.startsWith('import'))).map((l) => l.replace(/^\/\/ ?/, '')).join('\n'));
    return;
  }
  const a = parse(rest, ['--fixtures', '--home', '--appearance', '--size', '--id', '--timeout', '-n']);
  const who = me();
  const target = () => {
    try {
      return pick(allInstances(), who, flag(a, '--id'));
    } catch (e) {
      die((e as Error).message);
    }
  };
  switch (command) {
    case 'start':
      return start(a);
    case 'do':
      if (!a.rest.length) die(`do needs a command, like '{"cmd":"open","id":"demo_upload"}'`);
      return doCommands(target(), a);
    case 'dump': {
      const i = target();
      const c = await connect(i);
      const r = await c.ask({ cmd: 'dump' }, 30);
      c.close();
      if (r.error) die(r.error);
      console.log(has(a, '--json') ? JSON.stringify(r.result, null, 2) : dumpText(r.result as Record<string, unknown>));
      return;
    }
    case 'shot': {
      const file = a.rest[0];
      if (!file) die('shot needs a file');
      const i = target();
      const c = await connect(i);
      const r = await c.ask({ cmd: 'screenshot', path: path.resolve(file) }, 30);
      c.close();
      if (r.error) die(r.error);
      console.log(path.resolve(file));
      return;
    }
    case 'rebuild': {
      const i = target();
      if (!running(i)) die(`${i.id} isn't running (start again)`);
      const binary = build({ noBuild: has(a, '--no-build'), here: has(a, '--build-here'), logName: i.id });
      const c = await connect(i);
      const d = await c.ask({ cmd: 'dump' }, 30);
      const open = (d.result as { selection?: { id?: string } } | undefined)?.selection?.id;
      writeInstance({ ...i, binary });
      fs.writeFileSync(path.join(dirOf(i.id), 'relaunch'), '');
      // Gone now, so the wait below can't read the old app's port.
      fs.rmSync(path.join(dirOf(i.id), 'automation.json'), { force: true });
      await c.ask({ cmd: 'quit' }, 10);
      c.close();
      // The supervisor starts it again once this one's gone.
      const until = Date.now() + 15_000;
      while (alive(i.app, i.appSince) && Date.now() < until) await sleep(100);
      if (alive(i.app, i.appSince)) process.kill(i.app!, 'SIGTERM');
      await ready(i, 'rebuild');
      if (open) {
        const c2 = await connect(readInstance(i.id)!);
        await c2.ask({ cmd: 'open', id: open }, 15);
        c2.close();
      }
      console.log(`${i.id} rebuilt${open ? `, ${open} open` : ''}`);
      return;
    }
    case 'log': {
      const i = target();
      const file = path.join(dirOf(i.id), has(a, '--mock') ? 'mock.log' : 'app.log');
      const n = flag(a, '-n') ?? '40';
      spawnSync('tail', [...(has(a, '-f') ? ['-f'] : []), '-n', n, file], { stdio: 'inherit' });
      return;
    }
    case 'stop': {
      const all = allInstances();
      const which = has(a, '--everyone') ? all : has(a, '--all') ? all.filter((i) => mine(i, who)) : [target()];
      for (const i of which) {
        await stopOne(i);
        console.log(`stopped ${i.id}`);
      }
      if (!which.length) console.log('none of yours to stop');
      return;
    }
    case 'status': {
      reap();
      const all = allInstances();
      if (!all.length) console.log('no instances');
      for (const i of all) {
        console.log(`${i.id}  ${running(i) ? 'running' : 'stopped'}  ${i.mock ? `mock${i.fixtures.length ? ` (${i.fixtures.length} fixtures)` : ''}` : `home ${i.home}`}  ${mine(i, who) ? 'yours' : whose(i)}  started ${new Date(i.created).toLocaleTimeString()}`);
      }
      return;
    }
    default:
      die(`unknown command ${command} (--help lists them)`);
  }
}

if (process.argv[1] && fs.realpathSync(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main(process.argv.slice(2)).catch((e) => die((e as Error).message));
}
