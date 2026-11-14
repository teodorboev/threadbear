// Release-app idle work and main-thread responsiveness. On Tinker Desk only:
// remote.sh tinker-desk --desk -- env TB_GPUI_PROFILE=release
//   node features/tools/idle-perf.ts [fixture folder] [thread id]
// Takes the shared performance and check locks automatically.
// CPU is percent of one core; wakeups come from proc_pid_rusage. The command
// round trip measures scheduling responsiveness, not event-to-paint latency.
import { execFile, execFileSync, spawn, type ChildProcess } from 'node:child_process';
import fs from 'node:fs';
import { createHash } from 'node:crypto';
import { promisify } from 'node:util';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { setTimeout as delay } from 'node:timers/promises';
import { GpuiDriver } from '../support/gpui.ts';
import { SwiftDriver } from '../support/swift.ts';
import { ensurePerfLock } from './perf-lock.ts';
import { recording } from './real-screen.ts';

await ensurePerfLock();
const onlyLoading = process.env.TB_IDLE_ONLY === 'loading';
const requireLoaderPhase = process.env.TB_IDLE_REQUIRE_LOADER_PHASE === 'on';
if (requireLoaderPhase && (!onlyLoading || process.env.TB_REDUCE_MOTION === 'on' || process.env.TB_APP === 'swift')) throw new Error('The loader phase gate requires a focused GPUI loading run with Reduce Motion off.');
const mixed = process.env.TB_IDLE_WORKING === 'on';
if (mixed && (!onlyLoading || process.argv[2])) throw new Error('TB_IDLE_WORKING requires TB_IDLE_ONLY=loading and a generated fixture.');
const offscreen = process.env.TB_IDLE_ONLY === 'offscreen';
if (offscreen) {
  if (process.env.TB_APP === 'swift' || process.argv[2] || process.env.TB_IDLE_LOADING === 'on') throw new Error('Offscreen requires GPUI, a generated fixture and no held transcript read.');
  const others = Number(process.env.TB_IDLE_OTHERS ?? 60);
  const archived = Number(process.env.TB_IDLE_ARCHIVED ?? 0);
  if (!Number.isInteger(others) || !Number.isInteger(archived) || archived < 0 || others - archived < 8) throw new Error('Offscreen requires at least eight unarchived other threads.');
}
const seconds = Number(process.env.TB_IDLE_SECONDS ?? 8);
if (!Number.isFinite(seconds) || seconds <= 0) throw new Error('TB_IDLE_SECONDS must be positive.');
if (process.platform !== 'darwin') throw new Error('This measurement uses macOS process counters.');
const abort = new AbortController();
const sleep = (ms: number) => delay(ms, undefined, { signal: abort.signal });

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const work = fs.mkdtempSync(path.join(fs.realpathSync(os.tmpdir()), 'tb-idle-perf-'));
const counters = path.join(work, 'counters');
const windowWatch = path.join(work, 'window-watch');
const ownedActivator = path.join(work, 'activate-owned');
let swiftActivation: Record<string, unknown> | null = null;
let swiftBinarySha256: string | null = null;
let swiftBinaryUuids: string | null = null;
const activateOwned = promisify(execFile);
const fixtures = process.argv[2] ?? path.join(work, 'fixtures');
const id = process.argv[3] ?? 'long_thread';
execFileSync('clang', ['-O2', path.join(root, 'features/tools/process-counters.c'), '-o', counters]);
execFileSync('swiftc', ['-O', path.join(root, 'features/tools/idle-window-watch.swift'), '-o', windowWatch]);
if (!process.argv[2]) execFileSync(process.execPath, [path.join(root, 'desktop/scripts/make-long-thread.mjs'), fixtures, '1500', process.env.TB_IDLE_OTHERS ?? '60']);
if (!process.argv[2] && process.env.TB_IDLE_ARCHIVED) {
  const others = fs.readdirSync(fixtures).filter(id => id.startsWith('other_')).sort((a, b) => Number(a.slice(6)) - Number(b.slice(6)));
  const count = Number(process.env.TB_IDLE_ARCHIVED);
  if (!Number.isInteger(count) || count < 0 || count >= others.length) throw new Error('TB_IDLE_ARCHIVED must leave at least one other thread in the inbox.');
  for (const id of others.slice(others.length - count)) {
    const file = path.join(fixtures, id, 'thread.json');
    const thread = JSON.parse(fs.readFileSync(file, 'utf8'));
    fs.writeFileSync(file, JSON.stringify({ ...thread, archivedAt: thread.updatedAt, live: false, needsYou: false }));
  }
}
process.env.TB_MOCK_FIXTURES = fixtures;
process.env.THREADBEAR_RICH_CACHE_DIR = path.join(work, 'rich');
const loadingLabel = mixed ? 'reading a thread with another thread working' : 'reading a thread, spinner visible';
const focusedLabels: Record<string, string> = {
  idle: 'idle',
  loading: loadingLabel,
  background: 'another thread working',
  folded: 'another thread working, Threads panel folded away',
  offscreen: 'a working thread outside the visible list',
};
const focus = process.env.TB_IDLE_ONLY;
if (focus && !focusedLabels[focus]) throw new Error('TB_IDLE_ONLY is idle, loading, background, folded or offscreen.');
if (process.env.TB_IDLE_STRESS === 'on' && !focus) throw new Error('TB_IDLE_STRESS requires a focused phase.');
const loadingPhase = onlyLoading || process.env.TB_IDLE_LOADING === 'on';
if (loadingPhase) process.env.THREADBEAR_MOCK_EVENTS_DELAY_MS = String((seconds + 15) * 1000);
if ((focus === 'background' || focus === 'folded' || focus === 'offscreen' || mixed) && !process.argv[2]) {
  // Background tasks alone keep a thread idle, so they don't exercise the
  // sidebar's once-a-second busy clock. A quiet running turn does, without
  // streaming deltas or polling the app during the CPU interval.
  const folder = path.join(fixtures, 'other_0');
  const file = path.join(folder, 'thread.json');
  const thread = JSON.parse(fs.readFileSync(file, 'utf8'));
  const now = new Date().toISOString();
  const seq = thread.lastSeq + 1;
  fs.appendFileSync(path.join(folder, 'events.ndjson'), `${JSON.stringify({ threadId: 'other_0', seq, ts: now, turnId: 'quiet_turn', type: 'turn.started' })}\n`);
  fs.writeFileSync(file, JSON.stringify({ ...thread, status: 'running', needsYou: focus === 'offscreen' ? false : thread.needsYou, lastSeq: seq, lastActivityAt: now, updatedAt: now }));
}

type Counters = { cpuNs: number; interruptWakeups: number; idleWakeups: number; contextSwitches: number };
const swift = process.env.TB_APP === 'swift';
const app = swift ? new SwiftDriver() : new GpuiDriver();
let appPid: number;
let awake: ChildProcess | undefined;
const loads: ChildProcess[] = [];
// A parent timeout sends SIGTERM. Let the finally block stop our detached
// app/mock and load workers instead of orphaning them on the test machine.
function stop(code: number) {
  process.exitCode = code;
  abort.abort();
  for (const child of loads) child.kill();
}
for (const [signal, code] of [['SIGINT', 130], ['SIGTERM', 143], ['SIGHUP', 129]] as const) process.on(signal, () => stop(code));
process.stdout.on('error', error => { if ((error as NodeJS.ErrnoException).code === 'EPIPE') stop(143); else throw error; });
const parentPid = process.ppid;
const parentWatch = setInterval(() => { try { process.kill(parentPid, 0); } catch { stop(143); } }, 500);
parentWatch.unref();
function read(): Counters { return JSON.parse(execFileSync(counters, [String(appPid)], { encoding: 'utf8' })); }
function startLoad() {
  abort.signal.throwIfAborted();
  // Bound even SIGKILL/transport loss. Workers check their owner's life and
  // have a deadline longer than any one focused or loaded measurement.
  const deadline = Math.max(60_000, (seconds * 2 + 30) * 1000);
  const worker = `const owner=${process.pid};const until=Date.now()+${deadline};let n=0,next=0;while(Date.now()<until){for(let i=0;i<10000000;i++)n=Math.imul(n+1,1664525)|0;globalThis.load=n;if(Date.now()>=next){try{process.kill(owner,0)}catch{break}next=Date.now()+250}}`;
  for (let i = 0; i < os.cpus().length - 1; i++) loads.push(spawn(process.execPath, ['-e', worker], { stdio: 'ignore' }));
}
function assertMixed(dump: Awaited<ReturnType<typeof app.dump>>) {
  if (!mixed) return;
  const section = dump.sidebar.sections.find(section => section.threads.some(thread => thread.id === 'other_0'));
  const thread = section?.threads.find(thread => thread.id === 'other_0');
  if (dump.threadsShowing === false || !section || section.collapsed !== false || thread?.status !== 'running' || thread.inView !== true || thread.background != null || thread.waiting != null) {
    throw new Error(`The mixed loader run has no visible working arc: ${JSON.stringify({ section: section?.name, collapsed: section?.collapsed, thread })}`);
  }
}
function assertOffscreen(dump: Awaited<ReturnType<typeof app.dump>>) {
  if (!offscreen) return;
  const section = dump.sidebar.sections.find(section => section.threads.some(thread => thread.id === 'other_0'));
  const thread = section?.threads.find(thread => thread.id === 'other_0');
  const inbox = dump.sidebar.sections.find(section => section.name === 'Inbox');
  const selectedIndex = inbox?.threads.findIndex(thread => thread.id === id) ?? -1;
  const buffers = inbox?.threads.slice(-3) ?? [];
  // inView=false alone also includes the 34px fade at the list's foot.
  // A visible selection followed by three laid-out hidden Inbox rows puts
  // the following Running section fully below the real viewport as well.
  const belowInbox = inbox?.collapsed === false && selectedIndex >= 0 && inbox.threads[selectedIndex].inView === true
    && selectedIndex < inbox.threads.length - 3 && buffers.length === 3 && buffers.every(thread => thread.inView === false)
    && dump.sidebar.sections.indexOf(inbox) < dump.sidebar.sections.indexOf(section!);
  if (dump.transcript.state !== 'loaded' || dump.transcript.spinner || dump.threadsShowing !== true || section?.name !== 'Running' || section.collapsed !== false || thread?.status !== 'running' || thread.inView !== false || thread.background != null || thread.waiting != null || !belowInbox) {
    throw new Error(`The working row is not fully below the visible list: ${JSON.stringify({ transcript: dump.transcript.state, spinner: dump.transcript.spinner, section: section?.name, collapsed: section?.collapsed, thread, selectedIndex, buffers })}`);
  }
}
async function watchWindow() {
  const child = spawn(windowWatch, [String(appPid)], { stdio: ['pipe', 'pipe', 'pipe'] });
  let errors = '', output = '';
  child.stdout.on('data', data => { output += data; });
  child.stderr.on('data', data => { errors += data; });
  // A failed watcher can close its pipe before our finally block ends it.
  child.stdin.on('error', () => {});
  const exited = new Promise<number | null>(resolve => child.once('close', resolve));
  let startupTimeout: ReturnType<typeof setTimeout>;
  try {
    await new Promise<void>((resolve, reject) => {
      startupTimeout = setTimeout(() => reject(new Error('Window watch startup timed out.')), 20_000);
      child.once('error', reject);
      child.once('close', () => reject(new Error(`Window watch did not start: ${errors}`)));
      child.stdout.on('data', () => {
        if (!output.includes('\n')) return;
        if (output.split('\n')[0] === 'ready') resolve();
        else reject(new Error('Window watch sent an invalid readiness message.'));
      });
    });
  } catch (error) {
    child.stdin.end();
    child.kill('SIGTERM');
    throw error;
  } finally {
    clearTimeout(startupTimeout!);
  }
  return async () => {
    child.stdin.end();
    const timeout = setTimeout(() => child.kill('SIGTERM'), 5000);
    const code = await exited.finally(() => clearTimeout(timeout));
    if (code !== 0) throw new Error(`Window changed during measurement: ${errors.trim() || code}`);
    return JSON.parse(output.trim().split('\n').at(-1)!);
  };
}
async function measure(label: string) {
  abort.signal.throwIfAborted();
  if (focus && label !== focusedLabels[focus]) return;
  if (mixed) assertMixed(await app.dump());
  if (focus === 'offscreen') assertOffscreen(await app.dump());
  // Nothing polls the app during the interval: even dump forces a draw.
  // The separate watcher only reads window-server/Workspace state.
  const finishWatch = await watchWindow();
  let before: Counters, after: Counters, elapsed: number, frames, windowChecks;
  let windowActiveBefore: boolean | null = null;
  try {
    if (!swift) {
      const initial = JSON.parse(await app.do({ cmd: 'frame-stats', reset: true }));
      if (initial.windowActive === false || (process.env.TB_IDLE_REQUIRE_ACTIVE === 'on' && initial.windowActive !== true)) throw new Error('GPUI window is not active or this binary cannot report its active state.');
      windowActiveBefore = initial.windowActive ?? null;
    }
    before = read();
    const at = performance.now();
    await sleep(seconds * 1000);
    elapsed = (performance.now() - at) / 1000;
    after = read();
    frames = swift ? null : JSON.parse(await app.do({ cmd: 'frame-stats', reset: true }));
    if (!swift && (frames.windowActive === false || (process.env.TB_IDLE_REQUIRE_ACTIVE === 'on' && frames.windowActive !== true))) throw new Error('GPUI window lost its active state during the interval.');
  } finally {
    windowChecks = await finishWatch();
  }
  if (windowChecks.seconds && windowChecks.samples / windowChecks.seconds > 6) throw new Error('Window watcher ran faster than its sampling limit; no result recorded.');
  if (label === loadingLabel) {
    const dump = await app.dump();
    if (!dump.transcript.spinner) throw new Error('The read completed during the loading interval.');
    assertMixed(dump);
    if (requireLoaderPhase) {
      const loader = frames?.loader;
      if (!loader || loader.source !== 'transcript' || !Number.isInteger(loader.phaseChanges) || loader.phaseChanges < seconds * 10 || !Number.isFinite(loader.maxGapMs) || loader.maxGapMs > 500) throw new Error(`Transcript loader phase validation failed: ${JSON.stringify(loader)}`);
    }
    const drawn = frames?.screens.reduce((n: number, s: { drawn: number }) => n + s.drawn, 0) ?? 0;
    if (!swift && process.env.TB_REDUCE_MOTION !== 'on' && drawn < seconds * 10) throw new Error('Too few spinner frames; the display may be asleep or the app covered.');
  }
  if (focus === 'offscreen') assertOffscreen(await app.dump());
  if (!swift && (offscreen || label === 'another thread working') && process.env.TB_REDUCE_MOTION !== 'on') {
    const drawn = frames?.screens.reduce((n: number, s: { drawn: number }) => n + s.drawn, 0) ?? 0;
    if (drawn < Math.max(2, offscreen ? seconds / 2 : seconds)) throw new Error('Too few working frames; the display may be asleep or the app covered.');
  }
  const times: number[] = [];
  // Swift's automation uses JXA and a file, so its round trip would mostly
  // time that transport. Only compare CPU and wakeups across the apps.
  for (let i = 0; !swift && i < 40; i++) {
    const at = performance.now();
    await app.do({ cmd: 'frame-stats', reset: false });
    times.push(performance.now() - at);
    await sleep(40);
  }
  times.sort((a, b) => a - b);
  abort.signal.throwIfAborted();
  if (loads.some(child => child.exitCode !== null || child.signalCode !== null)) throw new Error('A load worker exited early; no loaded result recorded.');
  const drawn = frames?.screens.reduce((n: number, s: { drawn: number }) => n + s.drawn, 0) ?? null;
  console.log(JSON.stringify({ label, seconds: elapsed, cpuPercent: (after.cpuNs - before.cpuNs) / (elapsed * 1e7),
    mainCpuPercent: frames?.mainCpuMs == null ? null : frames.mainCpuMs / (elapsed * 10),
    framesPerSecond: drawn == null ? null : drawn / elapsed,
    mainMsPerFrame: !drawn || frames?.mainCpuMs == null ? null : frames.mainCpuMs / drawn,
    interruptWakeupsPerSecond: (after.interruptWakeups - before.interruptWakeups) / elapsed,
    idleWakeupsPerSecond: (after.idleWakeups - before.idleWakeups) / elapsed,
    contextSwitchesPerSecond: (after.contextSwitches - before.contextSwitches) / elapsed,
    commandMs: swift ? null : { p50: times[19], p95: times[37], max: times[39] }, load: os.loadavg(), frames, windowChecks,
    loaderGateEnforced: requireLoaderPhase && label === loadingLabel,
    windowActiveBefore, windowActiveAfter: frames?.windowActive ?? null, swiftActivation,
    priority: execFileSync('ps', ['-p', String(appPid), '-o', 'pri=,nice='], { encoding: 'utf8' }).trim() }));
  // Capture only after the counters and responsiveness probes are complete.
  if (process.env.TB_IDLE_SHOT) await app.shot(path.resolve(process.env.TB_IDLE_SHOT));
}
try {
  if (swift && loadingPhase) throw new Error('The Swift reference supports idle and working phases; its dump has no loading-spinner field.');
  if (swift && process.env.TB_IDLE_SWIFT_SHA256 && !process.env.TB_IDLE_SWIFT_UUIDS) throw new Error('A sealed Swift source hash also requires TB_IDLE_SWIFT_UUIDS; private bundle signing changes its raw hash.');
  if (recording()) throw new Error('A recording or encode is running; wait before bringing the test app forward.');
  awake = spawn('/usr/bin/caffeinate', ['-u', '-d', '-w', String(process.pid)], { stdio: 'ignore' });
  if (swift) execFileSync('swiftc', ['-O', path.join(root, 'features/tools/idle-activate.swift'), '-o', ownedActivator]);
  await app.start();
  abort.signal.throwIfAborted();
  appPid = app instanceof SwiftDriver ? await app.processId() : app.pid!;
  if (app instanceof SwiftDriver) {
    swiftBinarySha256 = createHash('sha256').update(fs.readFileSync(app.privateExecutablePath())).digest('hex');
    const uuids = execFileSync('/usr/bin/dwarfdump', ['--uuid', app.privateExecutablePath()], { encoding: 'utf8', timeout: 10_000 });
    swiftBinaryUuids = [...uuids.matchAll(/^UUID: ([a-f0-9-]+) \([^)]+\)/gim)].map(row => row[1].toLowerCase()).sort().join(',');
    if (!swiftBinaryUuids || (process.env.TB_IDLE_SWIFT_UUIDS && process.env.TB_IDLE_SWIFT_UUIDS !== swiftBinaryUuids)) throw new Error(`Private Swift build UUID mismatch: expected ${process.env.TB_IDLE_SWIFT_UUIDS}, got ${swiftBinaryUuids}`);
    // The frozen launcher re-signs each private bundle. Record the resulting
    // file hash separately; its build UUID remains the sealed source's UUID.
  }
  console.log(JSON.stringify({ swiftBinarySha256, swiftBinaryUuids, swiftSourceSha256: swift ? process.env.TB_IDLE_SWIFT_SHA256 : null, fixture: path.basename(fixtures), id, app: swift ? 'swift' : 'gpui', pid: appPid, cores: os.cpus().length, workingBesideLoader: mixed, offscreenWorking: focus === 'offscreen' }));
  if (process.env.TB_IDLE_STRESS === 'on') startLoad();
  for (let i = 0; i < 400; i++) {
    const dump = await app.dump();
    if (dump.connection === 'connected' && dump.sidebar.sections.some(s => s.threads.length)) break;
    await sleep(50);
  }
  await app.do({ cmd: 'front' });
  if (app instanceof SwiftDriver) {
    // Frozen front requests can be refused by AppKit. Activate only this
    // existing private process, then let the external watcher prove focus.
    const activation = await activateOwned(ownedActivator, [String(appPid), app.privateExecutablePath()], {
      encoding: 'utf8', signal: abort.signal, timeout: 10_000, killSignal: 'SIGKILL',
    });
    swiftActivation = JSON.parse(activation.stdout);
  }
  await sleep(2000);
  await app.do({ cmd: 'open', id });
  if (loadingPhase) {
    const loading = await app.dump();
    if (!loading.transcript.spinner) throw new Error('The held transcript is not showing its loading spinner.');
    await sleep(500);
    await measure(loadingLabel);
    if (onlyLoading && process.env.TB_IDLE_SAMPLE) {
      execFileSync('/usr/bin/sample', [String(appPid), '3', '-file', process.env.TB_IDLE_SAMPLE], { stdio: 'ignore' });
    }
  }
  if (!onlyLoading) {
    for (let i = 0; i < 400 && (await app.dump()).transcript.state !== 'loaded'; i++) await sleep(50);
    await sleep(Number(process.env.TB_IDLE_SETTLE_MS ?? 5000));
    await measure('idle');
    if (focus === 'offscreen') await measure(focusedLabels.offscreen);
    if (!process.argv[2] && focus !== 'offscreen') {
      const threads = (await app.dump()).sidebar.sections.flatMap(s => s.threads);
      const busy = threads.find(t => t.id === 'other_0') ?? threads.find(t => t.id !== id)!;
      await app.do({ cmd: 'mock', method: 'background', params: { threadId: busy.id, tasks: [{ kind: 'shell', description: 'npm test' }] } });
      await sleep(1500);
      await measure('another thread working');
      await app.do({ cmd: 'menu-key', key: '/', modifiers: ['cmd'] });
      if ((await app.dump()).threadsShowing !== false) throw new Error('The Threads panel did not fold away.');
      await sleep(1000);
      await measure('another thread working, Threads panel folded away');
      await app.do({ cmd: 'menu-key', key: '/', modifiers: ['cmd'] });
      await app.do({ cmd: 'mock', method: 'background', params: { threadId: busy.id, tasks: [] } });
      await app.do({ cmd: 'mock', method: 'background', params: { threadId: id, tasks: [{ kind: 'shell', description: 'npm test' }] } });
      await sleep(1500);
      await measure('open thread working');
      await app.do({ cmd: 'mock', method: 'background', params: { threadId: id, tasks: [] } });
    }
    if (process.env.TB_IDLE_RICH === 'on') {
      const text = '```html\n<div style="padding:20px;background:#424299;color:white">An idle page</div>\n```';
      await app.do({ cmd: 'mock', method: 'stream', params: { threadId: id, text, hz: 60 } });
      let drawn = false;
      for (let i = 0; i < 300; i++) {
        const dump = await app.dump();
        drawn = dump.transcript.rows.some(r => (r as { rich?: { state: string; shows: string }[] }).rich?.some(b => b.state === 'drawn' && b.shows === 'drawn'));
        if (drawn) break;
        await sleep(200);
      }
      if (!drawn) throw new Error('The HTML box did not finish drawing; no idle-rich result recorded.');
      // The row's arrival glow lasts 3.3s; include only the settled picture.
      await sleep(5000);
      await measure('drawn HTML, nothing working');
    }
    if (process.env.TB_IDLE_LOAD !== 'off' && !focus) {
      startLoad();
      await sleep(1500);
      await measure('idle, all cores but one busy');
      await app.do({ cmd: 'mock', method: 'background', params: { threadId: id, tasks: [{ kind: 'shell', description: 'npm test' }] } });
      await sleep(1500);
      await measure('open thread working, all cores but one busy');
    }
    if (process.env.TB_IDLE_SAMPLE) {
      execFileSync('/usr/bin/sample', [String(appPid), '3', '-file', process.env.TB_IDLE_SAMPLE], { stdio: 'ignore' });
    }
  }
} catch (error) {
  if (!abort.signal.aborted) throw error;
} finally {
  for (const child of loads) child.kill();
  await app.stop();
  awake?.kill();
  clearInterval(parentWatch);
  fs.rmSync(work, { recursive: true, force: true });
}
