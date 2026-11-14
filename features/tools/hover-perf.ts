// How long the GPUI app takes to show a hover or a press: from the real
// pointer event leaving the window server to the end of painting the frame
// that shows it, down the thread list and the Projects list, with nothing else
// running and then with every core but one busy. Mike, 13 Nov: "notice when I
// scroll up and down, mouse up and down, there's a noticeable delay on the
// highlighting in the projects area". Real events, so only on a machine
// nobody's using, and a release build with frame-stats:
//
//   desktop/scripts/remote.sh tinker-desk --desk --perf -- bash -c 'cd desktop &&
//     cargo build --release -p threadbear-gpui --features automation,frame-stats &&
//     cd .. && TB_GPUI_PROFILE=release node features/tools/hover-perf.ts'
//
// The data is the shape of Mike's install (TB_MOCK_PERF_PROJECTS: 1,400
// threads, nearly all archived, over 80 projects). HOVER_PERF_LOAD picks the
// conditions (idle,busy,nice; idle and busy by default); HOVER_PERF_JSON=<file>
// writes the numbers too. hover-ab.ts runs two builds turn about. Pointer to
// glass needs a build whose display link reports frames' due times, which
// main's doesn't (perf-hover.md); without it those columns are empty.
import { execFileSync, spawn, type ChildProcess } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { GpuiDriver } from '../support/gpui.ts';
import { sleep } from '../support/driver.ts';
import { assertInFront, windowFrame, work } from './real-screen.ts';
import { ensurePerfLock } from './perf-lock.ts';

// Tinker Desk's other perf runs measure there too: one at a time.
await ensurePerfLock();

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
process.env.TB_MOCK_PERF_PROJECTS ??= '1400';
const probe = path.join(work, 'pointer-probe');
execFileSync('swiftc', ['-O', path.join(root, 'features/tools/pointer-probe.swift'), '-o', probe]);

type Hover = { what: string; x: number; y: number; handledNs: number; presentedNs: number | null; dueNs: number | null };
type Press = { dispatchedNs: number; presentedNs: number | null; dueNs: number | null };
type Result = { glassP50: number; glassP90: number; late: number; missed: number; hz: number; what: string; n: number; p50: number; p90: number; max: number; queueP50: number; frames: number; cpuPerEvent: number; cpuPerFrame: number; workMs?: Record<string, number> };

const pct = (xs: number[], q: number) => (xs.length ? xs[Math.min(xs.length - 1, Math.floor(q * xs.length))] : NaN);
const r1 = (v: number) => Math.round(v * 10) / 10;

const app = new GpuiDriver();
const results: Record<string, Result> = {};
let load: ChildProcess[] = [];
function stopLoad() {
  for (const c of load) c.kill('SIGKILL');
  load = [];
}
process.on('exit', stopLoad);

async function round(name: string, mode: 'move' | 'press', x: number, from: number, to: number, step: number, everyMs: number) {
  const [wx, wy] = windowFrame(app.pid!);
  assertInFront(app.pid!, [[x, from], [x, to]]);
  await app.do({ cmd: 'frame-stats', reset: true });
  await app.do({ cmd: 'input-probe', start: true });
  // HOVER_PERF_SAMPLE=<folder>: a CPU profile of the app through each round.
  const sampler = process.env.HOVER_PERF_SAMPLE
    ? spawn('sample', [String(app.pid), '3', '-file', path.join(process.env.HOVER_PERF_SAMPLE, `${name.replace(/[^a-z0-9]+/gi, '-')}.txt`)], { stdio: 'ignore' })
    : undefined;
  const posts = execFileSync(probe, [mode, String(x), String(from), String(to), String(step), String(everyMs)])
    .toString()
    .trim()
    .split('\n')
    .map((l) => l.split(' ').map(Number));
  await sleep(600);
  if (sampler) await new Promise((done) => (sampler.exitCode === null ? sampler.on('exit', done) : done(null)));
  const got = JSON.parse(await app.do({ cmd: 'input-probe' })) as { hovers: Hover[]; presses: Press[]; frames: number; lost: number; mainCpuMs: number; error?: string };
  if (got.error) throw new Error(got.error);
  const frames = JSON.parse(await app.do({ cmd: 'frame-stats' }));
  const kind = mode === 'press' ? 'press' : 'hover';
  const lat: number[] = [];
  const queue: number[] = [];
  // To when the frame is due on the glass, from the display link.
  const glass: number[] = [];
  if (mode === 'press') {
    // Each press posted, and the first press the app dispatched after it.
    for (const [ns] of posts) {
      const p = got.presses.find((p) => p.dispatchedNs >= ns && p.presentedNs);
      if (!p) continue;
      lat.push((p.presentedNs! - ns) / 1e6);
      queue.push((p.dispatchedNs - ns) / 1e6);
      if (p.dueNs) glass.push((p.dueNs - ns) / 1e6);
    }
  } else {
    for (const i of got.hovers.filter((i) => i.presentedNs)) {
      // The post that put the pointer there: the last one at that point
      // before the app handled it.
      const sx = i.x + wx;
      const sy = i.y + wy;
      const post = posts.filter(([ns, px, py]) => ns <= i.handledNs && Math.abs(px - sx) < 1 && Math.abs(py - sy) < 1).at(-1);
      if (!post) continue;
      lat.push((i.presentedNs! - post[0]) / 1e6);
      queue.push((i.handledNs - post[0]) / 1e6);
      if (i.dueNs) glass.push((i.dueNs - post[0]) / 1e6);
    }
  }
  if (got.lost) console.log(`  (the journal lost ${got.lost} entries)`);
  const seen = mode === 'press' ? got.presses.length : got.hovers.length;
  lat.sort((a, b) => a - b);
  glass.sort((a, b) => a - b);
  queue.sort((a, b) => a - b);
  const screen = frames.screens?.[0];
  const res: Result = { glassP50: r1(pct(glass, 0.5)), glassP90: r1(pct(glass, 0.9)), late: screen?.late ?? NaN, missed: screen?.missedRefreshes ?? NaN, hz: screen?.hz ?? NaN, cpuPerFrame: r1((got.mainCpuMs / Math.max(1, got.frames)) * 10) / 10, cpuPerEvent: r1((got.mainCpuMs / Math.max(1, seen)) * 10) / 10, what: name, n: lat.length, p50: r1(pct(lat, 0.5)), p90: r1(pct(lat, 0.9)), max: r1(lat.at(-1) ?? NaN), queueP50: r1(pct(queue, 0.5)), frames: got.frames, workMs: screen?.workMs };
  results[name] = res;
  console.log(`${name}: ${res.n} ${kind}s matched of ${seen}; event to the glass (ms) median ${res.glassP50}, 90% ${res.glassP90}; to frame handed over median ${res.p50}, 90% ${res.p90}, worst ${res.max}; ${res.late} frames late, ${res.missed} refreshes missed at ${res.hz} Hz; waiting to be handled median ${res.queueP50}; main thread CPU ${res.cpuPerEvent} ms per ${kind}, ${res.cpuPerFrame} per frame; ${res.frames} frames, work median ${screen?.workMs?.p50} 90% ${screen?.workMs?.p90} worst ${screen?.workMs?.max}`);
}

await app.start();
try {
  for (let i = 0; i < 600; i += 1) {
    const d = await app.dump();
    if (d.connection === 'connected' && d.sidebar.sections.some((s: { threads: unknown[] }) => s.threads.length)) break;
    await sleep(50);
  }
  await app.do({ cmd: 'resize', w: 1440, h: 900 });
  await sleep(800);
  // A thread open, as he'd have.
  const first = (await app.dump()).sidebar.sections.flatMap((s: { threads: { id: string }[] }) => s.threads)[0];
  await app.do({ cmd: 'open', id: first.id });
  await sleep(1500);
  const [x, y, w, h] = windowFrame(app.pid!);
  console.log(`window ${w} x ${h} at ${x},${y}`);
  // The thread list's rows, past the activity bar: down a column of them at
  // a hand's pace (a point every 8 ms, 3 points each, about 370 points a
  // second), then back up.
  const listX = x + 160;
  const top = y + 110;
  const bottom = y + h - 80;
  const rounds = async (tag: string) => {
    await app.do({ cmd: 'mode', mode: 'inbox' });
    await sleep(500);
    await round(`threads hover|${tag}`, 'move', listX, top, bottom, 3, 8);
    await round(`threads hover back|${tag}`, 'move', listX, bottom, top, 3, 8);
    // The activity bar's buttons: GPUI's own hover style, no handler of the
    // app's, so this round's measure is CPU per frame.
    await round(`rail hover|${tag}`, 'move', x + 22, top - 60, top + 260, 3, 8);
    await app.do({ cmd: 'mode', mode: 'projects' });
    await sleep(800);
    await round(`projects hover|${tag}`, 'move', listX, top, bottom, 3, 8);
    await round(`projects hover back|${tag}`, 'move', listX, bottom, top, 3, 8);
    await app.do({ cmd: 'mode', mode: 'inbox' });
    await sleep(500);
    // Presses on the first rows, each opening its thread.
    await round(`threads press|${tag}`, 'press', listX, top + 20, top + 320, 60, 400);
  };
  // idle: nothing else running. busy: every core but one spinning at normal
  // priority, as builds and agents leave his Mac. nice: every core spinning
  // at nice 10, as the runner's background jobs do.
  const cores = os.cpus().length;
  for (const condition of (process.env.HOVER_PERF_LOAD ?? 'idle,busy').split(',').filter(Boolean)) {
    if (condition === 'busy') for (let i = 0; i < cores - 1; i += 1) load.push(spawn('yes', [], { stdio: 'ignore' }));
    if (condition === 'nice') for (let i = 0; i < cores; i += 1) load.push(spawn('nice', ['-n', '10', 'yes'], { stdio: 'ignore' }));
    await sleep(1000);
    await rounds(condition);
    stopLoad();
  }
  if (process.env.HOVER_PERF_JSON) fs.writeFileSync(process.env.HOVER_PERF_JSON, JSON.stringify(results, null, 2));
} finally {
  stopLoad();
  await app.stop();
}
