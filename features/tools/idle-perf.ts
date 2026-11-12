// How much the GPUI app draws while nothing's happening but a thread at work
// somewhere else: GPUI's frame profiler with a long thread open, settled, then
// while another thread's row in the sidebar turns its spinner. A frame the
// window didn't need is the main thread (and the battery) spent for nothing.
// A release build with frame-stats, on a desktop, as the window has to be on
// screen for macOS to ask it for frames:
//
//   desktop/scripts/remote.sh tinker-desk --desk -- bash -c 'cd desktop &&
//     cargo build --release -p threadbear-gpui --features automation,frame-stats &&
//     cd .. && TB_GPUI_PROFILE=release node features/tools/idle-perf.ts'
//
// It makes a long made-up thread and 60 others (desktop/scripts/make-long-thread.mjs),
// so the open transcript and the list are as full as real ones.
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { GpuiDriver } from '../support/gpui.ts';
import { sleep } from '../support/driver.ts';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const fixtures = path.join(fs.mkdtempSync(path.join(fs.realpathSync(os.tmpdir()), 'tb-idle-')), 'fixtures');
execFileSync(process.execPath, [path.join(root, 'desktop/scripts/make-long-thread.mjs'), fixtures, '1500', '60']);
process.env.TB_MOCK_FIXTURES = fixtures;

type Screen = { hz: number; drawn: number; presented: number; workMs: Record<string, number> };

async function measure(app: GpuiDriver, what: string, seconds = 5) {
  await app.do({ cmd: 'frame-stats', reset: true });
  await sleep(seconds * 1000);
  const stats = JSON.parse(await app.do({ cmd: 'frame-stats' })) as { screens: Screen[] };
  console.log(`\n${what}, ${seconds} s`);
  if (!stats.screens.length) console.log('  no frames drawn');
  for (const s of stats.screens) {
    console.log(`  ${s.hz} Hz screen: ${s.presented} frames, ${(s.presented / seconds).toFixed(1)} a second`);
    console.log(`    work per frame (ms): median ${s.workMs.p50}, 90% ${s.workMs.p90}, worst ${s.workMs.max}`);
  }
}

const app = new GpuiDriver();
await app.start();
try {
  for (let i = 0; i < 400; i += 1) {
    const d = await app.dump();
    if (d.connection === 'connected' && d.sidebar.sections.some((s) => s.threads.length > 1)) break;
    await sleep(50);
  }
  const threads = (await app.dump()).sidebar.sections.flatMap((s) => s.threads);
  await app.do({ cmd: 'open', id: 'long_thread' });
  for (let i = 0; i < 400 && (await app.dump()).transcript.state !== 'loaded'; i += 1) await sleep(50);
  await app.do({ cmd: 'front' });
  await sleep(4000);
  await measure(app, 'Settled, nothing working');
  // Another thread at work in the background: its row's spinner turns.
  const busy = threads.find((t) => t.id !== 'long_thread') ?? threads[0];
  await app.do({ cmd: 'mock', method: 'background', params: { threadId: busy.id, tasks: [{ kind: 'shell', description: 'npm test' }] } });
  await sleep(1500);
  await measure(app, `"${busy.title}" working in the background, its spinner turning`);
} finally {
  await app.stop();
}
