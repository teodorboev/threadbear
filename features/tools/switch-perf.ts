// How long switching threads takes in the GPUI app, and where the time goes:
// the runner's answer, its size, turning it into events and folding them into
// rows (both off the main thread), and the first frame drawn with them. Then the
// same switches again, which the threads kept loaded (pane_cache.rs) should make
// at once with no read, and the app's memory with the most threads kept.
//
//   TB_RUNNER_HOME=<dev runner home> node features/tools/switch-perf.ts <short id> <long id> [more ids...]
//
// Against a dev runner on real-sized data (dev-runner.sh --clone-real, copied to
// the machine it runs on), with a release build on a desktop with a real GPU:
//
//   desktop/scripts/remote.sh tinker-desk --desk -- bash -c 'cd desktop &&
//     cargo build --release -p threadbear-gpui --features automation && cd .. &&
//     TB_GPUI_PROFILE=release TB_RUNNER_HOME=... node features/tools/switch-perf.ts ...'
//
// With no TB_RUNNER_HOME it makes a long made-up thread for the mock runner
// (desktop/scripts/make-long-thread.mjs) and switches between it and a demo one.
// It fails (exit 1) when switching back to a thread reads it again or takes
// longer than a few frames: what the cache is for.
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { GpuiDriver } from '../support/gpui.ts';
import { ensurePerfLock } from './perf-lock.ts';

import { sleep } from '../support/driver.ts';

await ensurePerfLock();

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
let ids = process.argv.slice(2);
if (!process.env.TB_RUNNER_HOME) {
  const fixtures = path.join(fs.mkdtempSync(path.join(fs.realpathSync(os.tmpdir()), 'tb-switch-')), 'fixtures');
  execFileSync(process.execPath, [path.join(root, 'desktop/scripts/make-long-thread.mjs'), fixtures, '1500', '30']);
  process.env.TB_MOCK_FIXTURES = fixtures;
}

type Timing = { requestMs: number; bytes: number; decodeMs: number; foldMs: number; readyMs: number | null; paintedMs: number | null; reads: number };
type PaneTiming = { threadId: string | null; state: string | null; rows: number | null; timing: Timing | null; kept: string[] };

const app = new GpuiDriver();
await app.start();
let failed = false;
const rssMb = () => Number(execFileSync('ps', ['-o', 'rss=', '-p', String(app.pid)]).toString().trim()) / 1024;
const paneTiming = async () => JSON.parse(await app.do({ cmd: 'pane-timing' })) as PaneTiming;

try {
  let listed: string[] = [];
  for (let i = 0; i < 600; i += 1) {
    const d = await app.dump();
    listed = d.sidebar.sections.flatMap((s) => s.threads.map((t) => t.id));
    if (d.connection === 'connected' && listed.length && ids.every((id) => listed.includes(id))) break;
    await sleep(100);
  }
  if (!ids.length) {
    // The mock's own: a short demo thread and the made-up long one, then the rest.
    ids = [listed.find((id) => id !== 'long_thread')!, 'long_thread', ...listed.filter((id) => id !== 'long_thread').slice(1, 23)];
  }
  const [short, long] = ids;

  // Opens a thread and waits for its rows on screen: the wall time from the
  // click (to within the 5 ms this asks at) and the app's own split.
  async function open(id: string, label: string) {
    const before = await paneTiming();
    const readsBefore = before.kept.includes(id) ? null : undefined;
    const started = performance.now();
    await app.do({ cmd: 'open', id });
    let shown = 0;
    let t: PaneTiming;
    for (;;) {
      t = await paneTiming();
      if (t.threadId === id && t.state === 'loaded' && !shown) shown = performance.now() - started;
      // A window macOS isn't drawing (a sleeping screen) never gets its frame.
      if (t.threadId === id && t.state === 'loaded' && (t.timing?.paintedMs != null || performance.now() - started - shown > 3000)) break;
      if (performance.now() - started > 60_000) throw new Error(`${id} didn't open in a minute: ${JSON.stringify(t)}`);
      await sleep(5);
    }
    const tm = t.timing!;
    const kept = readsBefore === null;
    console.log(
      `${label.padEnd(26)} ${String(Math.round(shown)).padStart(5)} ms to rows` +
        (kept
          ? `  (kept: no read, ${t.rows} rows)`
          : `  request ${tm.requestMs.toFixed(0)} ms, ${(tm.bytes / 1024).toFixed(0)} KB, decode ${tm.decodeMs.toFixed(0)} ms, fold ${tm.foldMs.toFixed(0)} ms, ready ${tm.readyMs?.toFixed(0)} ms, first frame ${tm.paintedMs == null ? 'not drawn' : `${tm.paintedMs.toFixed(0)} ms`}, ${t.rows} rows`),
    );
    return { shown, kept, reads: tm.reads };
  }

  console.log(`app at ${rssMb().toFixed(0)} MB once connected`);
  await open(short, 'short, first open');
  await open(long, 'long, first open');
  await sleep(500);
  for (let round = 1; round <= 3; round += 1) {
    for (const [id, name] of [
      [short, 'short'],
      [long, 'long'],
    ] as const) {
      const r = await open(id, `${name}, back (${round})`);
      if (!r.kept || r.reads !== 1 || r.shown > 250) {
        console.log(`  FAIL: switching back to ${name} should show it kept, with no read, at once`);
        failed = true;
      }
    }
  }

  if (ids.length > 2) {
    const base = rssMb();
    for (const id of ids) await open(id, id.slice(0, 8));
    await sleep(1000);
    const full = await paneTiming();
    console.log(`\napp at ${rssMb().toFixed(0)} MB with ${full.kept.length + 1} threads loaded (${(rssMb() - base).toFixed(0)} MB more than with 2)`);
    // One more past the limit: the least recently shown goes.
    const first = ids[0];
    const backTo = await open(first, 'first again');
    console.log(`  ${first.slice(0, 8)} ${backTo.kept ? 'was still kept' : 'had gone and was read again'}`);
  }
} finally {
  await app.stop();
}
process.exit(failed ? 1 : 0);
