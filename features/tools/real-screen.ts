// For the tools that use the real screen and pointer, which run only on a
// machine nobody's using (Tinker Desk's desktop, AGENTS.md): where a window is
// on screen, and the Swift helpers in app/Tools that post real events.
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
export const work = fs.mkdtempSync(path.join(fs.realpathSync(os.tmpdir()), 'tb-real-screen-'));
process.on('exit', () => fs.rmSync(work, { recursive: true, force: true }));

/** app/Tools/<name>.swift, compiled once into a temp folder. */
export function swiftTool(name: string): string {
  const out = path.join(work, name);
  if (!fs.existsSync(out)) execFileSync('swiftc', ['-O', path.join(root, 'app/Tools', `${name}.swift`), '-o', out]);
  return out;
}

/** One on-screen window, as CGWindowListCopyWindowInfo lists them, front to back. */
export interface WindowRow {
  id: number;
  pid: number;
  layer: number;
  alpha: number;
  /** x, y, width and height in screen points from the top left, as listed (they can be fractional). */
  frame: number[];
  owner: string;
  /** Notification Centre's whole-screen sentinel, only when it's really its. */
  notificationSentinel: boolean;
}

const lister = path.join(work, 'windows');

/** Every on-screen window, front to back. */
export function screenWindows(): WindowRow[] {
  if (!fs.existsSync(lister)) {
    // Tab-separated, owner last, as an owner's name can have spaces in it.
    fs.writeFileSync(`${lister}.swift`, `import AppKit
import CoreGraphics
let list = CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID) as! [[String: Any]]
let notificationPIDs = Set(NSRunningApplication.runningApplications(withBundleIdentifier: "com.apple.notificationcenterui")
  .filter { $0.bundleURL?.path == "/System/Library/CoreServices/NotificationCenter.app" }.map(\\.processIdentifier))
for w in list {
  guard let b = w[kCGWindowBounds as String] as? [String: Double], let pid = w[kCGWindowOwnerPID as String] as? Int32 else { continue }
  let sentinel = (w[kCGWindowName as String] as? String) == "Notification Center" && notificationPIDs.contains(pid)
  let owner = (w[kCGWindowOwnerName as String] as? String ?? "?").replacingOccurrences(of: "\\t", with: " ").replacingOccurrences(of: "\\n", with: " ")
  print([w[kCGWindowNumber as String] as? Int ?? -1, Int(pid), w[kCGWindowLayer as String] as? Int ?? -1].map(String.init).joined(separator: "\\t"),
        w[kCGWindowAlpha as String] as? Double ?? 1, b["X"]!, b["Y"]!, b["Width"]!, b["Height"]!, sentinel ? 1 : 0, owner, separator: "\\t")
}
`);
    execFileSync('swiftc', ['-O', `${lister}.swift`, '-o', lister]);
  }
  return parseWindows(execFileSync(lister).toString());
}

/** screenWindows' output, for it and for tests. */
export function parseWindows(text: string): WindowRow[] {
  return text.split('\n').filter(Boolean).map((line) => {
    const [id, pid, layer, alpha, x, y, w, h, sentinel, ...owner] = line.split('\t');
    return {
      id: Number(id), pid: Number(pid), layer: Number(layer), alpha: Number(alpha),
      frame: [x, y, w, h].map(Number), owner: owner.join('\t'), notificationSentinel: sentinel === '1',
    };
  });
}

/** Which window of a process to look at, and a made-up screen for tests. */
export interface WindowPick {
  /** That exact CGWindowID, which must be one of the process's ordinary windows on screen. */
  windowId?: number;
  windows?: () => WindowRow[];
}

/** How big a window must be to count as an app's main window rather than a tooltip, menu or popup. */
export const MAIN_MIN = 300;

/**
 * The process's main window: with `windowId`, that one; otherwise its one
 * visible ordinary (layer 0) window at least MAIN_MIN points each way, or its
 * only visible one. Not merely the first listed: a tooltip or popup is a
 * layer 0 window of its own and can be in front, and on 15 Nov a 66 x 20
 * tooltip came first, which a scroll's guard took for the app (thread
 * 879afc1e). Not the biggest either: with two big windows (a rear one, or a
 * see-through overlay that lets clicks through) a guard would check the
 * wrong one, so two of that size throw and ask for `windowId`, as does none.
 */
export function mainWindow(pid: number, pick: WindowPick = {}): WindowRow {
  const rows = (pick.windows ?? screenWindows)();
  const own = rows.filter((w) => w.pid === pid && w.layer === 0 && w.alpha > 0);
  if (pick.windowId !== undefined) {
    const it = own.find((w) => w.id === pick.windowId);
    if (!it) throw new Error(`Window ${pick.windowId} isn't one of ${pid}'s visible ordinary windows on screen. No events were posted.`);
    return it;
  }
  if (!own.length) throw new Error(`${pid} has no window on screen. No events were posted.`);
  if (own.length === 1) return own[0];
  const big = own.filter((w) => w.frame[2] >= MAIN_MIN && w.frame[3] >= MAIN_MIN);
  if (big.length === 1) return big[0];
  const ids = (big.length ? big : own).map((w) => `${w.id} (${w.frame.slice(2).map(Math.trunc).join(' x ')})`).join(', ');
  throw new Error(`Can't tell which of ${pid}'s windows is its main one: ${ids}. Pass its windowId. No events were posted.`);
}

/**
 * The process's main window (mainWindow): x, y, width and height in whole
 * screen points from the top left. Pass `windowId` when you know which it is.
 */
export function windowFrame(pid: number, pick: WindowPick = {}): number[] {
  return mainWindow(pid, pick).frame.map(Math.trunc);
}

/**
 * Which of the process's visible ordinary windows is frontmost at a point,
 * or null: the one a press there lands on, if it's the app's at all, and so
 * the one a grip drag there moves, whichever is its main window.
 */
export function windowUnder(pid: number, x: number, y: number, pick: Pick<WindowPick, 'windows'> = {}): WindowRow | null {
  const rows = (pick.windows ?? screenWindows)();
  return rows.find((w) => w.pid === pid && w.layer === 0 && w.alpha > 0
    && x >= w.frame[0] && x < w.frame[0] + w.frame[2] && y >= w.frame[1] && y < w.frame[1] + w.frame[3]) ?? null;
}

const finder = path.join(work, 'window-at');

/** The app whose window is on top at a screen point: what a real click there reaches. */
export function windowAt(x: number, y: number): { pid: number; owner: string } | null {
  if (!fs.existsSync(finder)) {
    execFileSync('swiftc', ['-O', path.join(root, 'features/tools/window-at.swift'), '-o', finder]);
  }
  const out = execFileSync(finder, [String(x), String(y)]).toString().trim();
  if (!out) return null;
  const [pid, ...owner] = out.split(' ');
  return { pid: Number(pid), owner: owner.join(' ') };
}

/**
 * Stops before any real event if the app's window isn't the one on top at
 * every point it's about to press: on 10 Nov another job's windows covered
 * the test window on Tinker Desk, and the drags went to them instead.
 */
export function assertInFront(pid: number, points: [number, number][]): void {
  for (const [x, y] of points) {
    const top = windowAt(x, y);
    if (top?.pid !== pid) {
      throw new Error(`Not in front at (${x}, ${y}): ${top ? top.owner : 'nothing'} is there. No events were posted.`);
    }
  }
}

const displayer = path.join(work, 'displays');

/** Every active display's bounds: x, y, width and height in screen points from the top left. */
export function displays(): number[][] {
  if (!fs.existsSync(displayer)) {
    fs.writeFileSync(`${displayer}.swift`, `import CoreGraphics
var ids = [CGDirectDisplayID](repeating: 0, count: 32)
var count: UInt32 = 0
if CGGetActiveDisplayList(32, &ids, &count) == .success {
  for id in ids.prefix(Int(count)) {
    let b = CGDisplayBounds(id)
    print(Int(b.minX), Int(b.minY), Int(b.width), Int(b.height))
  }
}
`);
    execFileSync('swiftc', ['-O', `${displayer}.swift`, '-o', displayer]);
  }
  return execFileSync(displayer).toString().trim().split('\n').filter(Boolean).map((l) => l.split(' ').map(Number));
}

// Python's standard-library FFI avoids compiling another Swift helper for
// every guard. All input is PIDs, never command text or executable code.
const macExecutablePaths = `import ctypes, os, sys
lookup = ctypes.CDLL('/usr/lib/libproc.dylib').proc_pidpath
lookup.argtypes = [ctypes.c_int, ctypes.c_void_p, ctypes.c_uint32]
lookup.restype = ctypes.c_int
found = []
for pid in map(int, sys.stdin.read().split()):
    buf = ctypes.create_string_buffer(4096)
    if lookup(pid, buf, len(buf)) > 0:
        found.append(str(pid) + ' ' + os.fsdecode(buf.value))
if not found:
    raise RuntimeError('Could not read any executable paths')
print('\\n'.join(found))
`;

/** Raw process identities for the guard and its live-reader tests. */
export function processSnapshot(): string {
  // Linux exe can be hidden for another user's process; keep comm as a
  // fallback there. On macOS comm follows argv[0], so also read the kernel
  // name (ucomm) to catch an encoder launched with a different argv[0].
  const fields = process.platform === 'linux' ? ['exe', 'comm'] : ['comm'];
  if (process.platform === 'darwin') fields.push('ucomm');
  let snapshot = fields.map(field => execFileSync('/bin/ps', [
    '-Aww', '-o', 'pid=', '-o', `${field}=`,
  ], { encoding: 'utf8' })).join('\n');
  if (process.platform === 'darwin') {
    // ucomm is truncated at 16 characters. Kernel paths also catch a long
    // encoder basename when its argv[0] has been replaced. Inaccessible or
    // exited PIDs keep their ps identities, as Linux's comm fallback does.
    const pids = [...new Set(snapshot.split('\n').map(line => /^\s*(\d+)\s+/.exec(line)?.[1]).filter(Boolean))];
    snapshot += '\n' + execFileSync('/usr/bin/python3', ['-I', '-c', macExecutablePaths], {
      input: pids.join('\n'), encoding: 'utf8',
    });
  }
  return snapshot;
}

/** Process identities without arguments. Any failure to look throws. */
export function executableProcesses(read = processSnapshot): { pid: number; name: string }[] {
  // Read identities without arbitrary command arguments.
  // pgrep -f matches script text carried by shells/caffeinate; without -f,
  // pgrep truncates names and can miss a long encoder name's ffmpeg suffix.
  const found: { pid: number; name: string }[] = [];
  for (const line of read().split('\n')) {
    const row = /^\s*(\d+)\s+(.+)$/.exec(line);
    if (!row) continue;
    const name = path.basename(row[2].trim());
    found.push({ pid: Number(row[1]), name });
  }
  if (!found.length) throw new Error('Could not read process executable names');
  return found;
}

/** Encoder PIDs and names, or empty when none are running. Lookup failures throw. */
export function recording(read = executableProcesses): string {
  const found = new Map<number, string>();
  for (const p of read()) {
    if (/ffmpeg/i.test(p.name) && !found.has(p.pid)) found.set(p.pid, p.name);
  }
  return [...found].map(([pid, name]) => `${pid} ${name}`).join('\n');
}

/**
 * The windows over `pid`'s main window (mainWindow, so `windowId` picks it)
 * at a point, front to back, as `<window number> <owner>`: those a click there
 * would reach first. The process's own other windows (a tooltip, a popup)
 * count too. Null if `pid` has no ordinary window on screen. The same filter
 * as window-at.
 */
export function windowsAbove(pid: number, x: number, y: number, pick: WindowPick = {}): string[] | null {
  const rows = (pick.windows ?? screenWindows)();
  let main: WindowRow;
  try {
    main = mainWindow(pid, { ...pick, windows: () => rows });
  } catch (e) {
    // Gone is null, as before; two windows it can't tell apart still throw.
    if (/Can't tell which/.test(String(e))) throw e;
    return null;
  }
  const above: string[] = [];
  for (const w of rows) {
    if (w === main) return above;
    if (w.alpha <= 0 || w.notificationSentinel) continue;
    const [bx, by, bw, bh] = w.frame;
    if (x >= bx && x < bx + bw && y >= by && y < by + bh) above.push(`${w.id} ${w.owner}`);
  }
  return null;
}

/** The compiled window-at, for the helpers to look again just before they press (realdrag's RD_GUARD). */
export function windowAtTool(): string {
  if (!fs.existsSync(finder)) windowAt(-100000, -100000);
  return finder;
}

export type Point = [number, number];
/** What the guards read from the screen, so tests can hand them a made-up one. */
export interface Screen {
  windowAt(x: number, y: number): { pid: number; owner: string } | null;
  windowFrame(pid: number, pick?: WindowPick): number[];
  windowsAbove(pid: number, x: number, y: number, pick?: WindowPick): string[] | null;
  windowUnder(pid: number, x: number, y: number): WindowRow | null;
  displays(): number[][];
  recording(): string;
}
export const realScreen: Screen = { windowAt, windowFrame, windowsAbove, windowUnder: (pid, x, y) => windowUnder(pid, x, y), displays, recording };

/**
 * How far in from a display's edge a point an event lands on must be: the
 * Dock overlay exception in window-at refuses the same 24 points, as moving the
 * pointer there can reveal an auto-hidden Dock (or a hot corner) after a check.
 */
export const EDGE = 24;
const inside = ([x, y]: Point, [dx, dy, dw, dh]: number[], inset = 0) => x >= dx + inset && x < dx + dw - inset && y >= dy + inset && y < dy + dh - inset;

/**
 * Checks, immediately before a real event, that it can only reach `pid`:
 * nothing is recording, and `pid`'s window is the one on top at every point
 * the event presses, drops or scrolls at, well in from a display's edge.
 * Throws before anything is posted. Call it again before every event: a
 * window or alert can come up over a point between one drag and the next,
 * and a drag can move the app (10 and 13 Nov).
 */
export function guardInput(pid: number, points: Point[], screen: Screen = realScreen): void {
  const busy = screen.recording();
  if (busy) throw new Error(`Something is recording the screen (${busy.split('\n')[0]}). No events were posted.`);
  const shown = screen.displays();
  for (const p of points) {
    if (!shown.some((d) => inside(p, d, EDGE))) throw new Error(`(${p[0]}, ${p[1]}) is off screen or within ${EDGE} points of its edge. No events were posted.`);
    const top = screen.windowAt(p[0], p[1]);
    if (top?.pid !== pid) throw new Error(`Not in front at (${p[0]}, ${p[1]}): ${top ? top.owner : 'nothing'} is there. No events were posted.`);
  }
}

/**
 * The same for a drag meant to move `pid`'s window by its grip: the press
 * must reach the window, but the drop is where the window will be, so it
 * needn't be the window's now (the drag back to where it was starts and ends
 * outside it). AppKit moves the window with the pointer and keeps the mouse
 * for it all the way, so nothing else is sent the drag. What stays checked:
 * nothing recording, the grip on top and in from the edge, the drop in from
 * every edge (no Dock reveal or hot corner), any part of the drop that's over
 * the window now still the window's (so an alert over it stops it), and the
 * window wholly on a display once moved, so the next grip is on screen too.
 */
export function guardWindowMove(pid: number, from: Point, to: Point, screen: Screen = realScreen, pick: Omit<WindowPick, 'windows'> = {}): void {
  guardInput(pid, [from], screen);
  // The window that moves is the app's one under the grip, which needn't be
  // its main one (a smaller window in front of a bigger one, Codex's review).
  const grip = screen.windowUnder(pid, from[0], from[1]);
  if (!grip) throw new Error(`The grip (${from[0]}, ${from[1]}) isn't on the window. No events were posted.`);
  if (pick.windowId !== undefined && grip.id !== pick.windowId) throw new Error(`The grip (${from[0]}, ${from[1]}) is on window ${grip.id}, not ${pick.windowId}. No events were posted.`);
  pick = { windowId: grip.id };
  const [x, y, w, h] = grip.frame.map(Math.trunc);
  const shown = screen.displays();
  if (!shown.some((d) => inside(to, d, EDGE))) throw new Error(`The drop (${to[0]}, ${to[1]}) is off screen or within ${EDGE} points of its edge. No events were posted.`);
  const moved = [x + to[0] - from[0], y + to[1] - from[1], w, h];
  const onOne = shown.some(([dx, dy, dw, dh]) => moved[0] >= dx && moved[1] >= dy && moved[0] + w <= dx + dw && moved[1] + h <= dy + dh);
  if (!onOne) throw new Error(`Moving the window to (${moved[0]}, ${moved[1]}) would take it off its display. No events were posted.`);
  if (inside(to, [x, y, w, h])) {
    guardInput(pid, [to], screen);
    return;
  }
  // Off the window now: nothing may be stacked over the window there (an
  // alert, a panel, another app brought forward) but what's also over the
  // grip, which window-at has just let through there (the Dock's
  // see-through overlay, which covers the whole screen).
  const atDrop = screen.windowsAbove(pid, to[0], to[1], pick);
  const atGrip = screen.windowsAbove(pid, from[0], from[1], pick);
  if (!atDrop || !atGrip) throw new Error(`The window has gone from the screen. No events were posted.`);
  const over = atDrop.filter((wn) => !atGrip.includes(wn));
  if (over.length) throw new Error(`Something is over the window's place at (${to[0]}, ${to[1]}): ${over[0].replace(/^\S+ /, '')}. No events were posted.`);
}

/**
 * The 40 points realdrag moves through on the way, the same as it works them
 * out: one over a window that isn't `pids` would hand that window the drag (a
 * drag-and-drop's enter and update at least). realdrag looks at each again as
 * it goes; this stops a drag that can't work before anything is pressed.
 */
export function guardPath(pids: number[], from: Point, to: Point, screen: Screen = realScreen): void {
  const steps = 40;
  const shown = screen.displays();
  for (let i = 1; i < steps; i += 1) {
    const t = i / steps;
    const p: Point = [from[0] + (to[0] - from[0]) * t, from[1] + (to[1] - from[1]) * t];
    if (!shown.some((d) => inside(p, d, EDGE))) throw new Error(`The drag passes (${p[0]}, ${p[1]}), off screen or within ${EDGE} points of its edge. No events were posted.`);
    const top = screen.windowAt(p[0], p[1]);
    if (!top || !pids.includes(top.pid)) throw new Error(`The drag passes over ${top ? top.owner : 'nothing'} at (${p[0]}, ${p[1]}). No events were posted.`);
  }
}

/** Runs a helper that posts real events; tests hand in their own to see what would have been posted. */
export type Post = (tool: string, args: string[], env: Record<string, string>) => void;
const execPost: Post = (tool, args, env) => {
  execFileSync(tool, args, { env: { ...process.env, ...env } });
};

/**
 * A real mouse drag from `from` to `to` (a click when they're the same),
 * checked just before it's posted: the press on `pid`'s window, the drop on
 * `dropPid`'s (the app's own unless a drop target of ours), and every point
 * between on one of the two. realdrag looks again at the press after its
 * hover and at the drop before letting go (RD_GUARD).
 */
export function realDrag(tool: string, from: Point, to: Point, hold: number, opts: { pid: number; dropPid?: number; env?: Record<string, string>; screen?: Screen; post?: Post }): void {
  const { pid, dropPid = pid, screen = realScreen, post = execPost } = opts;
  guardInput(pid, [from], screen);
  guardInput(dropPid, [to], screen);
  guardPath([pid, dropPid], from, to, screen);
  post(tool, [String(from[0]), String(from[1]), String(to[0]), String(to[1]), String(hold), ''], {
    ...opts.env,
    RD_GUARD: screen === realScreen ? windowAtTool() : 'window-at',
    RD_PRESS_PID: String(pid),
    RD_DROP_PID: String(dropPid),
  });
}

/**
 * A drag by a grip on `pid`'s window, which may move the window: checked by
 * guardWindowMove just before it's posted. realdrag looks again at the grip
 * after its hover, at each step, and at the drop before letting go, by then
 * under the window, which moved there with the pointer.
 */
export function realGripDrag(tool: string, from: Point, to: Point, hold: number, opts: { pid: number; windowId?: number; env?: Record<string, string>; screen?: Screen; post?: Post }): void {
  const { pid, screen = realScreen, post = execPost } = opts;
  guardWindowMove(pid, from, to, screen, opts.windowId === undefined ? {} : { windowId: opts.windowId });
  post(tool, [String(from[0]), String(from[1]), String(to[0]), String(to[1]), String(hold), ''], {
    ...opts.env,
    RD_GUARD: screen === realScreen ? windowAtTool() : 'window-at',
    RD_PRESS_PID: String(pid),
    RD_DROP_PID: String(pid),
  });
}

/** What post-gesture needs to look again under the fingers after its hover. */
export function gestureGuard(pid: number): Record<string, string> {
  return { PG_GUARD: windowAtTool(), PG_PID: String(pid) };
}

const pointerTool = path.join(work, 'pointer');
function pointerHelper(): string {
  if (!fs.existsSync(pointerTool)) {
    // No arguments: where the pointer is. Two: put it there, with no click.
    fs.writeFileSync(`${pointerTool}.swift`, `import CoreGraphics
let a = CommandLine.arguments
if a.count == 3 {
  CGWarpMouseCursorPosition(CGPoint(x: Double(a[1])!, y: Double(a[2])!))
} else {
  let p = CGEvent(source: nil)!.location
  print(Int(p.x.rounded()), Int(p.y.rounded()))
}
`);
    execFileSync('swiftc', ['-O', `${pointerTool}.swift`, '-o', pointerTool]);
  }
  return pointerTool;
}
/** Where the pointer is now, in screen points from the top left, to put it back after a run. */
export function pointer(): Point {
  return execFileSync(pointerHelper()).toString().trim().split(' ').map(Number) as Point;
}
/** Puts the pointer back where it was, without a click. */
export function movePointer([x, y]: Point): void {
  execFileSync(pointerHelper(), [String(x), String(y)]);
}
