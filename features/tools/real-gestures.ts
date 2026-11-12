// Real trackpad swipes and mouse drags on the GPUI window, through the window
// server, as a hand makes them (app/Tools/post-gesture.swift and realdrag.swift):
// a row swiped away and back, Back and Forward by swiping the window, and rows
// and pictures dragged. Made-up events handed to a handler passed every test in
// the Swift app and failed on Mike's trackpad twice (7 Nov), so each of these is
// tried this way before it lands. Takes over the pointer, so only on a machine
// nobody's using (AGENTS.md, "Tests that need the real screen"):
//
//   desktop/scripts/remote.sh tinker-desk --desk -- bash -c \
//     '(cd desktop && cargo build -p threadbear-gpui --features automation) && node features/tools/real-gestures.ts'
import { execFileSync, spawn } from 'node:child_process';
import { GpuiDriver } from '../support/gpui.ts';
import { sleep, type Dump } from '../support/driver.ts';
import { assertInFront, swiftTool, windowFrame } from './real-screen.ts';

const gesture = swiftTool('post-gesture');
const dragger = swiftTool('realdrag');
const dropTarget = swiftTool('droptarget');

// post-gesture takes the fingers' way; these say the content's, which is what
// rows and page swipes go by. With natural scrolling the two are the same.
let natural = true;
try {
  natural = execFileSync('defaults', ['read', '-g', 'com.apple.swipescrolldirection']).toString().trim() !== '0';
} catch {
  // Never set: natural scrolling, the system's default.
}
const fingers = (contentDx: number) => (natural ? contentDx : -contentDx);
/** Two fingers at a screen point: `moves` content-way steps across, then lift. */
function swipe(x: number, y: number, moves: [number, number][], momentum = false): void {
  const tokens = moves.map(([dx, dy]) => `${fingers(dx)},${natural ? dy : -dy}`);
  execFileSync(gesture, [String(x), String(y), '0', ...tokens, ...(momentum ? ['M'] : [])]);
}
const steps = (total: number, count: number): [number, number][] => Array.from({ length: count }, () => [total / count, 0]);

const app = new GpuiDriver();
const results: string[] = [];
let failed = 0;
function check(name: string, ok: boolean, detail = ''): void {
  results.push(`${ok ? 'PASS' : 'FAIL'}  ${name}${detail ? `  (${detail})` : ''}`);
  if (!ok) failed += 1;
}

const titled = (d: Dump, title: string) => d.sidebar.sections.flatMap((s) => s.threads).find((t) => t.title === title);
const section = (d: Dump, title: string) => d.sidebar.sections.find((s) => s.threads.some((t) => t.title === title))?.name;
async function until(what: (d: Dump) => boolean, ms = 3000): Promise<boolean> {
  const end = Date.now() + ms;
  for (;;) {
    if (what(await app.dump())) return true;
    if (Date.now() > end) return false;
    await sleep(80);
  }
}
async function open(title: string): Promise<void> {
  const t = titled(await app.dump(), title);
  if (!t) throw new Error(`no thread called ${title}`);
  await app.do({ cmd: 'open', id: t.id });
  await sleep(600);
}
/** A row's middle on screen, from the app's own layout. */
async function rowPoint(title: string): Promise<[number, number]> {
  const t = titled(await app.dump(), title)!;
  const [, x, y] = (await app.do({ cmd: 'row-point', id: t.id })).split(' ').map(Number);
  const [wx, wy] = windowFrame(app.pid!);
  return [wx + x, wy + y];
}

await app.start();
try {
  for (let i = 0; i < 300; i += 1) {
    const d = await app.dump();
    if (d.connection === 'connected' && d.sidebar.sections.some((s) => s.threads.length)) break;
    await sleep(50);
  }
  // A scripted window starts behind everything; these need it in front.
  await app.do({ cmd: 'front' });
  await sleep(800);
  const [wx, wy, ww, wh] = windowFrame(app.pid!);
  console.log(`window ${ww} x ${wh}, natural scrolling ${natural ? 'on' : 'off'}`);
  // Over the thread, low enough to miss a system alert in the middle of
  // the screen, which sits above every window.
  const middle: [number, number] = [wx + Math.round(ww * 0.6), wy + Math.round(wh * 0.7)];

  // History to go back through: Morning brief, then Make uploads.
  await open('Morning brief');
  await open('Make uploads resume after a crash');
  let row = await rowPoint('Morning brief');
  assertInFront(app.pid!, [row, middle]);
  // A click on a blank part of the transcript first, to make the window key.
  execFileSync(dragger, [String(middle[0]), String(middle[1]), String(middle[0]), String(middle[1]), '0', '']);
  await sleep(400);

  // A row: the fingers held part way, it's that far along...
  const held = spawn(gesture, [String(row[0]), String(row[1]), '1500', ...steps(-40, 8).map(([dx, dy]) => `${fingers(dx)},${natural ? dy : -dy}`)]);
  await sleep(1100);
  const along = await app.do({ cmd: 'row-swipe' });
  await new Promise((done) => held.on('exit', done));
  check('a row follows the fingers part way', /offset -40 armed false/.test(along), along);
  // ...and let go short of the line, it goes back into place.
  await sleep(700);
  check('a short swipe on a row leaves it in the Inbox', section(await app.dump(), 'Morning brief') === 'Inbox', section(await app.dump(), 'Morning brief'));
  // So one that went wrong doesn't take the rest with it.
  if (section(await app.dump(), 'Morning brief') !== 'Inbox') {
    await app.do({ cmd: 'undo-archive' });
    await until((d) => section(d, 'Morning brief') === 'Inbox');
    await sleep(500);
  }
  row = await rowPoint('Morning brief');

  // Past the line, it's archived.
  swipe(row[0], row[1], steps(-220, 14));
  check('a swipe past the line archives the row', await until((d) => section(d, 'Morning brief') === 'Archived'), section(await app.dump(), 'Morning brief'));
  await app.do({ cmd: 'undo-archive' });
  await until((d) => section(d, 'Morning brief') === 'Inbox');
  await sleep(500);

  // Back: the content to the right across the transcript.
  swipe(middle[0], middle[1], steps(300, 15));
  check('two fingers right across the window go back', await until((d) => d.selection?.title === 'Morning brief'), (await app.dump()).selection?.title);
  // Forward, with the trackpad's momentum after the lift, which mustn't scroll.
  swipe(middle[0], middle[1], steps(-300, 15), true);
  check('two fingers left go forward again', await until((d) => d.selection?.title === 'Make uploads resume after a crash'), (await app.dump()).selection?.title);
  await sleep(500);

  // Short of the line: nowhere.
  swipe(middle[0], middle[1], steps(90, 9));
  await sleep(600);
  check('a short page swipe stays put', (await app.dump()).selection?.title === 'Make uploads resume after a crash');

  // Up and down is a scroll, never a swipe.
  swipe(middle[0], middle[1], Array.from({ length: 12 }, () => [1, 14]));
  await sleep(600);
  check('a scroll up and down is not a page swipe', (await app.dump()).selection?.title === 'Make uploads resume after a crash');

  // Over a row, content to the right is Back's while there's somewhere to go.
  row = await rowPoint('Morning brief');
  swipe(row[0], row[1], steps(300, 15));
  check('a swipe right over a row goes back', await until((d) => d.selection?.title === 'Morning brief'), (await app.dump()).selection?.title);
  await open('Make uploads resume after a crash');

  // The mouse: a row dragged left past the line is archived too.
  row = await rowPoint('Morning brief');
  execFileSync(dragger, [String(row[0]), String(row[1]), String(row[0] - 220), String(row[1]), '0.1', '']);
  check('a row dragged left past the line is archived', await until((d) => section(d, 'Morning brief') === 'Archived'), section(await app.dump(), 'Morning brief'));
  await app.do({ cmd: 'undo-archive' });
  await until((d) => section(d, 'Morning brief') === 'Inbox');
  await open('Make uploads resume after a crash');
  await sleep(400);

  // A row dragged into the open thread goes into its message as its chip,
  // and the thread the press opened gives way to the one it's dropped on.
  row = await rowPoint('Morning brief');
  execFileSync(dragger, [String(row[0]), String(row[1]), String(middle[0]), String(middle[1]), '0.2', '']);
  await sleep(700);
  const chips = await app.do({ cmd: 'composer-chips' }).catch((e) => String(e));
  const after = (await app.dump()).selection?.title;
  check('a row dragged onto the thread goes in as its chip', /thread: Morning brief/.test(chips) && after === 'Make uploads resume after a crash', `${after}; ${chips}`);
  await app.do({ cmd: 'type', text: '', replace: true });

  // Files dragged out of the window go as the files themselves, to a window
  // of our own beside it (app/Tools/droptarget.swift), as a drop on the
  // Finder would copy into whatever folder someone has open there. Right of
  // the window if there's room, else left of it.
  const target: [number, number, number, number] = wx + ww + 260 < 1900 ? [wx + ww + 20, wy + 150, 240, 240] : [Math.max(0, wx - 260), wy + 150, 240, 240];
  async function dragOut(what: string, from: [number, number], name: string): Promise<void> {
    const to: [number, number] = [target[0] + 120, target[1] + 120];
    const drops = spawn(dropTarget, [...target.map(String), '12']);
    let said = '';
    drops.stdout.on('data', (d) => (said += d));
    for (let i = 0; i < 50 && !said.includes('ready'); i += 1) await sleep(100);
    try {
      assertInFront(app.pid!, [from]);
      execFileSync(dragger, [String(from[0]), String(from[1]), String(to[0]), String(to[1]), '0.4', '']);
      await sleep(1200);
      const dropped = said.split('\n').find((l) => l.startsWith('dropped '))?.slice('dropped '.length) ?? '';
      check(`${what} dragged out of the window goes as its file`, dropped.endsWith(`/${name}`), dropped || 'nothing dropped');
    } catch (e) {
      check(`${what} dragged out of the window goes as its file`, false, e instanceof Error ? e.message : String(e));
    } finally {
      drops.kill();
    }
  }
  // A file in Highlights, in the pane on the right.
  await open('Rebuild the three landing pages');
  await sleep(800);
  const highlight = await app.do({ cmd: 'label-point', label: 'pricing-before.png' }).catch((e) => String(e));
  if (highlight.startsWith('ok')) {
    const [, rx, ry] = highlight.split(' ').map(Number);
    await dragOut('a Highlights file', [wx + rx, wy + ry], 'pricing-before.png');
  } else {
    check('a Highlights file dragged out of the window goes as its file', false, highlight);
  }
  // A picture in the transcript: the screenshots in "Worked for 21s",
  // opened and brought on screen, in a narrower window so they're left of
  // the middle of the screen, where a system alert can sit over everything.
  await app.do({ cmd: 'resize', w: 820, h: 800 });
  await sleep(600);
  await open('Make uploads resume after a crash');
  const group = (await app.dump()).transcript.rows.find((r) => r.kind === 'group' && r.text === 'Worked for 21s');
  if (group) await app.do({ cmd: 'expand', row: group.id });
  await sleep(400);
  const pictures = (await app.dump()).transcript.rows.filter((r) => r.kind === 'images').at(-1);
  if (pictures) await app.do({ cmd: 'scroll', row: pictures.id, offset: 40 });
  await sleep(600);
  const tile = await app.do({ cmd: 'tile-point', count: 2 }).catch((e) => String(e));
  if (tile.startsWith('ok')) {
    const [, tx, ty, name] = tile.split(' ');
    await dragOut('a picture', [wx + Number(tx), wy + Number(ty)], name);
  } else {
    check('a picture dragged out of the window goes as its file', false, tile);
  }
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
