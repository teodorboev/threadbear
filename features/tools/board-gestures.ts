// Real window-server scrolling of the Highlights board, only on a test Mac.
// Through remote.sh tinker-desk --desk, take the shared perf lock around:
//   TB_REAL_SCREEN=1 node features/tools/board-gestures.ts
// The @highlights.board @real-screen scenario calls the same check.
import { execFileSync } from 'node:child_process';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { GpuiDriver } from '../support/gpui.ts';
import { sleep } from '../support/driver.ts';
import { assertInFront, recording, swiftTool, windowFrame } from './real-screen.ts';

export async function checkBoardGestures(app: GpuiDriver): Promise<void> {
  if (process.platform !== 'darwin' || process.env.TB_REAL_SCREEN !== '1') throw new Error('Board gestures need a test Mac and TB_REAL_SCREEN=1');
  if (recording()) throw new Error('A recording is running; no GUI events posted');
  const gesture = swiftTool('post-gesture');
  let backId: string | undefined;
  let natural = true;
  try { natural = execFileSync('defaults', ['read', '-g', 'com.apple.swipescrolldirection']).toString().trim() !== '0'; } catch {}
  async function point(label: string) {
    return (await app.do({ cmd: 'label-point', label })).trim().split(/\s+/).slice(-2).map(Number);
  }
  async function find(label: string) {
    for (let n = 0; n < 30; n++) {
      try { return await point(label); } catch {}
      if (n % 3 === 2) { try { await app.do({ cmd: 'click', tip: 'Next board column' }); } catch {} }
      await sleep(100);
    }
    throw Error(`Missing ${label}`);
  }
  async function openBoard() {
    await app.do({ cmd: 'open', id: 'demo_button_colour' });
    await app.do({ cmd: 'waitFor', loaded: true });
    await sleep(150);
  }
  async function wentBack(why: string) {
    for (let n = 0; n < 40; n++) {
      if ((await app.dump()).selection?.id === backId) { console.log(`PASS: ${why}`); return; }
      await sleep(75);
    }
    throw Error(why);
  }
  function swipe(at: number[], dx: number, dy: number) {
    const [wx, wy] = windowFrame(app.pid!);
    const screen: [number, number] = [wx + at[0], wy + at[1]];
    assertInFront(app.pid!, [screen]);
    const token = `${natural ? dx : -dx},${natural ? dy : -dy}`;
    execFileSync(gesture, [String(screen[0]), String(screen[1]), '0', ...Array(12).fill(token)]);
  }
  try {
    for (let n = 0; n < 100; n++) {
      if ((await app.dump()).sidebar.sections.some(s => s.threads.some(t => t.id === 'demo_button_colour'))) break;
      await sleep(100);
    }
    await app.do({ cmd: 'open', id: 'demo_upload' });
    backId = (await app.dump()).selection?.id;
    assert(backId, 'the navigation control thread did not open');
    await openBoard();
    await app.do({ cmd: 'mock', method: 'boardMore', params: { threadId: 'demo_button_colour', count: 10 } });
    const more = await find('8 more');
    await app.do({ cmd: 'click-at', x: more[0], y: more[1] });
    await app.do({ cmd: 'resize', w: 1180, h: 500 });
    execFileSync('caffeinate', ['-u', '-t', '2']);
    await app.do({ cmd: 'front' });
    await sleep(600);
    const card = 'Board card: Share a walk with a friend';
    const first = await point(card);
    const link = await point('Live site');
    let sideways = first;
    let moved = false;
    for (const [dx, dy] of [[-6, -1], [6, 1]]) {
      swipe(sideways, dx, dy);
      await sleep(400);
      const after = await point(card);
      const linkAfter = await point('Live site');
      console.log('horizontal gesture', dx, dy, 'card', sideways, after, 'pane', link, linkAfter);
      assert(Math.abs(linkAfter[1] - link[1]) <= 1, 'sideways gesture also moved the Highlights pane');
      moved ||= Math.abs(after[0] - sideways[0]) > 10;
      sideways = after;
    }
    assert(moved, 'neither sideways direction moved the board');
    console.log('PASS: real sideways gestures move the board and leave the pane in place');
    let verticalMoved = false;
    let before = sideways;
    for (const dy of [-8, 8]) {
      swipe(before, 0, dy);
      await sleep(400);
      const after = await point(card);
      console.log('vertical gesture', dy, 'card', before, after);
      assert(Math.abs(after[0] - before[0]) <= 1, 'vertical gesture also moved the board sideways');
      verticalMoved ||= Math.abs(after[1] - before[1]) > 5;
      before = after;
    }
    assert(verticalMoved, 'neither vertical direction scrolled the pane');
    console.log('PASS: real vertical gestures scroll the pane and leave the board horizontal position');
    // Positive control: ordinary page navigation must still get gestures.
    swipe([first[0], 32], 30, 0);
    await wentBack('a gesture on the pane header navigates back before the board reaches it');
    await openBoard();
    // A tall column can extend under the dock's header after vertical scroll.
    swipe(await point(card), 0, -40);
    await sleep(350);
    swipe([first[0], 32], 30, 0);
    await wentBack('a clipped board does not own gestures on the pane header');
    await openBoard();
    const folder = fs.mkdtempSync(path.join(os.tmpdir(), 'threadbear-board-preview-'));
    const file = path.join(folder, 'preview.txt');
    fs.writeFileSync(file, 'A plain preview over the board.\n');
    try {
      await app.do({ cmd: 'preview', path: file });
      await app.dump();
      swipe(first, 30, 0);
      await wentBack('a preview does not leave the old board gesture target behind');
      await openBoard();
      await app.do({ cmd: 'preview', path: file });
      await app.do({ cmd: 'float', id: 'threadbear.highlights' });
      await app.do({ cmd: 'front' });
      await sleep(500);
      swipe(first, 30, 0);
      await wentBack('a floating preview does not reserve its old rectangle in the main window');
    } finally { fs.rmSync(folder, { recursive: true, force: true }); }
  } catch (error) {
    try { const file = path.join(os.tmpdir(), `threadbear-board-gesture-${process.pid}`); fs.writeFileSync(`${file}.json`, JSON.stringify(await app.dump())); await app.shot(`${file}.png`); console.error(`Saved ${file}.png`); } catch {}
    throw error;
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const app = new GpuiDriver();
  await app.start();
  try { await checkBoardGestures(app); } finally { await app.stop(); }
}

