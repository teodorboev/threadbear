// Pictures of a sidebar section heading in both apps, with the real pointer
// away from it and then resting on it, for checking it brightens under the
// pointer as the Swift app's `SectionHeading` does (GPUI's stayed grey until
// 12 Nov: style.rs, `group_hover_text`). Moves the real pointer, so only on a
// machine nobody's using (AGENTS.md, "Tests that need the real screen"), on
// Tinker Desk's desktop:
//
//   desktop/scripts/remote.sh tinker-desk --desk -- bash -c '(cd app && swift build --disable-keychain --product Threadbear | tail -1) &&
//     (cd desktop && cargo build -p threadbear-gpui --features automation | tail -1) && node features/tools/heading-hover-shot.ts <out folder> <label> [swift|gpui]'
//
// Writes <label>-<app>-away.png and <label>-<app>-hover.png, a piece of the
// screen around the heading, and index.html with every label's side by side.
// TB_HEADING names the heading (Inbox if not); TB_APPEARANCE sets the look,
// dark if not.
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { GpuiDriver } from '../support/gpui.ts';
import { SwiftDriver } from '../support/swift.ts';
import { sleep, type Driver } from '../support/driver.ts';
import { windowAt, windowFrame, work } from './real-screen.ts';
import { ensurePerfLock } from './perf-lock.ts';

await ensurePerfLock();

const [outArg, label = 'shot', ...only] = process.argv.slice(2);
if (!outArg) throw new Error('usage: heading-hover-shot.ts <out folder> <label> [swift|gpui]');
const out = path.resolve(outArg);
fs.mkdirSync(out, { recursive: true });
const heading = process.env.TB_HEADING ?? 'Inbox';

// The pointer moved through the window server, as a hand's is, and a
// window's pid and frame by its number.
const helper = path.join(work, 'hover-helper');
fs.writeFileSync(
  `${helper}.swift`,
  `import CoreGraphics
import Foundation
let a = CommandLine.arguments
switch a[1] {
case "move":
  let p = CGPoint(x: Double(a[2])!, y: Double(a[3])!)
  CGEvent(mouseEventSource: nil, mouseType: .mouseMoved, mouseCursorPosition: p, mouseButton: .left)?.post(tap: .cghidEventTap)
case "window":
  let list = CGWindowListCopyWindowInfo([.optionIncludingWindow], CGWindowID(Int(a[2])!)) as! [[String: Any]]
  if let w = list.first, let b = w[kCGWindowBounds as String] as? [String: Double] {
    print(w[kCGWindowOwnerPID as String] as! Int32, Int(b["X"]!), Int(b["Y"]!), Int(b["Width"]!), Int(b["Height"]!))
  }
default: exit(2)
}
`,
);
execFileSync('swiftc', ['-O', `${helper}.swift`, '-o', helper]);
const run = (...args: string[]) => execFileSync(helper, args).toString().trim();

async function shoot(name: 'swift' | 'gpui', app: Driver): Promise<void> {
  await app.start();
  try {
    for (let i = 0; i < 400; i += 1) {
      const d = await app.dump();
      if (d.connection === 'connected' && d.sidebar.sections.some((s) => s.threads.length)) break;
      await sleep(50);
    }
    await app.do({ cmd: 'appearance', mode: process.env.TB_APPEARANCE ?? 'dark' });
    await app.do({ cmd: 'front' });
    await sleep(800);
    const id = name === 'swift' ? /ok (\d+)/.exec(await app.do({ cmd: 'window' }))?.[1] : undefined;
    if (name === 'swift' && !id) throw new Error('swift: no window number');
    const pid = id ? Number(run('window', id).split(' ')[0]) : ((app as GpuiDriver).pid ?? 0);
    const frame = id ? run('window', id).split(' ').slice(1).map(Number) : windowFrame(pid);
    // The fold button, by its tip: Hide while open, Show folded.
    let said = '';
    for (const tip of [`Hide ${heading}`, `Show ${heading}`]) {
      // The Swift app's driver throws on a label it can't find.
      said = await app.do({ cmd: 'label-point', label: tip }).catch((e: Error) => String(e.message));
      if (/^ok /.test(said)) break;
    }
    // The Swift app's fold button has no tip anchor it can find, so the
    // Inbox heading is found by its sort button, 80 points left of it.
    if (!/^ok /.test(said) && heading === 'Inbox') {
      const sort = await app.do({ cmd: 'label-point', label: 'Sort: ', prefix: true }).catch((e: Error) => String(e.message));
      const m = /ok (-?\d+) (-?\d+)/.exec(sort);
      if (m) said = `ok ${Number(m[1]) - 80} ${m[2]}`;
    }
    const [, bx, by] = /ok (-?\d+) (-?\d+)/.exec(said) ?? [];
    if (bx === undefined) throw new Error(`${name}: no ${heading} heading (${said})`);
    const [x, y] = [frame[0] + Number(bx), frame[1] + Number(by)];
    // Its left edge: the sidebar's, from where the window starts.
    const left = frame[0] + 4;
    let top = windowAt(x, y);
    for (let i = 0; i < 20 && top?.pid !== pid; i += 1) {
      await sleep(200);
      top = windowAt(x, y);
    }
    if (top?.pid !== pid) throw new Error(`${name}: not in front at the ${heading} heading (${x}, ${y}): ${top?.owner ?? 'nothing'} is. No events were posted.`);
    const take = (state: string) => {
      const file = path.join(out, `${label}-${name}-${state}.png`);
      execFileSync('screencapture', ['-x', `-R${left},${y - 22},${Math.max(240, (x - left) * 2)},40`, file]);
      console.log(`${name} ${state}: ${file}`);
    };
    // Away: well below the sidebar's headings, in the window's middle.
    run('move', String(frame[0] + frame[2] / 2), String(frame[1] + frame[3] - 40));
    await sleep(600);
    take('away');
    // On the heading, past its title (the hairline), so the pointer isn't
    // over the words it's checking. Rests for the hover's fade.
    run('move', String(x), String(y));
    await sleep(700);
    take('hover');
  } finally {
    await app.stop();
  }
}

const apps = only.length ? only : ['swift', 'gpui'];
if (apps.includes('swift')) await shoot('swift', new SwiftDriver());
if (apps.includes('gpui')) await shoot('gpui', new GpuiDriver());

const labels = [...new Set(fs.readdirSync(out).filter((f) => /-(swift|gpui)-away\.png$/.test(f)).map((f) => f.replace(/-(swift|gpui)-away\.png$/, '')))].sort((a, b) => (a === 'before' ? -1 : b === 'before' ? 1 : a.localeCompare(b)));
const cell = (f: string) => (fs.existsSync(path.join(out, f)) ? `<td><img src="${f}"><br>${f}</td>` : '<td></td>');
const rows = labels.flatMap((l) => ['away', 'hover'].map((s) => `<tr><th>${l}<br>${s}</th>${cell(`${l}-swift-${s}.png`)}${cell(`${l}-gpui-${s}.png`)}</tr>`));
fs.writeFileSync(
  path.join(out, 'index.html'),
  `<!doctype html><meta charset="utf-8"><title>Section heading hover: Swift and GPUI</title>
<style>body{font:13px -apple-system,sans-serif;background:#222;color:#ddd}td,th{padding:6px;vertical-align:top}img{width:480px}</style>
<p>The ${heading} heading with the real pointer away and resting on it, on Tinker Desk.</p>
<table><tr><th></th><th>Swift</th><th>GPUI</th></tr>${rows.join('\n')}</table>
`,
);
console.log(`wrote ${path.join(out, 'index.html')}`);
