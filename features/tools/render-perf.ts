// How smoothly the GPUI app scrolls each of its long lists under a real
// trackpad: the transcript of a long thread, the Inbox's thread list and the
// Projects list, at Mike's size (TB_MOCK_PERF_PROJECTS: 1,186 threads over 80
// projects, as his install had on 11 Nov), each idle and then with the Mac
// kept busy (`--load`, one spinning process per core, as a build does).
// From GPUI's frame profiler (frames shown, late, work per frame) and the main
// thread's own CPU time, which leaves out waiting for a core, so it moves
// less than frame times on a shared check host (though not nothing: the
// thread may land on an efficiency core; compare builds in turns). Real gestures through the window server,
// so only on a machine nobody's using, and a release build with frame-stats:
//
//   desktop/scripts/remote.sh tinker-desk --desk --perf -- bash -c 'cd desktop &&
//     cargo build --release -p threadbear-gpui --features automation,frame-stats &&
//     cd .. && TB_GPUI_PROFILE=release node features/tools/render-perf.ts [--load]
//     [--sample <scenario>] [--only <scenario,...>] [--fixtures <dir> --thread <id>]'
//
// --perf holds the host's perf lock over the build and the run. With
// --sample it saves a `sample` CPU profile of that scenario's flings and
// prints where the main thread's time went (features/tools/sample-split.mjs).
import { execFileSync, spawn, type ChildProcess } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { GpuiDriver } from '../support/gpui.ts';
import { sleep } from '../support/driver.ts';
import { assertInFront, swiftTool, windowFrame, work } from './real-screen.ts';
import { ensurePerfLock } from './perf-lock.ts';

// One measurement or perf build on the host at a time (perf-lock.ts).
await ensurePerfLock();

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const argv = process.argv.slice(2);
const flag = (name: string) => argv.includes(`--${name}`);
const value = (name: string) => {
  const i = argv.indexOf(`--${name}`);
  return i >= 0 ? argv[i + 1] : undefined;
};
const only = value('only')?.split(',');
const sampleOf = value('sample');
let fixtures = value('fixtures');
let threadId = value('thread');
if (!fixtures) {
  fixtures = path.join(work, 'fixtures');
  execFileSync(process.execPath, [path.join(root, 'desktop/scripts/make-long-thread.mjs'), fixtures, '1500', '60']);
  threadId = 'long_thread';
}
process.env.TB_MOCK_FIXTURES = fixtures;
process.env.TB_MOCK_PERF_PROJECTS ??= '1186';
// Projects folded to a row each, saying how many decisions wait in them: the
// decisions experiment, which Mike has on.
process.env.TB_MOCK_DECISIONS ??= '1';
const gesture = swiftTool('post-gesture');
const dragger = swiftTool('realdrag');

type Screen = { hz: number; presented: number; late: number; missedRefreshes: number; workMs: Record<string, number> };
type Stats = { screens: Screen[]; mainCpuMs: number };
type Row = { scenario: string; load: boolean; frames: number; late: number; p50: number; p90: number; p99: number; max: number; cpuMs: number; cpuPerFrame: number; cpuPerSecond?: number };
const results: Row[] = [];

function record(scenario: string, load: boolean, stats: Stats, seconds?: number) {
  // The screen it drew most of the frames on.
  const s = [...stats.screens].sort((a, b) => b.presented - a.presented)[0];
  if (!s) {
    console.log(`${scenario}${load ? ' (loaded)' : ''}: no frames recorded`);
    return;
  }
  const row: Row = {
    scenario,
    load,
    frames: s.presented,
    late: s.late,
    p50: s.workMs.p50,
    p90: s.workMs.p90,
    p99: s.workMs.p99,
    max: s.workMs.max,
    cpuMs: stats.mainCpuMs,
    cpuPerFrame: Math.round((100 * stats.mainCpuMs) / Math.max(1, s.presented)) / 100,
    ...(seconds ? { cpuPerSecond: Math.round(stats.mainCpuMs / seconds) } : {}),
  };
  results.push(row);
  const pct = Math.round((100 * row.late) / Math.max(1, row.frames));
  console.log(
    `${scenario}${load ? ' (loaded)' : ''}: ${row.frames} frames at ${s.hz} Hz, ${row.late} late (${pct}%); work ms p50 ${row.p50} p90 ${row.p90} p99 ${row.p99} max ${row.max}; main thread ${row.cpuMs} ms CPU, ${row.cpuPerFrame} a frame`,
  );
}

// One spinning process per core, at normal priority, as a build keeps a Mac.
function startLoad(): ChildProcess[] {
  return Array.from({ length: os.cpus().length }, () => spawn('/bin/sh', ['-c', 'while :; do :; done'], { stdio: 'ignore' }));
}

const app = new GpuiDriver();
await app.start();
const loads: ChildProcess[] = [];
try {
  for (let i = 0; i < 400; i += 1) {
    const d = await app.dump();
    if (d.connection === 'connected' && d.sidebar.sections.some((s) => s.threads.length > 10)) break;
    await sleep(50);
  }
  threadId ??= (await app.dump()).sidebar.sections.flatMap((s) => s.threads)[0].id;
  await app.do({ cmd: 'open', id: threadId });
  for (let i = 0; i < 400 && (await app.dump()).transcript.state !== 'loaded'; i += 1) await sleep(50);
  await app.do({ cmd: 'load-all' }, 60);
  await sleep(1500);
  console.log(`${(await app.dump()).transcript.rows.length} rows in the thread, load average ${os.loadavg().map((n) => n.toFixed(1)).join(' ')}`);

  // A scripted app is kept behind everything: bring it forward first.
  await app.do({ cmd: 'front' });
  await sleep(800);
  const [x, y, w, h] = windowFrame(app.pid!);
  // The sidebar's column: where the first row that's on screen is.
  let sidebarX = 0;
  for (const t of (await app.dump()).sidebar.sections.flatMap((s) => s.threads)) {
    try {
      // In the window's own points, from its top left.
      sidebarX = x + Math.round(Number(String(await app.do({ cmd: 'row-point', id: t.id })).split(' ')[1]));
      break;
    } catch {
      // Off screen or folded away: the next one.
    }
  }
  if (!sidebarX) throw new Error('no sidebar row on screen');
  // The sidebar's right edge is about as far past its rows' middle as the
  // window's left edge is before it: the transcript is halfway from there.
  const transcriptX = Math.round((2 * sidebarX - x + x + w) / 2);
  const midY = y + Math.round(h / 2);
  console.log(`window ${w} x ${h} at ${x}, ${y}, sidebar at x ${sidebarX}, transcript at x ${transcriptX}`);
  assertInFront(app.pid!, [[x + w - 200, y + h - 50], [transcriptX, midY], [sidebarX, midY]]);
  // In front and key: a click in the composer, which only focuses it.
  execFileSync(dragger, [String(x + w - 200), String(y + h - 50), String(x + w - 200), String(y + h - 50), '0', '']);
  await sleep(800);

  // A fling: fingers move 30 times, 16 ms apart, then lift with momentum.
  const fling = (at: number, dy: number) => execFileSync(gesture, [String(at), String(midY), '0', ...Array(30).fill(`0,${dy}`), 'M']);
  const stats = async (): Promise<Stats> => JSON.parse(await app.do({ cmd: 'frame-stats', reset: true }));

  // A reply streaming in at 30 chunks a second (the mock's `mock.stream`),
  // measured for STREAM_SECONDS with nothing else moving: in the open thread,
  // and in one kept loaded behind it (opened, then left for this one).
  const streamSeconds = 6;
  const other = (await app.dump()).sidebar.sections.flatMap((s) => s.threads).find((t) => t.id !== threadId)!.id;
  const openThread = async (id: string) => {
    await app.do({ cmd: 'open', id });
    for (let i = 0; i < 400 && (await app.dump()).transcript.state !== 'loaded'; i += 1) await sleep(50);
  };
  const stream = async (id: string) => {
    await app.do({ cmd: 'mock', method: 'stream', params: { threadId: id, hz: 30, seconds: streamSeconds + 3 } });
    await sleep(1000);
    await stats();
    await sleep(streamSeconds * 1000);
  };

  type Scenario = { name: string; at: number; setup: () => Promise<void>; run?: () => Promise<void> };
  const scenarios: Scenario[] = [
    { name: 'transcript', at: transcriptX, setup: async () => {} },
    { name: 'inbox', at: sidebarX, setup: async () => void (await app.do({ cmd: 'mode', mode: 'inbox' })) },
    { name: 'projects', at: sidebarX, setup: async () => void (await app.do({ cmd: 'mode', mode: 'projects' })) },
    { name: 'stream', at: 0, setup: async () => void (await app.do({ cmd: 'mode', mode: 'inbox' })), run: () => stream(threadId!) },
    {
      name: 'stream-kept',
      at: 0,
      setup: async () => {
        await openThread(other);
        await openThread(threadId!);
      },
      run: () => stream(other),
    },
  ].filter((s) => (only ? only.includes(s.name) : !s.name.startsWith('stream')));

  const round = async (s: Scenario, load: boolean) => {
    await s.setup();
    await sleep(1200);
    if (s.run) {
      // The profile, when asked for, covers the measured seconds.
      let sampledRun: Promise<unknown> | undefined;
      let runProfile = '';
      if (sampleOf === s.name && !load) {
        runProfile = path.join(fs.realpathSync(process.env.TB_PROFILE_DIR ?? os.tmpdir()), `tb-${s.name}-sample.txt`);
        const sampler = spawn('/usr/bin/sample', [String(app.pid), String(streamSeconds + 2), '-mayDie', '-file', runProfile], { stdio: 'ignore' });
        sampledRun = new Promise((done) => sampler.on('exit', done));
      }
      await s.run();
      record(s.name, load, await stats(), streamSeconds);
      if (sampledRun) {
        await sampledRun;
        console.log(`  profile: ${runProfile}`);
        console.log(execFileSync(process.execPath, [path.join(root, 'features/tools/sample-split.mjs'), runProfile]).toString());
      }
      await sleep(4000);
      return;
    }
    // TB_PERF_SHOTS=<dir>: what each scenario scrolls, to check it's the right thing.
    if (process.env.TB_PERF_SHOTS) await app.shot(path.join(process.env.TB_PERF_SHOTS, `${s.name}.png`));
    // A first pass so every row's been drawn once (the transcript opens at
    // the bottom), then the measured one: up and back down again.
    for (let i = 0; i < 4; i += 1) fling(s.at, 30);
    for (let i = 0; i < 4; i += 1) fling(s.at, -30);
    await sleep(600);
    await stats();
    let sampled: Promise<unknown> | undefined;
    let profile = '';
    if (sampleOf === s.name && !load) {
      profile = path.join(fs.realpathSync(process.env.TB_PROFILE_DIR ?? os.tmpdir()), `tb-${s.name}-sample.txt`);
      const sampler = spawn('/usr/bin/sample', [String(app.pid), '7', '-mayDie', '-file', profile], { stdio: 'ignore' });
      sampled = new Promise((done) => sampler.on('exit', done));
      await sleep(300);
    }
    for (let i = 0; i < 6; i += 1) fling(s.at, 30);
    for (let i = 0; i < 6; i += 1) fling(s.at, -30);
    record(s.name, load, await stats());
    if (sampled) {
      await sampled;
      console.log(`  profile: ${profile}`);
      console.log(execFileSync(process.execPath, [path.join(root, 'features/tools/sample-split.mjs'), profile]).toString());
    }
  };

  for (const s of scenarios) await round(s, false);
  if (flag('load')) {
    loads.push(...startLoad());
    await sleep(1500);
    console.log(`\nWith ${loads.length} spinning processes, load average ${os.loadavg().map((n) => n.toFixed(1)).join(' ')}`);
    for (const s of scenarios) await round(s, true);
  }
  console.log(`\nRESULTS ${JSON.stringify(results)}`);
} catch (error) {
  console.error(error instanceof Error ? error.message : error);
  process.exitCode = 1;
} finally {
  for (const l of loads) l.kill('SIGKILL');
  await app.stop();
  process.exit();
}
