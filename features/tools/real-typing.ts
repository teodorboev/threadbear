// Real typing in the GPUI window, through the window server, as Voice Type
// and a hand make it: the key Voice Type is held on, its pasted words and
// picture (⌘V, each 0.35 s apart, as voice-type/native's Paster posts them),
// then letters typed, each checked in the box they should land in. Run after a
// web panel's page has had the keyboard, which is what left letters beeping in
// the new-thread box while pastes still landed (Mike, 12 Nov). A script's own
// keystrokes go straight into GPUI, so only real ones show where AppKit sends
// them. Takes over the keyboard, so only on a machine nobody's using
// (AGENTS.md, "Tests that need the real screen"), with the web runtime's
// archive:
//
//   desktop/scripts/remote.sh tinker-desk --desk -- bash -c \
//     '(cd desktop && cargo build -p threadbear-gpui --features automation) && \
//      TB_WEB_RUNTIME_ARCHIVE=<the pinned archive> node features/tools/real-typing.ts'
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { GpuiDriver } from '../support/gpui.ts';
import { sleep, type Dump } from '../support/driver.ts';
import { startSites, stopSites } from '../support/web-pages.ts';
import { assertInFront, windowFrame, work } from './real-screen.ts';

const archive = process.env.TB_WEB_RUNTIME_ARCHIVE;
if (!archive) throw new Error('TB_WEB_RUNTIME_ARCHIVE is the pinned web runtime archive');
const picture = path.join(path.dirname(fileURLToPath(import.meta.url)), '../support/sample.png');

// The keys, as voice-type/native posts them (MacPaste.swift): a HID-state
// source to the HID tap, ⌘V as Command down, V down and up, Command up.
const keys = path.join(work, 'keys');
fs.writeFileSync(`${keys}.swift`, `import AppKit
import CoreGraphics
let source = CGEventSource(stateID: .hidSystemState)
func key(_ code: CGKeyCode, _ down: Bool, _ flags: CGEventFlags) {
  let e = CGEvent(keyboardEventSource: source, virtualKey: code, keyDown: down)!
  e.flags = flags
  e.post(tap: .cghidEventTap)
}
func command(_ code: CGKeyCode) {
  key(55, true, .maskCommand); key(code, true, .maskCommand); key(code, false, .maskCommand); key(55, false, [])
}
let codes: [Character: CGKeyCode] = ["a": 0, "s": 1, "d": 2, "f": 3, "h": 4, "g": 5, "z": 6, "x": 7, "c": 8, "v": 9, "b": 11, "q": 12, "w": 13, "e": 14, "r": 15, "y": 16, "t": 17, "o": 31, "u": 32, "i": 34, "p": 35, "l": 37, "j": 38, "k": 40, "n": 45, "m": 46, " ": 49]
let board = NSPasteboard.general
// Put back afterwards, as Voice Type does: it's the desk's own clipboard.
let saved = (board.pasteboardItems ?? []).map { item in item.types.compactMap { t in item.data(forType: t).map { (t, $0) } } }
for arg in CommandLine.arguments.dropFirst() {
  if arg == "front" {
    print(NSWorkspace.shared.frontmostApplication?.processIdentifier ?? 0)
  } else if arg == "right-control" {
    // Held to talk, then let go. Voice Type's tap swallows the key itself, so
    // an app sees at most the modifier change.
    let down = CGEvent(keyboardEventSource: source, virtualKey: 62, keyDown: true)!
    down.type = .flagsChanged
    down.flags = [.maskControl, CGEventFlags(rawValue: 0x2000)]
    down.post(tap: .cghidEventTap)
    usleep(400_000)
    let up = CGEvent(keyboardEventSource: source, virtualKey: 62, keyDown: false)!
    up.type = .flagsChanged
    up.flags = []
    up.post(tap: .cghidEventTap)
  } else if arg.hasPrefix("words:") {
    board.clearContents(); board.setString(String(arg.dropFirst(6)), forType: .string)
    usleep(50_000); command(9)
  } else if arg.hasPrefix("picture:") {
    board.clearContents(); board.setData(try! Data(contentsOf: URL(fileURLWithPath: String(arg.dropFirst(8)))), forType: .png)
    usleep(50_000); command(9)
  } else if arg.hasPrefix("type:") {
    for c in arg.dropFirst(5) { if let code = codes[c] { key(code, true, []); usleep(20_000); key(code, false, []); usleep(40_000) } }
  } else if arg == "new-thread" {
    command(45)
  } else if arg.hasPrefix("click:") {
    let xy = arg.dropFirst(6).split(separator: ",").map { Double($0)! }
    let at = CGPoint(x: xy[0], y: xy[1])
    CGEvent(mouseEventSource: source, mouseType: .mouseMoved, mouseCursorPosition: at, mouseButton: .left)!.post(tap: .cghidEventTap)
    usleep(100_000)
    CGEvent(mouseEventSource: source, mouseType: .leftMouseDown, mouseCursorPosition: at, mouseButton: .left)!.post(tap: .cghidEventTap)
    usleep(60_000)
    CGEvent(mouseEventSource: source, mouseType: .leftMouseUp, mouseCursorPosition: at, mouseButton: .left)!.post(tap: .cghidEventTap)
  }
  usleep(350_000)
}
usleep(300_000)
board.clearContents()
board.writeObjects(saved.map { kept in let item = NSPasteboardItem(); for (t, d) in kept { item.setData(d, forType: t) }; return item })
`);
execFileSync('swiftc', ['-O', `${keys}.swift`, '-o', keys]);
const post = (...args: string[]) => execFileSync(keys, args);
/** Keys go to the app in front: this one, or nothing's posted. */
async function inFront(): Promise<void> {
  for (let i = 0; i < 20; i += 1) {
    if (Number(execFileSync(keys, ['front']).toString().trim()) === app.pid) return;
    await app.do({ cmd: 'front' });
    await sleep(250);
  }
  throw new Error('the app never came to the front, so no keys were posted');
}

// The web runtime, installed once into a folder of this run's.
const runtime = fs.mkdtempSync(path.join(fs.realpathSync(os.tmpdir()), 'tb-real-typing-runtime-'));
process.env.THREADBEAR_WEB_RUNTIME_DIR = runtime;
const sites = await startSites();

const app = new GpuiDriver();
const results: string[] = [];
let failed = 0;
function check(name: string, ok: boolean, detail = ''): void {
  results.push(`${ok ? 'PASS' : 'FAIL'}  ${name}${detail ? `  (${detail})` : ''}`);
  if (!ok) failed += 1;
}
type Page = { key: string; title?: string; loading?: boolean; hidden?: boolean; hasKeyboard?: boolean; slot?: number[] };
const pages = (d: Dump) => ((d as { webPanels?: { pages: Page[] } }).webPanels?.pages ?? []);
const keyboard = (d: Dump) => (d as { keyboard?: string }).keyboard;
const composer = (d: Dump) => d.composer as { draft?: string; text?: string; images?: number };
const titled = (d: Dump, title: string) => d.sidebar.sections.flatMap((s) => s.threads).find((t) => t.title === title);
async function until(what: (d: Dump) => boolean, ms = 5000): Promise<boolean> {
  const end = Date.now() + ms;
  for (;;) {
    if (what(await app.dump())) return true;
    if (Date.now() > end) return false;
    await sleep(100);
  }
}
async function open(title: string): Promise<void> {
  const t = titled(await app.dump(), title);
  if (!t) throw new Error(`no thread called ${title}`);
  await app.do({ cmd: 'open', id: t.id });
  await sleep(600);
}
/** The thread's page shown in its panel, and a real click in it. */
async function clickInPage(): Promise<void> {
  await open('Add dark mode to the dashboard');
  const id = (await app.dump()).selection!.id;
  await app.do({ cmd: 'mock', method: 'urlPanel', params: { threadId: id, url: sites.home, title: 'Preview' } });
  await app.do({ cmd: 'panel', id: 'mock.page' });
  if (!(await until((d) => pages(d).some((p) => p.title === 'Panel home' && p.loading === false && p.hidden === false), 60_000))) throw new Error("the panel's page didn't show");
  const slot = pages(await app.dump()).find((p) => p.title === 'Panel home')!.slot!;
  const [wx, wy] = windowFrame(app.pid!);
  // Low in the page, under its links and buttons.
  const at: [number, number] = [wx + slot[0] + slot[2] / 2, wy + slot[1] + slot[3] - 40];
  assertInFront(app.pid!, [at]);
  post(`click:${at[0]},${at[1]}`);
  check('a click in the page gives it the keyboard', await until((d) => keyboard(d) === 'page'), keyboard(await app.dump()));
}
/** Voice Type into the new-thread box after ⌘N, then letters typed. */
async function dictateIntoNewThread(what: string, word: string): Promise<void> {
  await inFront();
  post('new-thread');
  if (!(await until((d) => d.selection == null && composer(d).draft === 'new'))) throw new Error('⌘N opened no new-thread page');
  await sleep(500);
  const before = await app.dump();
  console.log(`${what}: after ⌘N the keyboard is with ${keyboard(before)}`);
  const said = `Dictated ${word}.`;
  post('right-control', `words:${said}`, `picture:${picture}`);
  const pasted = await until((d) => (composer(d).text ?? '').includes(said) && (composer(d).images ?? 0) >= 1);
  check(`${what}: the dictated words and picture land in the new-thread box`, pasted, JSON.stringify(composer(await app.dump())));
  post(`type:${word}`);
  // After the picture's chip: the dictated words have the word in them too.
  const typed = await until((d) => (composer(d).text ?? '').endsWith(`] ${word}`));
  const after = await app.dump();
  check(`${what}: letters typed after it land there too`, typed, `${JSON.stringify(composer(after).text)}, keyboard with ${keyboard(after)}`);
}

await app.start();
try {
  for (let i = 0; i < 300; i += 1) {
    const d = await app.dump();
    if (d.connection === 'connected' && d.sidebar.sections.some((s) => s.threads.length)) break;
    await sleep(50);
  }
  await app.do({ cmd: 'webRuntime', do: 'install', archive, wait: 600 }, 630);
  await app.do({ cmd: 'front' });
  await sleep(800);
  const [wx, wy, ww, wh] = windowFrame(app.pid!);
  console.log(`window ${ww} x ${wh} at ${wx}, ${wy}; keyboard with ${keyboard(await app.dump())}`);
  assertInFront(app.pid!, [[wx + ww / 2, wy + wh / 2], [wx + 100, wy + 100], [wx + ww - 100, wy + wh - 100]]);

  // Nothing but GPUI has had the keyboard.
  await open('Morning brief');
  await dictateIntoNewThread('with no page touched', 'first');

  // A page had it, then the new-thread page put it out of sight.
  await clickInPage();
  await dictateIntoNewThread('after a click in a page', 'second');

  // A page had it, then a click on a thread in the sidebar: the letters are
  // that thread's.
  await clickInPage();
  const row = titled(await app.dump(), 'Morning brief')!;
  const [, rx, ry] = (await app.do({ cmd: 'row-point', id: row.id })).split(' ').map(Number);
  const [fx, fy] = windowFrame(app.pid!);
  assertInFront(app.pid!, [[fx + rx, fy + ry]]);
  post(`click:${fx + rx},${fy + ry}`);
  await until((d) => d.selection?.title === 'Morning brief');
  check('a click on a thread takes the keyboard back from the page', await until((d) => keyboard(d) === 'app'), keyboard(await app.dump()));
} catch (error) {
  console.error(error instanceof Error ? error.message : error);
  process.exitCode = 1;
} finally {
  console.log(results.join('\n'));
  console.log(failed === 0 && !process.exitCode ? 'PASS' : 'FAIL');
  if (failed) process.exitCode = 1;
  await app.stop();
  stopSites();
  fs.rmSync(runtime, { recursive: true, force: true });
  process.exit();
}
