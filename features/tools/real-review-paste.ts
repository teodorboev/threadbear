// Voice Type's paste into the design review window, through the window server
// as voice-type/native posts it (MacPaste.swift: the words on the clipboard,
// then Command down, V down and up, Command up to the HID tap). Mike ticked an
// option and dictated straight away, and nothing reached the reply box (12 Nov).
// A script's own keystrokes go straight into GPUI, so only real ones show
// where AppKit sends them. Takes over the keyboard and pointer, so only on a
// machine nobody's using (AGENTS.md, "Tests that need the real screen"):
//
//   desktop/scripts/remote.sh tinker-desk --desk -- bash -c \
//     '(cd desktop && cargo build -p threadbear-gpui --features automation) && node features/tools/real-review-paste.ts'
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { GpuiDriver } from '../support/gpui.ts';
import { sleep } from '../support/driver.ts';
import { assertInFront, windowFrame, work } from './real-screen.ts';

const REVIEW = 'review_account_picker';
const keys = path.join(work, 'keys');
fs.writeFileSync(`${keys}.swift`, `import AppKit
import CoreGraphics
let source = CGEventSource(stateID: .hidSystemState)
func key(_ code: CGKeyCode, _ down: Bool, _ flags: CGEventFlags) {
  let e = CGEvent(keyboardEventSource: source, virtualKey: code, keyDown: down)!
  e.flags = flags
  e.post(tap: .cghidEventTap)
}
let board = NSPasteboard.general
// Put back afterwards, as Voice Type does: it's the desk's own clipboard.
let saved = (board.pasteboardItems ?? []).map { item in item.types.compactMap { t in item.data(forType: t).map { (t, $0) } } }
for arg in CommandLine.arguments.dropFirst() {
  if arg == "front" {
    print(NSWorkspace.shared.frontmostApplication?.processIdentifier ?? 0)
  } else if arg == "right-control" {
    let down = CGEvent(keyboardEventSource: source, virtualKey: 62, keyDown: true)!
    down.type = .flagsChanged; down.flags = [.maskControl, CGEventFlags(rawValue: 0x2000)]
    down.post(tap: .cghidEventTap); usleep(400_000)
    let up = CGEvent(keyboardEventSource: source, virtualKey: 62, keyDown: false)!
    up.type = .flagsChanged; up.flags = []; up.post(tap: .cghidEventTap)
  } else if arg.hasPrefix("key:") {
    let code = CGKeyCode(arg.dropFirst(4))!
    key(code, true, []); usleep(100_000); key(code, false, [])
  } else if arg.hasPrefix("words:") {
    board.clearContents(); board.setString(String(arg.dropFirst(6)), forType: .string)
    usleep(50_000)
    key(55, true, .maskCommand); key(9, true, .maskCommand); key(9, false, .maskCommand); key(55, false, [])
  }
  usleep(350_000)
}
usleep(300_000)
board.clearContents()
board.writeObjects(saved.map { kept in let item = NSPasteboardItem(); for (t, d) in kept { item.setData(d, forType: t) }; return item })
`);
execFileSync('swiftc', ['-O', `${keys}.swift`, '-o', keys]);
const post = (...args: string[]) => execFileSync(keys, args).toString();

const app = new GpuiDriver();
const results: string[] = [];
let failed = 0;
function check(name: string, ok: boolean, detail = ''): void {
  results.push(`${ok ? 'PASS' : 'FAIL'}  ${name}${detail ? `  (${detail})` : ''}`);
  if (!ok) failed += 1;
}
type Review = { reply: string; pickedIds: string[]; focused: string; peeking: boolean };
const review = async (cmd: Record<string, unknown> = {}): Promise<Review> => {
  const out = await app.do({ cmd: 'review', id: REVIEW, open: false, ...cmd });
  return JSON.parse(out.slice(out.indexOf('{'))) as Review;
};
async function replyBecomes(wanted: string, ms = 3000): Promise<string> {
  const end = Date.now() + ms;
  for (;;) {
    const { reply } = await review();
    if (reply === wanted || Date.now() > end) return reply;
    await sleep(100);
  }
}
/** Keys go to the app in front, on the review's window. */
async function reviewInFront(): Promise<void> {
  for (let i = 0; i < 20; i += 1) {
    await app.do({ cmd: 'front', review: REVIEW });
    await sleep(300);
    if (Number(post('front').trim()) === app.pid) break;
  }
  if (Number(post('front').trim()) !== app.pid) throw new Error('the app never came to the front, so no keys were posted');
  const [x, y, w, h] = windowFrame(app.pid!);
  assertInFront(app.pid!, [[x + w / 2, y + h / 2]]);
}

await app.start();
try {
  let thread;
  for (let i = 0; i < 300 && !thread; i += 1) {
    thread = (await app.dump()).sidebar.sections.flatMap((s) => s.threads).find((t) => t.title === 'Compare account picker designs');
    await sleep(50);
  }
  if (!thread) throw new Error('no demo review thread');
  await app.do({ cmd: 'open', id: thread.id });
  await sleep(1000);
  await app.do({ cmd: 'review', id: REVIEW });
  await sleep(800);
  // Brought forward and made key, with the keys where the window opened them:
  // on the window, not in the box. A ⌘V that went to the main window instead
  // would land in its composer, not here, so it can't pass by mistake.
  await reviewInFront();

  // A window-only screenshot lets the person running this check look before
  // releasing real keys. With no gate, the tool remains unattended.
  const gate = process.env.TB_REVIEW_PASTE_GATE;
  if (gate) {
    await app.do({ cmd: 'screenshot', review: REVIEW, path: `${gate}.png` });
    fs.writeFileSync(`${gate}.ready`, String(app.pid));
    const deadline = Date.now() + 120_000;
    while (!fs.existsSync(gate)) {
      if (Date.now() > deadline) throw new Error('screen check was not released');
      await sleep(200);
    }
  }

  await reviewInFront();
  post('key:124');
  check('the right arrow still browses on the stage', (await review()).focused === 'B');
  await reviewInFront();
  post('key:123');
  check('the left arrow still browses on the stage', (await review()).focused === 'A');
  await reviewInFront();
  post('key:49');
  check('Space on the stage does not type a space in the reply', (await review()).reply === '');

  // What Mike did: tick B (a press on its tick, which takes no keys), then talk.
  await app.do({ cmd: 'click', tip: 'Pick B', review: REVIEW });
  await reviewInFront();
  post('right-control', 'words:Bigger please');
  const after = await replyBecomes('Bigger please');
  check('Voice Type\'s ⌘V after ticking an option lands in the reply box', after === 'Bigger please', JSON.stringify(after));
  check('the option stays ticked', (await review()).pickedIds.join(',') === 'B');
  if (process.env.TB_REVIEW_PASTE_SHOT) {
    await app.do({ cmd: 'screenshot', review: REVIEW, path: process.env.TB_REVIEW_PASTE_SHOT });
  }
  await reviewInFront();
  post('right-control', 'words:. That one.');
  const repeated = await replyBecomes('Bigger please. That one.');
  check('a second dictated phrase lands once in the now-focused box', repeated === 'Bigger please. That one.', JSON.stringify(repeated));

  // With the keys in the box, the same ⌘V lands too: it isn't this second window
  // that loses it.
  await review({ reply: '', focusReply: true });
  await reviewInFront();
  post('right-control', 'words:In the box');
  const inBox = await replyBecomes('In the box');
  check('Voice Type\'s ⌘V with the keys in the reply box lands there', inBox === 'In the box', JSON.stringify(inBox));
} catch (error) {
  console.error(error instanceof Error ? error.message : error);
  process.exitCode = 1;
} finally {
  console.log(results.join('\n'));
  console.log(failed === 0 && !process.exitCode ? 'PASS' : 'FAIL');
  if (failed) process.exitCode = 1;
  await app.stop();
  process.exit();
}
