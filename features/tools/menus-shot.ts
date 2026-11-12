// Pictures of the GPUI app's menus, each opened with a real click and a line
// under the real pointer, taken off the real screen: for checking every menu
// has the same look (the kit's menu/mac_look.rs and style.rs's `menu`). Takes
// over the pointer and keys, so only on Tinker Desk's desktop:
//
//   desktop/scripts/remote.sh tinker-desk --desk -- node features/tools/menus-shot.ts <out folder> [light|dark] [menu ...]
//
// Menus: sort (the Inbox's sort button), moon (a row's Hibernate), row (a
// row's right-click menu), plus (the composer's +), permission (its
// permission chip) and panel (the Highlights panel's body, right-clicked).
// Writes <menu>-<look>.png for each. TB_PLACE="x,y" moves the window first.
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { GpuiDriver } from '../support/gpui.ts';
import { sleep } from '../support/driver.ts';
import { swiftTool, windowAt, windowFrame, work } from './real-screen.ts';

const [outArg, look = 'dark', ...wanted] = process.argv.slice(2);
if (!outArg) throw new Error('usage: menus-shot.ts <out folder> [light|dark] [menu ...]');
const out = path.resolve(outArg);
fs.mkdirSync(out, { recursive: true });
const clicker = swiftTool('realdrag');

const helper = path.join(work, 'menus-helper');
fs.writeFileSync(
  `${helper}.swift`,
  `import ApplicationServices
import CoreGraphics
import Foundation
let a = CommandLine.arguments
switch a[1] {
case "move":
  let p = CGPoint(x: Double(a[2])!, y: Double(a[3])!)
  CGEvent(mouseEventSource: nil, mouseType: .mouseMoved, mouseCursorPosition: p, mouseButton: .left)?.post(tap: .cghidEventTap)
case "key":
  let code = CGKeyCode(Int(a[2])!)
  CGEvent(keyboardEventSource: nil, virtualKey: code, keyDown: true)?.post(tap: .cghidEventTap)
  usleep(40_000)
  CGEvent(keyboardEventSource: nil, virtualKey: code, keyDown: false)?.post(tap: .cghidEventTap)
case "rclick":
  let p = CGPoint(x: Double(a[2])!, y: Double(a[3])!)
  CGEvent(mouseEventSource: nil, mouseType: .rightMouseDown, mouseCursorPosition: p, mouseButton: .right)?.post(tap: .cghidEventTap)
  usleep(60_000)
  CGEvent(mouseEventSource: nil, mouseType: .rightMouseUp, mouseCursorPosition: p, mouseButton: .right)?.post(tap: .cghidEventTap)
case "place":
  let app = AXUIElementCreateApplication(pid_t(Int32(a[2])!))
  var v: CFTypeRef?
  AXUIElementCopyAttributeValue(app, kAXWindowsAttribute as CFString, &v)
  var p = CGPoint(x: Double(a[3])!, y: Double(a[4])!)
  for w in (v as? [AXUIElement]) ?? [] {
    AXUIElementSetAttributeValue(w, kAXPositionAttribute as CFString, AXValueCreate(.cgPoint, &p)!)
  }
default: exit(2)
}
`,
);
execFileSync('swiftc', ['-O', `${helper}.swift`, '-o', helper]);
const run = (...args: string[]) => execFileSync(helper, args).toString().trim();

type Menu = {
  /** What's clicked, by its label (a prefix with `prefix`), and with which button. */
  target: (row: string) => { label: string; prefix?: boolean; right?: boolean; hoverFirst?: string; below?: number; fromRight?: [number, number] };
  /** The line put under the pointer once it's open. */
  item: string;
  /** The piece of the screen taken, from the click: left, top, width, height. */
  region: [number, number, number, number];
};

const menus: Record<string, Menu> = {
  sort: { target: () => ({ label: 'Sort: Newest first' }), item: 'Needs you first', region: [-20, -14, 280, 150] },
  moon: { target: (row) => ({ label: 'Hibernate', hoverFirst: `${row}\n` }), item: 'Tomorrow, 9 am', region: [-120, -20, 420, 320] },
  row: { target: (row) => ({ label: `${row}\n`, prefix: true, right: true }), item: 'Rename', region: [-10, -10, 320, 420] },
  plus: { target: () => ({ label: 'Attach images, change the folder or schedule the message' }), item: 'Attach images…', region: [-20, -330, 340, 350] },
  permission: { target: () => ({ label: 'Full access' }), item: 'Ask before edits', region: [-60, -260, 410, 280] },
  panel: { target: () => ({ label: '', right: true, fromRight: [160, 620] }), item: 'Open in New Window', region: [-20, -10, 330, 200] },
};

const app = new GpuiDriver();
await app.start();
try {
  let row = '';
  for (let i = 0; i < 400 && !row; i += 1) {
    const d = await app.dump();
    const inbox = d.sidebar.sections.find((s) => /inbox/i.test(s.name) && s.threads.length);
    if (d.connection === 'connected' && inbox) row = inbox.threads[0].title;
    else await sleep(50);
  }
  if (!row) throw new Error('no inbox thread');
  const id = (await app.dump()).sidebar.sections.flatMap((s) => s.threads).find((t) => t.title === row)?.id;
  await app.do({ cmd: 'appearance', mode: look });
  await app.do({ cmd: 'sort', order: 'newest' });
  await app.do({ cmd: 'open', id });
  await app.do({ cmd: 'front' });
  await sleep(1200);
  const pid = app.pid ?? 0;
  if (process.env.TB_PLACE) {
    run('place', String(pid), ...process.env.TB_PLACE.split(','));
    await sleep(500);
  }
  const frame = windowFrame(pid);
  const at = async (label: string, prefix = false) => {
    const said = await app.do({ cmd: 'label-point', label, prefix });
    const [, x, y] = /ok (-?\d+) (-?\d+)/.exec(said) ?? [];
    if (x === undefined) throw new Error(`nothing labelled ${JSON.stringify(label)} (${said})`);
    return [frame[0] + Number(x), frame[1] + Number(y)];
  };
  const inFront = async (x: number, y: number) => {
    for (let i = 0; i < 20 && windowAt(x, y)?.pid !== pid; i += 1) await sleep(200);
    if (windowAt(x, y)?.pid !== pid) throw new Error(`not in front at (${x}, ${y}): ${windowAt(x, y)?.owner ?? 'nothing'} is. No events were posted.`);
  };
  for (const name of wanted.length ? wanted : Object.keys(menus)) {
    const menu = menus[name];
    const target = menu.target(row);
    try {
      if (target.hoverFirst) {
        const [hx, hy] = await at(target.hoverFirst, true);
        await inFront(hx, hy);
        run('move', String(hx), String(hy));
        await sleep(600);
      }
      // A point in from the window's top right (the Highlights panel's
      // empty body), or the middle of what has the label.
      const [x, y0] = target.fromRight ? [frame[0] + frame[2] - target.fromRight[0], frame[1] + target.fromRight[1]] : await at(target.label, target.prefix);
      // Below the label: the panel's body under its bar, which has the menu.
      const y = y0 + (target.below ?? 0);
      await inFront(x, y);
      run('move', String(x), String(y));
      await sleep(300);
      if (target.right) run('rclick', String(x), String(y));
      else execFileSync(clicker, [String(x), String(y), String(x), String(y), '0', '']);
      await sleep(900);
      // A line not found (the menu's lines differ by state) still gets a
      // picture, the pointer on whichever line is a little below the click.
      const [ix, iy] = await at(menu.item).catch((e: Error) => {
        console.log(`${name}: ${e.message}; taking it anyway`);
        return [x + 60, y + 40];
      });
      run('move', String(ix), String(iy));
      await sleep(700);
      const [l, t, w, h] = menu.region;
      const file = path.join(out, `${name}-${look}.png`);
      execFileSync('screencapture', ['-x', `-R${x + l},${y + t},${w},${h}`, file]);
      console.log(`${name}: ${file}`);
    } catch (e) {
      console.log(`${name}: failed: ${(e as Error).message}`);
    }
    run('key', '53');
    await sleep(400);
  }
} finally {
  await app.stop();
}
process.exit(0);
