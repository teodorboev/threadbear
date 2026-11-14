// Real clicks on the GPUI window while another app is in front (Finder,
// activated with no window over ours), saying whether each one pressed what it
// landed on or only brought the app forward. The Swift app makes every click
// count (FirstClick.swift: AppKit asks the view under the pointer
// `acceptsFirstMouse`, and swallows the click unless it says yes), and Mike
// expects the same here (`foundation.firstclick`). A script's own clicks go
// straight into GPUI, so only real ones through the window server show it.
// Takes over the pointer, so only on a machine nobody's using (AGENTS.md,
// "Tests that need the real screen"), holding the perf lock, with the web
// runtime's archive:
//
//   desktop/scripts/remote.sh tinker-desk -- bash -c 'cd desktop && cargo build -p threadbear-gpui --features automation'
//   desktop/scripts/remote.sh tinker-desk --desk -- lockf ~/.cache/threadbear-perf.lock \
//     env TB_WEB_RUNTIME_ARCHIVE=<the pinned archive> node features/tools/first-click.ts
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { GpuiDriver } from '../support/gpui.ts';
import { sleep, type Dump } from '../support/driver.ts';
import { startSites, stopSites } from '../support/web-pages.ts';
import { assertInFront, recording, windowFrame, work } from './real-screen.ts';

const archive = process.env.TB_WEB_RUNTIME_ARCHIVE;
if (!archive) throw new Error('TB_WEB_RUNTIME_ARCHIVE is the pinned web runtime archive');
if (recording()) throw new Error('something is recording this screen; no events posted');

// The front app's pid, and one real click (moved there first, as a hand does).
const tool = path.join(work, 'first-click');
fs.writeFileSync(`${tool}.swift`, `import AppKit
import CoreGraphics
let a = CommandLine.arguments
if a[1] == "front" {
  print(NSWorkspace.shared.frontmostApplication?.processIdentifier ?? 0)
  exit(0)
}
let at = CGPoint(x: Double(a[2])!, y: Double(a[3])!)
let source = CGEventSource(stateID: .hidSystemState)
func post(_ type: CGEventType) { CGEvent(mouseEventSource: source, mouseType: type, mouseCursorPosition: at, mouseButton: .left)!.post(tap: .cghidEventTap) }
post(.mouseMoved); usleep(250_000)
post(.leftMouseDown); usleep(60_000)
post(.leftMouseUp); usleep(100_000)
`);
execFileSync('swiftc', ['-O', `${tool}.swift`, '-o', tool]);
const front = () => Number(execFileSync(tool, ['front']).toString().trim());
const click = (x: number, y: number) => execFileSync(tool, ['click', String(Math.round(x)), String(Math.round(y))]);

const runtime = fs.mkdtempSync(path.join(fs.realpathSync(os.tmpdir()), 'tb-first-click-runtime-'));
process.env.THREADBEAR_WEB_RUNTIME_DIR = runtime;
const sites = await startSites();
const app = new GpuiDriver();
const results: string[] = [];
let failed = 0;
function check(name: string, ok: boolean, detail = ''): void {
  results.push(`${ok ? 'PASS' : 'FAIL'}  ${name}${detail ? `  (${detail})` : ''}`);
  if (!ok) failed += 1;
}
type Page = { key: string; title?: string; loading?: boolean; hidden?: boolean; slot?: number[] };
const pages = (d: Dump) => ((d as { webPanels?: { pages: Page[] } }).webPanels?.pages ?? []);
const titled = (d: Dump, title: string) => d.sidebar.sections.flatMap((s) => s.threads).find((t) => t.title === title);
async function until(what: (d: Dump) => boolean, ms = 3000): Promise<boolean> {
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
/** Finder in front, with no window of its own, so ours is still uncovered. */
async function putAnotherAppInFront(): Promise<void> {
  execFileSync('/usr/bin/osascript', ['-e', 'tell application "Finder" to activate']);
  for (let i = 0; i < 20 && front() === app.pid; i += 1) await sleep(150);
  if (front() === app.pid) throw new Error('Finder never came in front');
  await sleep(400);
}
/** A real click at a point in the window, with another app in front; the app's pid in front after. */
async function clickBehind(x: number, y: number): Promise<number> {
  await putAnotherAppInFront();
  const [wx, wy] = windowFrame(app.pid!);
  const at: [number, number] = [wx + x, wy + y];
  assertInFront(app.pid!, [at]);
  click(...at);
  await sleep(500);
  return front();
}
const point = (said: string) => said.split(' ').slice(1, 3).map(Number) as [number, number];

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
  console.log(`window ${ww} x ${wh} at ${wx}, ${wy}`);

  // A thread in the list.
  await open('Add dark mode to the dashboard');
  const row = titled(await app.dump(), 'Morning brief')!;
  const [rx, ry] = point(await app.do({ cmd: 'row-point', id: row.id }));
  let after = await clickBehind(rx, ry);
  check('a thread in the list opens on the first click', await until((d) => d.selection?.title === 'Morning brief'), `open: ${(await app.dump()).selection?.title}`);
  check('  and the app comes in front', after === app.pid);

  // A button: the activity bar's Scheduled.
  const [bx, by] = point(await app.do({ cmd: 'label-point', label: 'Scheduled', prefix: true }));
  after = await clickBehind(bx, by);
  check("the activity bar's Scheduled button shows the schedules on the first click", await until((d) => (d.sidebar as { mode?: string }).mode === 'scheduled'), `mode: ${(((await app.dump()).sidebar) as { mode?: string }).mode}`);
  check('  and the app comes in front', after === app.pid);
  await app.do({ cmd: 'mode', mode: 'inbox' }).catch(() => {});
  await sleep(300);

  // A link in a web panel's page: a page of the same site, which stays in the panel.
  await open('Add dark mode to the dashboard');
  const id = (await app.dump()).selection!.id;
  await app.do({ cmd: 'mock', method: 'urlPanel', params: { threadId: id, url: sites.home, title: 'Preview' } });
  await app.do({ cmd: 'panel', id: 'mock.page' });
  if (!(await until((d) => pages(d).some((p) => p.title === 'Panel home' && p.loading === false && p.hidden === false), 60_000))) throw new Error("the panel's page didn't show");
  const slot = pages(await app.dump()).find((p) => p.title === 'Panel home')!.slot!;
  const rect = JSON.parse(await app.do({ cmd: 'webPanel', do: 'eval', js: "(() => { const r = document.getElementById('next').getBoundingClientRect(); return [r.x + r.width / 2, r.y + r.height / 2]; })()" }).then((s) => s.replace(/^ok\s*/, '')));
  const [lx, ly] = rect.value as [number, number];
  after = await clickBehind(slot[0] + lx, slot[1] + ly);
  check("a link in a web panel's page is followed on the first click", await until((d) => pages(d).some((p) => p.title === 'Second page'), 8000), pages(await app.dump()).map((p) => p.title).join(', '));
  check('  and the app comes in front', after === app.pid);
} catch (error) {
  console.error(error instanceof Error ? error.stack : error);
  process.exitCode = 1;
} finally {
  console.log(results.join('\n'));
  if (failed) process.exitCode = 1;
  await app.stop();
  stopSites();
  fs.rmSync(runtime, { recursive: true, force: true });
  process.exit();
}
