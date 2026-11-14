// A picture of the whole window off the real screen, in either app, and the
// colours a few known places come out as there: for checking the GPUI app's
// colours reach the screen as the Swift app's do (colour_space.rs). A capture
// of the app's window from inside the app (`shot` on the GPUI app) shows the
// colours it asked for, not what the screen showed, so this takes it with
// screencapture and converts it to sRGB with the display's profile, as the
// eye would see it, before reading it. Brings the window to the front, so
// only on a machine nobody's using, on Tinker Desk's desktop:
//
//   desktop/scripts/remote.sh tinker-desk --desk -- node features/tools/colour-shot.ts <out.png> swift|gpui [light|dark]
//
// Prints a line of JSON: each place's sRGB colour as hex. TB_GPUI_BIN picks
// another GPUI build (a "before" one), TB_PLACE="x,y" moves the window there.
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { GpuiDriver } from '../support/gpui.ts';
import { SwiftDriver } from '../support/swift.ts';
import { sleep, type Driver } from '../support/driver.ts';
import { windowFrame, work } from './real-screen.ts';
import { ensurePerfLock } from './perf-lock.ts';

await ensurePerfLock();

const [outArg, which, look = 'dark'] = process.argv.slice(2);
if (!outArg || !which) throw new Error('usage: colour-shot.ts <out.png> swift|gpui [light|dark]');
const out = path.resolve(outArg);
fs.mkdirSync(path.dirname(out), { recursive: true });

const helper = path.join(work, 'colour-helper');
fs.writeFileSync(
  `${helper}.swift`,
  `import AppKit
import CoreGraphics
let a = CommandLine.arguments
switch a[1] {
case "window":
  // The process's main window's number.
  let list = CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID) as! [[String: Any]]
  for w in list where (w[kCGWindowOwnerPID as String] as? Int32) == Int32(a[2])! && (w[kCGWindowLayer as String] as? Int) == 0 {
    print(w[kCGWindowNumber as String] as! Int)
    break
  }
case "place":
  let app = AXUIElementCreateApplication(pid_t(Int32(a[2])!))
  var v: CFTypeRef?
  AXUIElementCopyAttributeValue(app, kAXWindowsAttribute as CFString, &v)
  var p = CGPoint(x: Double(a[3])!, y: Double(a[4])!)
  for w in (v as? [AXUIElement]) ?? [] {
    AXUIElementSetAttributeValue(w, kAXPositionAttribute as CFString, AXValueCreate(.cgPoint, &p)!)
  }
case "sample":
  // The most common colour in each rectangle (x0,y0,x1,y1 in pixels), or
  // with "blue" the most common strongly blue one in the whole picture.
  let rep = NSBitmapImageRep(data: try! Data(contentsOf: URL(fileURLWithPath: a[2])))!
  func px(_ x: Int, _ y: Int) -> String {
    let c = rep.colorAt(x: x, y: y)!.usingColorSpace(.sRGB)!
    return String(format: "#%02x%02x%02x", Int((c.redComponent * 255).rounded()), Int((c.greenComponent * 255).rounded()), Int((c.blueComponent * 255).rounded()))
  }
  var out: [String] = []
  for spec in a.dropFirst(3) {
    var counts: [String: Int] = [:]
    if spec == "blue" {
      for y in stride(from: 0, to: rep.pixelsHigh, by: 1) {
        for x in stride(from: 0, to: rep.pixelsWide, by: 1) {
          let c = rep.colorAt(x: x, y: y)!.usingColorSpace(.sRGB)!
          if c.blueComponent > 0.7 && c.blueComponent - c.redComponent > 0.35 { counts[px(x, y), default: 0] += 1 }
        }
      }
    } else {
      let n = spec.split(separator: ",").map { Int($0)! }
      for y in max(0, n[1])..<min(rep.pixelsHigh, n[3]) {
        for x in max(0, n[0])..<min(rep.pixelsWide, n[2]) { counts[px(x, y), default: 0] += 1 }
      }
    }
    out.append(counts.max(by: { $0.value < $1.value })?.key ?? "none")
  }
  print(out.joined(separator: " "))
default: exit(2)
}
`,
);
execFileSync('swiftc', ['-O', `${helper}.swift`, '-o', helper]);
const run = (...args: string[]) => execFileSync(helper, args).toString().trim();

const app: Driver = which === 'swift' ? new SwiftDriver() : new GpuiDriver();
await app.start();
try {
  let title = '';
  let id = '';
  for (let i = 0; i < 400 && !title; i += 1) {
    const d = await app.dump();
    const inbox = d.sidebar.sections.find((s) => /inbox/i.test(s.name) && s.threads.length);
    if (d.connection === 'connected' && inbox) [title, id] = [inbox.threads[0].title, inbox.threads[0].id];
    else await sleep(50);
  }
  if (!title) throw new Error('no inbox thread');
  await app.do({ cmd: 'appearance', mode: look });
  await app.do({ cmd: 'open', id });
  await app.do({ cmd: 'front' });
  await sleep(1500);
  const swiftWindow = which === 'swift' ? /ok (\d+)/.exec(await app.do({ cmd: 'window' }))?.[1] : undefined;
  const pid = which === 'swift' ? 0 : ((app as GpuiDriver).pid ?? 0);
  if (process.env.TB_PLACE && pid) {
    run('place', String(pid), ...process.env.TB_PLACE.split(','));
    await sleep(500);
  }
  const windowId = swiftWindow ?? run('window', String(pid));
  if (which === 'gpui') console.log(`colour space: ${await app.do({ cmd: 'colour-space' })}`);
  const raw = `${out}.raw.png`;
  execFileSync('screencapture', ['-x', '-o', `-l${windowId}`, raw]);
  // To sRGB through the display's profile: what the screen showed, in the
  // numbers the apps' colours are written in.
  execFileSync('sips', ['-m', '/System/Library/ColorSync/Profiles/sRGB Profile.icc', raw, '--out', out], { stdio: 'ignore' });
  fs.rmSync(raw);
  const said = await app.do({ cmd: 'label-point', label: `${title}\n`, prefix: true });
  const [, rx, ry] = (/ok (-?\d+) (-?\d+)/.exec(said) ?? []).map(Number);
  const size = execFileSync('sips', ['-g', 'pixelWidth', '-g', 'pixelHeight', out]).toString();
  const w = Number(/pixelWidth: (\d+)/.exec(size)?.[1]);
  const h = Number(/pixelHeight: (\d+)/.exec(size)?.[1]);
  const frame = which === 'gpui' ? windowFrame(pid) : [0, 0, w, h];
  const scale = w / frame[2];
  const s = (n: number) => Math.round(n * scale);
  const [sidebar, row, transcript, accent] = run(
    'sample',
    out,
    // Down the sidebar's left, past the rows' words.
    `${s(4)},${s(80)},${s(14)},${h - s(80)}`,
    // The open thread's row, beside its title.
    `${s(rx - 70)},${s(ry - 3)},${s(rx + 70)},${s(ry + 3)}`,
    // The transcript's background, top right.
    `${w - s(220)},${s(70)},${w - s(20)},${s(120)}`,
    'blue',
  ).split(' ');
  console.log(JSON.stringify({ app: which, look, picture: out, sidebar, selectedRow: row, transcript, accent }));
} finally {
  await app.stop();
}
