// The Swift and GPUI apps side by side on the same thing: how long from the
// real pointer moving onto a thread list row to the row's highlight showing
// on the screen, timed from outside the app with ScreenCaptureKit
// (hover-glass.swift), so neither app needs to time itself. The same mock
// data for both (TB_MOCK_PERF_PROJECTS: 1,400 real-sized threads). Real
// events and Screen Recording, so Tinker Desk's desktop only:
//
//   desktop/scripts/remote.sh tinker-desk --desk --perf -- \
//     env TB_GPUI_BIN=<release build> node features/tools/hover-glass.ts [rounds=3]
//
// The Swift app is the debug build dev-app.sh starts (features/support/swift.ts),
// so it's no faster than a release would be. HOVER_GLASS_ONLY=gpui|swift runs one.
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { GpuiDriver } from '../support/gpui.ts';
import { SwiftDriver } from '../support/swift.ts';
import { sleep } from '../support/driver.ts';
import { assertInFront, work } from './real-screen.ts';
import { ensurePerfLock } from './perf-lock.ts';

// Tinker Desk's other perf runs measure there too: one at a time.
await ensurePerfLock();

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
process.env.TB_MOCK_PERF_PROJECTS ??= '1400';
// The GPUI build comes from TB_GPUI_BIN, which gpui.ts reads as it loads.
const rounds = Number(process.argv[2] ?? 3);
const timer = path.join(work, 'hover-glass');
execFileSync('swiftc', ['-O', path.join(root, 'features/tools/hover-glass.swift'), '-o', timer]);

const lister = path.join(work, 'windows');
fs.writeFileSync(`${lister}.swift`, `import CoreGraphics
import Foundation
let list = CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID) as! [[String: Any]]
for w in list where (w[kCGWindowLayer as String] as? Int) == 0 {
  let b = w[kCGWindowBounds as String] as! [String: Double]
  print(w[kCGWindowOwnerPID as String] as! Int32, Int(b["X"]!), Int(b["Y"]!), Int(b["Width"]!), Int(b["Height"]!), w[kCGWindowOwnerName as String] as? String ?? "")
}
`);
execFileSync('swiftc', ['-O', `${lister}.swift`, '-o', lister]);

/** The front-most ordinary window whose app's name says Threadbear (or is the GPUI binary). */
function frontWindow(): { pid: number; x: number; y: number; w: number; h: number; owner: string } {
  for (const line of execFileSync(lister).toString().trim().split('\n')) {
    const [pid, x, y, w, h, ...owner] = line.split(' ');
    const name = owner.join(' ');
    if (/threadbear|latency-|hover-/i.test(name) && Number(w) > 600) return { pid: Number(pid), x: Number(x), y: Number(y), w: Number(w), h: Number(h), owner: name };
  }
  throw new Error('no Threadbear window on screen');
}

const pct = (xs: number[], q: number) => (xs.length ? xs[Math.min(xs.length - 1, Math.floor(q * xs.length))] : NaN);
const out: Record<string, unknown> = {};

for (const which of ['gpui', 'swift'] as const) {
  if (process.env.HOVER_GLASS_ONLY && process.env.HOVER_GLASS_ONLY !== which) continue;
  const app = which === 'gpui' ? new GpuiDriver() : new SwiftDriver();
  await app.start();
  try {
    for (let i = 0; i < 600; i += 1) {
      const d = await app.dump().catch(() => undefined);
      if (d?.sidebar?.sections?.some((s: { threads: unknown[] }) => s.threads.length)) break;
      await sleep(100);
    }
    // The Swift app opens at 1180 x 800 (swift.ts): the GPUI one the same.
    if (which === 'gpui') await app.do({ cmd: 'resize', w: 1180, h: 800 });
    await app.do({ cmd: 'front' });
    await sleep(1500);
    const win = frontWindow();
    console.log(`${which}: ${win.owner} (pid ${win.pid}), window ${win.w} x ${win.h} at ${win.x},${win.y}`);
    // The thread list's rows, past the activity bar: one move onto each of
    // about ten rows, from off the window, a round at a time.
    const x = win.x + 160;
    const top = win.y + 120;
    const bottom = Math.min(win.y + win.h - 120, top + 420);
    assertInFront(win.pid, [[x, top], [x, bottom]]);
    const times: number[] = [];
    let missed = 0;
    for (let r = 0; r < rounds; r += 1) {
      const lines = execFileSync(timer, [String(x), String(top + r * 7), String(bottom), '42', '350']).toString().trim().split('\n');
      for (const l of lines) {
        const ms = Number(l.split(' ')[1]);
        if (ms >= 0) times.push(ms);
        else missed += 1;
      }
    }
    times.sort((a, b) => a - b);
    const res = { n: times.length, unseen: missed, p50: pct(times, 0.5), p90: pct(times, 0.9), min: times[0], max: times.at(-1) };
    out[which] = res;
    console.log(`${which}: pointer to highlight on screen (ms), ${res.n} moves (${missed} with no change seen): median ${res.p50}, 90% ${res.p90}, fastest ${res.min}, slowest ${res.max}`);
  } finally {
    await app.stop();
  }
}
if (process.env.HOVER_GLASS_JSON) fs.writeFileSync(process.env.HOVER_GLASS_JSON, JSON.stringify(out, null, 2));
