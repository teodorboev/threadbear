// Pictures of a video playing in the side pane, in both apps, for checking one
// against the other. The player is the system's own view over the window, which
// the GPUI app's own picture of its window (`screenshot`) can't see, so the GPUI
// window is taken from the screen by its number, as the Swift app's always is.
//
//   node features/tools/media-shot.ts <out folder> [file in the demo thread's folder]
//
// The file is screens/hero-walkthrough.mp4 unless another is named. Needs both
// apps built and a desktop, so on Tinker Desk (compare.ts's header says how).
// TB_APPEARANCE sets the look.
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { GpuiDriver } from '../support/gpui.ts';
import { SwiftDriver } from '../support/swift.ts';
import { sleep, type Driver } from '../support/driver.ts';
import { pane } from '../support/pane.ts';
import { work } from './real-screen.ts';

const [outArg, relative = 'screens/hero-walkthrough.mp4'] = process.argv.slice(2);
if (!outArg) throw new Error('usage: media-shot.ts <out folder> [file]');
const out = path.resolve(outArg);
fs.mkdirSync(out, { recursive: true });

/** The window number of a process's main window, for screencapture -l. */
function windowNumber(pid: number): string {
  const tool = path.join(work, 'window-number');
  if (!fs.existsSync(tool)) {
    fs.writeFileSync(`${tool}.swift`, `import CoreGraphics
import Foundation
let pid = Int32(CommandLine.arguments[1])!
let list = CGWindowListCopyWindowInfo([.optionAll], kCGNullWindowID) as! [[String: Any]]
for w in list where (w[kCGWindowOwnerPID as String] as? Int32) == pid && (w[kCGWindowLayer as String] as? Int) == 0 {
  print(w[kCGWindowNumber as String] as! Int)
  break
}
`);
    execFileSync('swiftc', ['-O', `${tool}.swift`, '-o', tool]);
  }
  return execFileSync(tool, [String(pid)]).toString().trim();
}

async function shoot(name: 'swift' | 'gpui', app: Driver): Promise<void> {
  await app.start();
  try {
    let id = '';
    for (let i = 0; i < 400 && !id; i += 1) {
      const d = await app.dump();
      id = d.sidebar.sections.flatMap((s) => s.threads).find((t) => t.title === 'Rebuild the three landing pages')?.id ?? '';
      if (!id) await sleep(50);
    }
    if (!id) throw new Error(`${name}: no demo landing pages thread`);
    if (process.env.TB_APPEARANCE) await app.do({ cmd: 'appearance', mode: process.env.TB_APPEARANCE });
    await app.do({ cmd: 'open', id });
    let cwd = '';
    for (let i = 0; i < 200 && !cwd; i += 1) {
      cwd = ((await app.dump()).composer.cwd ?? '').replace(/\/$/, '');
      if (!cwd) await sleep(50);
    }
    await app.do({ cmd: 'preview', path: `${cwd}/${relative}` });
    let shows = '';
    for (let i = 0; i < 200; i += 1) {
      const p = pane(await app.dump());
      shows = `${p?.previewKind} ${p?.previewText ?? ''}`;
      if (p?.previewKind === 'video' && p.previewText) break;
      await sleep(50);
    }
    console.log(`${name}: ${shows}, ${await app.do({ cmd: 'preview', play: true })}`);
    // A second or so in, past the first frame.
    await sleep(1500);
    const file = path.join(out, `video-${name}.png`);
    if (name === 'gpui') {
      const pid = (app as GpuiDriver).pid;
      if (!pid) throw new Error('gpui: no app process');
      execFileSync('screencapture', ['-x', '-o', `-l${windowNumber(pid)}`, file]);
    } else {
      await app.shot(file);
    }
    console.log(`${name}: ${file}`);
  } finally {
    await app.stop();
  }
}

await shoot('swift', new SwiftDriver());
await shoot('gpui', new GpuiDriver());
process.exit(0);
