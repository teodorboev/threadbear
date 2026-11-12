// Short clips of the same animation in the Swift app and the GPUI app, side by
// side (Swift on the left), for checking a port's motion against the original:
// its timing and curves, and whether it holds up with the main thread busy.
// Each app is recorded in turn on the real screen, in front, with
// `screencapture -v` of its window, so it needs a desktop nobody's using:
//
//   desktop/scripts/remote.sh tinker-desk --desk -- bash -c '(cd app && swift build --disable-keychain --product Threadbear | tail -1) &&
//     (cd desktop && cargo build --release -p threadbear-gpui --features automation | tail -1) &&
//     TB_GPUI_PROFILE=release node features/tools/motion-clip.ts <out folder> <scene> [--busy <seconds>]'
//
// Scenes: archive, hibernate (archive-flight.feature), and the panel ones
// (panel-motion.feature): terminal, terminal-close, highlights-close. `--busy` blocks each app's
// main thread for that long straight after the click (the automation's
// `stall`), as opening a heavy next thread would. Writes <scene>.mp4 at full
// speed and <scene>-slow.mp4 at a quarter of it, both apps in dark mode.
import { execFileSync, spawn } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { GpuiDriver } from '../support/gpui.ts';
import { SwiftDriver } from '../support/swift.ts';
import { sleep, type Driver, type Dump } from '../support/driver.ts';
import { windowFrame } from './real-screen.ts';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');

const [outArg, scene = 'archive', ...rest] = process.argv.slice(2);
if (!outArg) throw new Error('usage: motion-clip.ts <out folder> <scene> [--busy <seconds>]');
const out = path.resolve(outArg);
fs.mkdirSync(out, { recursive: true });
const busyAt = rest.indexOf('--busy');
const busy = busyAt >= 0 ? Number(rest[busyAt + 1]) : 0;
const ffmpeg = fs.existsSync('/opt/homebrew/bin/ffmpeg') ? '/opt/homebrew/bin/ffmpeg' : 'ffmpeg';

/** The app's process: the GPUI driver knows it, and dev-app.sh says the Swift one's. */
function pidOf(app: Driver): number {
  if (app instanceof GpuiDriver) return app.pid!;
  const id = (app as unknown as { id: string }).id;
  const status = execFileSync(path.join(root, 'app/Tools/dev-app.sh'), ['status']).toString();
  const line = status.split('\n').find((l) => l.includes(` ${id} `));
  const pid = line?.match(/pid (\d+)/)?.[1];
  if (!pid) throw new Error(`no pid for ${id} in:\n${status}`);
  return Number(pid);
}

const titled = (d: Dump, title: string) => d.sidebar.sections.flatMap((s) => s.threads).find((t) => t.title === title);

async function until<T>(app: Driver, what: string, test: (d: Dump) => T | undefined | false, ms = 15_000): Promise<T> {
  const end = Date.now() + ms;
  for (;;) {
    const d = await app.dump();
    const v = test(d);
    if (v) return v;
    if (Date.now() > end) throw new Error(`${app.name}: no ${what}`);
    await sleep(150);
  }
}

/** What each scene does before recording, and the click it records. */
const scenes: Record<string, { setup: (app: Driver) => Promise<void>; act: (app: Driver) => Promise<void> }> = {
  archive: {
    setup: async (app) => {
      const t = await until(app, 'Morning brief', (d) => titled(d, 'Morning brief'));
      await app.do({ cmd: 'open', id: t.id });
      await until(app, 'its transcript', (d) => d.transcript.threadId === t.id && d.transcript.state === 'loaded');
    },
    act: async (app) => {
      await app.do({ cmd: 'archive' });
    },
  },
  // The Terminal coming up under the thread (⌘J), in a temp folder of its own
  // (the demo threads' folders are Mike's), and going away again.
  terminal: {
    setup: async (app) => {
      await scenes.archive.setup(app);
      const d = await app.dump();
      const folder = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'tb-clip-')));
      await app.do({ cmd: 'mock', method: 'terminal', params: { threadId: d.selection!.id, cwd: folder } });
    },
    act: async (app) => {
      await app.do({ cmd: 'menu-key', key: 'j', modifiers: ['cmd'] });
    },
  },
  'terminal-close': {
    setup: async (app) => {
      await scenes.terminal.setup(app);
      await scenes.terminal.act(app);
      await sleep(1200);
    },
    act: async (app) => scenes.terminal.act(app),
  },
  // The pane beside the thread, closed and opened from the right rail.
  'highlights-close': {
    setup: async (app) => scenes.archive.setup(app),
    act: async (app) => {
      await app.do({ cmd: 'click', tip: 'Highlights' });
    },
  },
  hibernate: {
    setup: async (app) => scenes.archive.setup(app),
    act: async (app) => {
      const d = await app.dump();
      await app.do({ cmd: 'snooze', id: d.selection!.id, seconds: 3600 });
    },
  },
};

async function record(app: Driver, file: string): Promise<void> {
  const s = scenes[scene];
  if (!s) throw new Error(`no scene "${scene}": ${Object.keys(scenes).join(', ')}`);
  await app.start();
  try {
    await app.do({ cmd: 'appearance', mode: 'dark' });
    await app.do({ cmd: 'resize', w: 1180, h: 760 });
    await s.setup(app);
    await app.do({ cmd: 'front' });
    await sleep(800);
    // By its process, not whatever's in front: another run may share the desktop.
    const [x, y, w, h] = windowFrame(pidOf(app));
    if (!w) throw new Error(`${app.name}'s window isn't on screen`);
    fs.rmSync(file, { force: true });
    const rec = spawn('screencapture', ['-v', '-V', '3', '-R', `${x},${y},${w},${h}`, file], { stdio: 'ignore' });
    const done = new Promise<void>((resolve) => rec.once('exit', () => resolve()));
    await sleep(900);
    await s.act(app);
    if (busy > 0) await app.do({ cmd: 'stall', seconds: busy });
    await done;
  } finally {
    await app.stop();
  }
}

const swift = path.join(out, `${scene}-swift.mov`);
const gpui = path.join(out, `${scene}-gpui.mov`);
await record(new SwiftDriver(), swift);
await record(new GpuiDriver(), gpui);
const name = busy > 0 ? `${scene}-busy` : scene;
// Both at the Swift window's height, side by side, with a gap between.
const stack = '[0:v]scale=-2:720,pad=iw+16:ih:0:0:black[a];[1:v]scale=-2:720[b];[a][b]hstack=inputs=2,format=yuv420p';
execFileSync(ffmpeg, ['-y', '-loglevel', 'error', '-i', swift, '-i', gpui, '-filter_complex', `${stack}[v]`, '-map', '[v]', '-r', '60', '-c:v', 'libx264', '-crf', '20', path.join(out, `${name}.mp4`)]);
execFileSync(ffmpeg, ['-y', '-loglevel', 'error', '-i', swift, '-i', gpui, '-filter_complex', `${stack},setpts=4*PTS[v]`, '-map', '[v]', '-r', '60', '-c:v', 'libx264', '-crf', '20', path.join(out, `${name}-slow.mp4`)]);
fs.rmSync(swift, { force: true });
fs.rmSync(gpui, { force: true });
console.log(`wrote ${path.join(out, `${name}.mp4`)} and ${name}-slow.mp4`);
process.exit(0);
