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
// Scenes: archive, hibernate (archive-flight.feature), the panel ones
// (panel-motion.feature): terminal, terminal-close, highlights-close,
// threads opening (opening.feature): switch, arrows, new-thread, and the
// Swift app's small motions beside GPUI's: select, finish, archive-row,
// inbox-zero, send, bear-hover, woke, background, activity, librarian,
// milestone. `--busy` blocks each app's
// main thread for that long straight after the click (the automation's
// `stall`), as opening a heavy next thread would. Writes <scene>.mp4 at full
// speed and <scene>-slow.mp4 at a quarter of it, both apps in dark mode.
import { execFileSync, spawn } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { GpuiDriver } from '../support/gpui.ts';
import { SwiftDriver } from '../support/swift.ts';
import { okValue, sleep, type Driver, type Dump } from '../support/driver.ts';
import { windowFrame } from './real-screen.ts';
import { fileURLToPath } from 'node:url';
import { ensurePerfLock } from './perf-lock.ts';

await ensurePerfLock();

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');

const [outArg, scene = 'archive', ...rest] = process.argv.slice(2);
const only = rest.includes('--only') ? rest[rest.indexOf('--only') + 1] : '';
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
type Scene = {
  setup: (app: Driver) => Promise<void>;
  act: (app: Driver) => Promise<void>;
  /** How long to film, from 0.9 s before the act. */
  seconds?: number;
};

/** A thread's sidebar row by id, wherever it is. */
const byId = (d: Dump, id: string) => d.sidebar.sections.flatMap((s) => s.threads).find((t) => t.id === id || t.id.startsWith(id));

/** Clicks the control whose tooltip starts with `label` where it is, through
 * the window as a real click goes: both apps say where (`label-point`). */
async function clickLabel(app: Driver, label: string): Promise<void> {
  const [x, y] = okValue(await app.do({ cmd: 'label-point', label, prefix: true })).split(' ').map(Number);
  await app.do({ cmd: 'click-at', x, y });
}

/** Archives every thread in Inbox, Running and Needs you but `keep`, with
 * time for each to go, so the next archive leaves the inbox empty. */
async function archiveAllBut(app: Driver, keep: string): Promise<void> {
  // The GPUI app can answer before its list has come in.
  await until(app, 'its thread list', (d) => byId(d, keep));
  for (let i = 0; i < 30; i++) {
    const d = await app.dump();
    const live = d.sidebar.sections.filter((s) => !/archiv|hibernat|snooz/i.test(s.name)).flatMap((s) => s.threads).filter((t) => t.id !== keep);
    if (!live.length) return;
    await app.do({ cmd: 'archive', id: live[0].id });
    await sleep(500);
  }
  throw new Error(`${app.name}: the inbox won't empty`);
}

const scenes: Record<string, Scene> = {
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
  // The selected row's highlight sliding from row to row as threads open.
  select: {
    setup: async (app) => scenes.archive.setup(app),
    act: async (app) => {
      for (const id of ['demo_tidy_imports', 'demo_settings_tabs', 'demo_release_notes', 'demo_reply_buttons']) {
        await app.do({ cmd: 'select', id });
        await sleep(550);
      }
    },
    seconds: 3.5,
  },
  // Another thread finishing its turn while you watch the open one: its
  // status changing, the accent wash, the unread tick drawn on.
  finish: {
    setup: async (app) => scenes.archive.setup(app),
    act: async (app) => {
      await app.do({ cmd: 'mock', method: 'stream', params: { threadId: 'demo_dark_mode', seconds: 1.2, hz: 4, completed: { needs: 'nothing' } } });
    },
    seconds: 4.5,
  },
  // A row that isn't open archived from its button or ⌘E: it slides off.
  'archive-row': {
    setup: async (app) => scenes.archive.setup(app),
    act: async (app) => {
      await app.do({ cmd: 'archive', id: 'demo_tidy_imports', slide: true });
    },
    seconds: 2.5,
  },
  // Archiving the last thread in the inbox: the page flies to Archived, then
  // the bear rises over the new thread page and in the sidebar, and waves.
  // TB_BEAR_DATE=2026-11-13T23:00 for the bedtime one (the Swift app only).
  'inbox-zero': {
    setup: async (app) => {
      await archiveAllBut(app, 'demo_reply_buttons');
      await sleep(1500);
      await scenes.archive.setup(app);
    },
    act: async (app) => {
      await app.do({ cmd: 'archive' });
    },
    seconds: 4,
  },
  // Sending a message: it lifts into the thread, the arrow hops, the bear
  // pops up and waves while the turn runs, and the done card settles in
  // with its tick drawing on.
  send: {
    setup: async (app) => scenes.archive.setup(app),
    act: async (app) => {
      await app.do({ cmd: 'send', text: 'Any follow-ups?' });
    },
    seconds: 7,
  },
  // The working line through a long turn: Swift's bear pops up, waves,
  // breathes and blinks; GPUI's paw moves every couple of seconds.
  'bear-hover': {
    setup: async (app) => scenes.archive.setup(app),
    act: async (app) => {
      await app.do({ cmd: 'mock', method: 'stream', params: { threadId: 'demo_reply_buttons', seconds: 30, hz: 1 } });
    },
    // TB_CLIP_SECONDS for longer, to see more of the GPUI paw's moves.
    seconds: Number(process.env.TB_CLIP_SECONDS) || 8,
  },
  // A thread woken by its background jobs, opened: the bear getting up.
  woke: {
    setup: async (app) => {
      await scenes.archive.setup(app);
      // Hibernated for a moment: the mock wakes it with the runner's note.
      await app.do({ cmd: 'snooze', id: 'demo_dark_mode', seconds: 2 });
      await until(app, 'it waking', (d) => byId(d, 'demo_dark_mode')?.status !== 'snoozed' && !/hibernat/i.test(d.sidebar.sections.find((s) => s.threads.some((t) => t.id === 'demo_dark_mode'))?.name ?? 'hibernat'));
      await sleep(800);
    },
    act: async (app) => {
      await app.do({ cmd: 'open', id: 'demo_dark_mode' });
    },
    seconds: 3,
  },
  // Background work: the sitting bear (still in Swift too).
  background: {
    setup: async (app) => scenes.archive.setup(app),
    act: async (app) => {
      await app.do({ cmd: 'open', id: 'demo_landing_pages' });
    },
    seconds: 2,
  },
  // The activity bar: a click bounces the icon, the lit square slides, and
  // the sidebar's view fades and settles in.
  activity: {
    setup: async (app) => scenes.archive.setup(app),
    act: async (app) => {
      await clickLabel(app, 'Scheduled');
      await sleep(900);
      await clickLabel(app, 'Projects');
      await sleep(900);
      await clickLabel(app, 'Threads');
    },
    seconds: 4,
  },
  // The Librarian's picture, poked: a wobble and a line.
  librarian: {
    setup: async (app) => {
      await scenes.archive.setup(app);
      await clickLabel(app, 'Librarian');
      await sleep(800);
    },
    act: async (app) => {
      // Where both apps draw the picture in a 1180 by 760 window.
      const [x, y] = (process.env.TB_PICTURE_AT ?? '530,190').split(',').map(Number);
      await app.do({ cmd: 'click-at', x, y });
      await sleep(1200);
      await app.do({ cmd: 'click-at', x, y });
    },
    seconds: 3.5,
  },
  // The milestone toast dropping in with its bear waving (GPUI has none yet,
  // so its side shows nothing happening).
  milestone: {
    setup: async (app) => scenes.archive.setup(app),
    act: async (app) => {
      await app.do({ cmd: 'milestone', tier: 1000 }).catch(() => {});
    },
    seconds: 3,
  },
  hibernate: {
    setup: async (app) => scenes.archive.setup(app),
    act: async (app) => {
      const d = await app.dump();
      await app.do({ cmd: 'snooze', id: d.selection!.id, seconds: 3600 });
    },
  },
  // Threads coming in as they open (opening.feature, open_motion.rs; the
  // Swift app swaps them on the spot): clicking two rows, then the arrows.
  switch: {
    setup: async (app) => scenes.archive.setup(app),
    act: async (app) => {
      const d = await app.dump();
      const rows = d.sidebar.sections.filter((s) => !s.collapsed).flatMap((s) => s.threads).filter((t) => t.id !== d.selection?.id).slice(0, 2);
      for (const row of rows) {
        await app.do({ cmd: 'click', tipPrefix: `${row.title}\n` });
        await sleep(700);
      }
    },
  },
  arrows: {
    setup: async (app) => {
      const t = await until(app, 'Morning brief', (d) => titled(d, 'Morning brief'));
      await app.do({ cmd: 'click', tipPrefix: 'Morning brief\n' });
      await until(app, 'its transcript', (d) => d.transcript.threadId === t.id && d.transcript.state === 'loaded');
    },
    act: async (app) => {
      // A scripted Swift app's arrows only reach a text box
      // (navigation.feature), so it clicks the same rows, which swap the
      // same way there.
      const d = await app.dump();
      const rows = d.sidebar.sections.filter((s) => !s.collapsed).flatMap((s) => s.threads);
      const at = rows.findIndex((t) => t.id === d.selection?.id);
      for (const [dir, step] of [['down', 1], ['down', 2], ['up', 1]] as const) {
        if (app instanceof GpuiDriver) await app.do({ cmd: 'arrow', dir });
        else await app.do({ cmd: 'click', tipPrefix: `${rows[at + step].title}\n` });
        await sleep(600);
      }
    },
  },
  // ⌘N, then a message from the new-thread page: the page fades up, and the
  // box drops to the bottom as the thread opens.
  // A thread at work in the background: its row's arc turning in the
  // sidebar (spinner.rs's layer in the GPUI app). Swift's turns in Core
  // Animation, so the two should look alike.
  spinner: {
    setup: async (app) => {
      await scenes.archive.setup(app);
      const d = await app.dump();
      const other = d.sidebar.sections.filter((s) => !s.collapsed).flatMap((s) => s.threads).find((t) => t.id !== d.selection?.id);
      if (!other) throw new Error('no other thread to set working');
      await app.do({ cmd: 'mock', method: 'background', params: { threadId: other.id, tasks: [{ kind: 'shell', description: 'npm test' }] } });
      await sleep(1200);
    },
    act: async () => {},
  },
  'new-thread': {
    seconds: 4,
    setup: async (app) => scenes.archive.setup(app),
    act: async (app) => {
      await app.do({ cmd: 'new' });
      await sleep(700);
      await app.do({ cmd: 'send', text: 'Check the landing copy' });
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
    const rec = spawn('screencapture', ['-v', '-V', String(s.seconds ?? 3), '-R', `${x},${y},${w},${h}`, file], { stdio: 'ignore' });
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
// `--only swift|gpui` films one app, to look at, and keeps its .mov.
if (only) {
  await record(only === 'swift' ? new SwiftDriver() : new GpuiDriver(), only === 'swift' ? swift : gpui);
  console.log(`wrote ${only === 'swift' ? swift : gpui}`);
  process.exit(0);
}
await record(new SwiftDriver(), swift);
await record(new GpuiDriver(), gpui);
const name = busy > 0 ? `${scene}-busy` : scene;
// Both at the Swift window's height, side by side, with a gap between:
// 720 tall, or `--sharp` at the windows' own Retina size (1440), to crop in
// on something small (a dot, a tick) and still see it.
const height = rest.includes('--sharp') ? 1440 : 720;
const stack = `[0:v]scale=-2:${height},pad=iw+16:ih:0:0:black[a];[1:v]scale=-2:${height}[b];[a][b]hstack=inputs=2,format=yuv420p`;
execFileSync(ffmpeg, ['-y', '-loglevel', 'error', '-i', swift, '-i', gpui, '-filter_complex', `${stack}[v]`, '-map', '[v]', '-r', '60', '-c:v', 'libx264', '-crf', '20', path.join(out, `${name}.mp4`)]);
execFileSync(ffmpeg, ['-y', '-loglevel', 'error', '-i', swift, '-i', gpui, '-filter_complex', `${stack},setpts=4*PTS[v]`, '-map', '[v]', '-r', '60', '-c:v', 'libx264', '-crf', '20', path.join(out, `${name}-slow.mp4`)]);
fs.rmSync(swift, { force: true });
fs.rmSync(gpui, { force: true });
console.log(`wrote ${path.join(out, `${name}.mp4`)} and ${name}-slow.mp4`);
process.exit(0);
