// Pictures of the Inbox's sort menu in both apps, opened with a real click and
// taken off the real screen, for checking the GPUI app's menus against the
// Swift app's. The Swift app's menu is a real macOS menu, a window of the
// system's that a capture of the app's window leaves out, so both are taken
// as the same piece of the screen around the sort button. Takes over the
// pointer and keys, so only on a machine nobody's using (AGENTS.md, "Tests
// that need the real screen"), on Tinker Desk's desktop:
//
//   desktop/scripts/remote.sh tinker-desk --desk -- bash -c '(cd app && swift build --disable-keychain --product Threadbear | tail -1) &&
//     (cd desktop && cargo build -p threadbear-gpui --features automation | tail -1) && node features/tools/menu-shot.ts <out folder> <label>'
//
// For each app it writes <label>-<app>-open.png (the menu as it opens),
// <label>-<app>-hover.png (the pointer on Newest first) and
// <label>-<app>-ticked.png (the menu again once Newest first is chosen), and
// for each pair <label>-overlay-<state>.png (pixel-overlay: Swift red, GPUI
// cyan). Then index.html puts every label's pictures in the folder side by
// side ("before" and "after" runs into one folder). TB_APPEARANCE sets the
// look, dark if not. TB_PLACE="x,y" moves each window there first (screen
// points), clear of anything left up on that screen, and TB_ROW_MENU="<a
// thread's title>" adds <label>-<app>-row.png, that row's right-click menu
// (separators, a submenu's chevron, icons with no ticks).
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { GpuiDriver } from '../support/gpui.ts';
import { SwiftDriver } from '../support/swift.ts';
import { sleep, type Driver } from '../support/driver.ts';
import { swiftTool, windowAt, windowFrame, work } from './real-screen.ts';
import { ensurePerfLock } from './perf-lock.ts';

await ensurePerfLock();

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const [outArg, label = 'shot', ...only] = process.argv.slice(2);
if (!outArg) throw new Error('usage: menu-shot.ts <out folder> <label> [swift|gpui]');
const out = path.resolve(outArg);
fs.mkdirSync(out, { recursive: true });
const clicker = swiftTool('realdrag');

// The pointer moved, a key pressed, and a window found by its number, through
// the window server as a hand's would.
const helper = path.join(work, 'menu-helper');
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
case "window":
  // pid x y width height of a window by its number.
  let list = CGWindowListCopyWindowInfo([.optionIncludingWindow], CGWindowID(Int(a[2])!)) as! [[String: Any]]
  if let w = list.first, let b = w[kCGWindowBounds as String] as? [String: Double] {
    print(w[kCGWindowOwnerPID as String] as! Int32, Int(b["X"]!), Int(b["Y"]!), Int(b["Width"]!), Int(b["Height"]!))
  }
case "menu-item":
  // A menu item's middle on screen, by its title, through Accessibility:
  // the open menu of the app with this pid.
  let app = AXUIElementCreateApplication(pid_t(Int32(a[2])!))
  func children(_ e: AXUIElement) -> [AXUIElement] {
    var v: CFTypeRef?
    AXUIElementCopyAttributeValue(e, kAXChildrenAttribute as CFString, &v)
    return (v as? [AXUIElement]) ?? []
  }
  func string(_ e: AXUIElement, _ name: String) -> String? {
    var v: CFTypeRef?
    AXUIElementCopyAttributeValue(e, name as CFString, &v)
    return v as? String
  }
  func find(_ e: AXUIElement, _ depth: Int) -> AXUIElement? {
    if string(e, kAXRoleAttribute) == kAXMenuItemRole, string(e, kAXTitleAttribute) == a[3] { return e }
    if depth > 12 { return nil }
    for c in children(e) { if let f = find(c, depth + 1) { return f } }
    return nil
  }
  guard let item = find(app, 0) else { exit(1) }
  var p = CGPoint.zero, s = CGSize.zero
  var v: CFTypeRef?
  AXUIElementCopyAttributeValue(item, kAXPositionAttribute as CFString, &v)
  AXValueGetValue(v as! AXValue, .cgPoint, &p)
  AXUIElementCopyAttributeValue(item, kAXSizeAttribute as CFString, &v)
  AXValueGetValue(v as! AXValue, .cgSize, &s)
  print(Int(p.x + s.width / 2), Int(p.y + s.height / 2))
case "place":
  // The app's windows moved to x y on screen, so nothing of anyone else's
  // (a crash report left up on Tinker Desk) sits over the menu's piece.
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
const key = { down: '125', escape: '53' };

/** The menu's piece of the screen: from a little left of and above the button. */
const REGION = { left: 24, top: 14, width: 300, height: 170 };

async function shoot(name: 'swift' | 'gpui', app: Driver): Promise<void> {
  await app.start();
  try {
    for (let i = 0; i < 400; i += 1) {
      const d = await app.dump();
      if (d.connection === 'connected' && d.sidebar.sections.some((s) => s.threads.length)) break;
      await sleep(50);
    }
    await app.do({ cmd: 'appearance', mode: process.env.TB_APPEARANCE ?? 'dark' });
    await app.do({ cmd: 'sort', order: 'needsYou' });
    await app.do({ cmd: 'front' });
    await sleep(800);
    // Where the window is: the Swift app says its number, the GPUI app's is
    // found by its process.
    const id = name === 'swift' ? /ok (\d+)/.exec(await app.do({ cmd: 'window' }))?.[1] : undefined;
    if (name === 'swift' && !id) throw new Error('swift: no window number');
    const pid = id ? Number(run('window', id).split(' ')[0]) : ((app as GpuiDriver).pid ?? 0);
    // TB_PLACE="x,y" moves the window there first (screen points).
    if (process.env.TB_PLACE) {
      run('place', String(pid), ...process.env.TB_PLACE.split(','));
      await sleep(500);
    }
    const frame = id ? run('window', id).split(' ').slice(1).map(Number) : windowFrame(pid);
    const button = async () => {
      const said = await app.do({ cmd: 'label-point', label: `Sort: ${(await app.dump()).sidebar.sort === 'newest' ? 'Newest first' : 'Needs you first'}` });
      const [, x, y] = /ok (-?\d+) (-?\d+)/.exec(said) ?? [];
      if (x === undefined) throw new Error(`${name}: no sort button (${said})`);
      return [frame[0] + Number(x), frame[1] + Number(y)];
    };
    const take = async (state: string) => {
      const [x, y] = await button();
      const file = path.join(out, `${label}-${name}-${state}.png`);
      execFileSync('screencapture', ['-x', `-R${x - REGION.left},${y - REGION.top},${REGION.width},${REGION.height}`, file]);
      console.log(`${name} ${state}: ${file}`);
    };
    const open = async () => {
      const [x, y] = await button();
      // Stops before any real event if another window is over the button.
      // A window just moved or brought forward can take a moment to be there.
      let top = windowAt(x, y);
      for (let i = 0; i < 20 && top?.pid !== pid; i += 1) {
        await sleep(200);
        top = windowAt(x, y);
      }
      if (top?.pid !== pid) throw new Error(`${name}: not in front at the sort button (${x}, ${y}; the window at ${frame.join(" ")}): ${top?.owner ?? "nothing"} is. No events were posted.`);
      execFileSync(clicker, [String(x), String(y), String(x), String(y), '0', '']);
      await sleep(700);
    };
    // The item's middle on screen: the Swift app's through Accessibility,
    // the GPUI app's from its own layout.
    const item = async (title: string) => {
      // The system's menu can take a moment to show up through Accessibility.
      for (let i = 0; name === 'swift' && i < 10; i += 1) {
        try {
          return run('menu-item', String(pid), title).split(' ').map(Number);
        } catch {
          await sleep(300);
        }
      }
      if (name === 'swift') throw new Error(`swift: no menu line ${title} came up`);
      const [, x, y] = /ok (-?\d+) (-?\d+)/.exec(await app.do({ cmd: 'label-point', label: title })) ?? [];
      return [frame[0] + Number(x), frame[1] + Number(y)];
    };

    await open();
    await take('open');
    const [hx, hy] = await item('Newest first');
    run('move', String(hx), String(hy));
    await sleep(500);
    await take('hover');
    // Chosen with a real click, as a hand does, then opened again.
    execFileSync(clicker, [String(hx), String(hy), String(hx), String(hy), '0', '']);
    await sleep(800);
    await open();
    await take('ticked');
    run('key', key.escape);
    await sleep(300);
    // TB_ROW_MENU names a thread: its row's right-click menu too, with its
    // separators, submenus and icons, from a real right-click on the row.
    if (process.env.TB_ROW_MENU) {
      const said = await app.do({ cmd: 'label-point', label: `${process.env.TB_ROW_MENU}\n`, prefix: true });
      const [, x, y] = /ok (-?\d+) (-?\d+)/.exec(said) ?? [];
      if (x === undefined) throw new Error(`${name}: no row ${process.env.TB_ROW_MENU} (${said})`);
      const [rx, ry] = [frame[0] + Number(x), frame[1] + Number(y)];
      if (windowAt(rx, ry)?.pid !== pid) throw new Error(`${name}: not in front at the row. No events were posted.`);
      run('move', String(rx), String(ry));
      await sleep(300);
      run('rclick', String(rx), String(ry));
      await sleep(800);
      const file = path.join(out, `${label}-${name}-row.png`);
      execFileSync('screencapture', ['-x', `-R${rx - 10},${ry - 10},300,340`, file]);
      console.log(`${name} row: ${file}`);
      run('key', key.escape);
      await sleep(300);
    }
  } finally {
    await app.stop();
  }
}

const apps = only.length ? only : ['swift', 'gpui'];
if (apps.includes('swift')) await shoot('swift', new SwiftDriver());
if (apps.includes('gpui')) await shoot('gpui', new GpuiDriver());

// The overlays: pixel-overlay, compiled once.
const overlay = path.join(work, 'pixel-overlay');
execFileSync('swiftc', ['-O', path.join(root, 'features/tools/pixel-overlay.swift'), '-o', overlay]);
for (const state of ['open', 'hover', 'ticked', 'row']) {
  const [s, g] = ['swift', 'gpui'].map((a) => path.join(out, `${label}-${a}-${state}.png`));
  if (fs.existsSync(s) && fs.existsSync(g)) execFileSync(overlay, ['overlay', s, g, path.join(out, `${label}-overlay-${state}.png`)]);
}

// Every label's pictures in the folder, one row per state.
const labels = [...new Set(fs.readdirSync(out).filter((f) => /-(swift|gpui)-open\.png$/.test(f)).map((f) => f.replace(/-(swift|gpui)-open\.png$/, '')))].sort((a, b) => (a === 'before' ? -1 : b === 'before' ? 1 : a.localeCompare(b)));
const cell = (f: string) => (fs.existsSync(path.join(out, f)) ? `<td><img src="${f}"><br>${f}</td>` : '<td></td>');
const rows = labels.flatMap((l) => ['open', 'hover', 'ticked', 'row'].map((s) => `<tr><th>${l}<br>${s}</th>${cell(`${l}-swift-${s}.png`)}${cell(`${l}-gpui-${s}.png`)}${cell(`${l}-overlay-${s}.png`)}</tr>`));
fs.writeFileSync(
  path.join(out, 'index.html'),
  `<!doctype html><meta charset="utf-8"><title>Sort menu: Swift and GPUI</title>
<style>body{font:13px -apple-system,sans-serif;background:#222;color:#ddd}td,th{padding:6px;vertical-align:top}img{image-rendering:pixelated;width:600px}</style>
<p>The Inbox's sort menu, opened with a real click on Tinker Desk. Swift (a real macOS menu), GPUI, and the two laid over each other: Swift in red, GPUI in cyan, grey where they match.</p>
<table><tr><th></th><th>Swift</th><th>GPUI</th><th>Overlay</th></tr>${rows.join('\n')}</table>
`,
);
console.log(`wrote ${path.join(out, 'index.html')}`);
