// Selecting a reply's words with the real pointer: slow and quick drags, a
// double-click on a word and a Shift-click that extends, in an idle thread
// and in one that's working (its spinner and working line redrawing while
// the hand moves). The automation's `select-text` selects a reply whole
// without a drag, so it can't see a drag that stops following the pointer
// (Mike, 12 Nov: "it only selects if I select it very quickly"). Takes over
// the pointer, so only on a machine nobody's using:
//
//   desktop/scripts/remote.sh tinker-desk --desk -- bash -c 'cd desktop && cargo build -p threadbear-gpui --features automation && cd ../features && node tools/real-select.ts'
//
// Prints each try, what was selected while the button was still down and
// after, and PASS or FAIL.
import { execFileSync, spawn } from 'node:child_process';
import path from 'node:path';
import { GpuiDriver } from '../support/gpui.ts';
import { sleep, type Dump } from '../support/driver.ts';
import { assertInFront, windowFrame, work } from './real-screen.ts';

const tool = path.join(work, 'realselect');
execFileSync('swiftc', ['-O', path.join(import.meta.dirname, 'realselect.swift'), '-o', tool]);

const reply = [
  'Here is where things stand.',
  '',
  '- **still active** on the main branch',
  '- **paused** until Friday',
  '- the last one is done',
  '',
  'That is everything for now.',
].join('\n');

type Line = { text: string; x: number; y: number; w: number; h: number };
type Selection = { text: string; bar: string | null; barTop: number | null; following: boolean; panelDrag: string | null };

const app = new GpuiDriver();
await app.start();
let failed = 0;
const answer = (s: string) => s.replace(/^ok /, '');
const selection = async (): Promise<Selection> => JSON.parse(answer(await app.do({ cmd: 'selection' })));

try {
  for (let i = 0; i < 300; i += 1) {
    const d = await app.dump();
    if (d.connection === 'connected' && d.sidebar.sections.some((s) => s.threads.length)) break;
    await sleep(50);
  }
  // A thread of its own, so the reply's near the top and nothing scrolls it.
  const threadId = 'select-test-thread';
  await app.do({ cmd: 'mock', method: 'thread', params: { id: threadId, title: 'Selecting words' } });
  await sleep(500);
  await app.do({ cmd: 'open', id: threadId });
  await sleep(800);

  await app.do({ cmd: 'mock', method: 'stream', params: { threadId, text: reply, hz: 60 } });
  const finished = (d: Dump) => d.transcript.rows.some((r) => r.kind === 'footer' && r.turn === 'turn_stream');
  for (let i = 0; i < 400 && !finished(await app.dump()); i += 1) await sleep(50);
  const rowId = [...(await app.dump()).transcript.rows].reverse().find((r) => r.kind === 'assistant')!.id;
  await app.do({ cmd: 'front' });
  await sleep(600);

  const lines = async (): Promise<Line[]> => JSON.parse(answer(await app.do({ cmd: 'text-lines', row: rowId })));
  const screen = async () => {
    const [wx, wy] = windowFrame(app.pid!);
    const ls = await lines();
    const find = (words: string) => {
      const l = ls.find((l) => l.text.includes(words));
      if (!l) throw new Error(`no line with ${words}: ${JSON.stringify(ls.map((l) => l.text))}`);
      // The line's middle, its left end (just inside), and past its last word.
      return { mid: wy + l.y + Math.min(l.h, 26) / 2, left: wx + l.x + 1, right: wx + l.x + l.w - 2, line: l };
    };
    return { find, wx, wy };
  };

  // A drag from one point to another, `steps` moves `step` seconds apart,
  // asking what's selected every 120 ms while the button's down.
  async function drag(from: [number, number], to: [number, number], press: number, steps: number, step: number): Promise<{ during: Selection[]; after: Selection }> {
    assertInFront(app.pid!, [from, to]);
    const child = spawn(tool, ['drag', ...from.map(String), ...to.map(String), String(press), String(steps), String(step), '0.3']);
    const during: Selection[] = [];
    let done = false;
    child.stdout.on('data', (d) => d.toString().includes('releasing') && (done = true));
    // Done too if it ends without saying so (a crash), and in any case well
    // after the drag should have: never a poll that goes on for ever.
    let code: number | null = null;
    const exited = new Promise((resolve) => child.on('exit', (c) => { code = c; done = true; resolve(c); }));
    const deadline = Date.now() + (press + steps * step) * 1000 + 15_000;
    // Not the first moment: the press itself.
    await sleep(450);
    while (!done) {
      if (Date.now() > deadline) {
        child.kill();
        throw new Error('the drag never said it was releasing');
      }
      during.push(await selection());
      await sleep(120);
    }
    await exited;
    if (code !== 0) throw new Error(`realselect drag ended with ${code}`);
    await sleep(400);
    return { during, after: await selection() };
  }
  const click = async (at: [number, number], count: number, shift = false) => {
    // Settled first, with nothing else redrawing the pane: a click that
    // changes the selection but doesn't have it drawn shows nothing then,
    // which a busy window hid half the time (the automation's read goes by
    // what was drawn, as the screen does).
    await sleep(1500);
    assertInFront(app.pid!, [at]);
    execFileSync(tool, ['click', ...at.map(String), String(count), ...(shift ? ['shift'] : [])]);
    await sleep(400);
    return selection();
  };
  const clear = async (s: Awaited<ReturnType<typeof screen>>) => {
    // A click well under the reply, in the transcript's empty part.
    const last = s.find('That is everything');
    await click([last.left + 40, last.mid + 120], 1);
  };
  // `want`: the words exactly, or the words it starts with and then more.
  const check = (name: string, got: Selection, want: string | { startsWith: string }, during: Selection[] = []) => {
    const words = got.text.trim();
    const right = typeof want === 'string' ? words === want : words.startsWith(want.startsWith) && words.length > want.startsWith.length;
    const ok = right && got.bar !== null && got.bar.trim() === words && got.panelDrag === null && during.every((d) => d.bar === null && d.panelDrag === null);
    if (!ok) failed += 1;
    const seen = during.map((d) => `${JSON.stringify(d.text.slice(0, 40))}${d.bar !== null ? ' +bar' : ''}${d.panelDrag !== null ? ` +picked up ${d.panelDrag}` : ''}`);
    console.log(`${ok ? 'pass' : 'FAIL'}  ${name}\n      got ${JSON.stringify(got.text)}${got.bar === null ? ' (no bar)' : ''}, want ${JSON.stringify(want)}`);
    if (seen.length) console.log(`      while down: ${[...new Set(seen)].join(' | ')}`);
  };

  async function round(label: string) {
    const s = await screen();
    const one = s.find('still active');
    const three = s.find('the last one');
    const oneText = one.line.text.trim();
    // Slow (5 s, 300 moves), as a careful hand; the same after resting half
    // a second on the press; then quick (0.3 s).
    for (const [speed, press, steps, step] of [['slow', 0.08, 300, 0.016], ['rest, then slow', 0.5, 180, 0.016], ['quick', 0.05, 20, 0.015]] as const) {
      await clear(s);
      const r = await drag([one.left, one.mid], [one.right, one.mid], press, steps, step);
      check(`${label}: ${speed} drag along "${oneText}"`, r.after, oneText, r.during);
    }
    await clear(s);
    const many = await drag([one.left, one.mid], [three.right, three.mid], 0.5, 200, 0.016);
    check(`${label}: slow drag down three items`, many.after, ['still active on the main branch', 'paused until Friday', 'the last one is done'].join('\n'), many.during);
    // A double-click on "Here", the reply's first word.
    await clear(s);
    const first = s.find('Here is where');
    check(`${label}: double-click a word`, await click([first.left + 6, first.mid], 2), 'Here');
    await shiftClick(label, s);
    await clickAway(label, s);
    // Not while working: the transcript follows the reply's new lines, so
    // it scrolls a line under a drag that takes a second or two.
    if (label === 'idle') await anotherItem(label, s);
    if (label === 'working') await pausedWhileSelecting(label);
  }

  // A selection in one item, then a drag along another: the new words, with
  // the bar over them. SHOTS=<dir> keeps a picture of each, to see the
  // highlight is drawn where the bar says (Mike's recording, 12 Nov, had
  // the bar over words with nothing highlighted).
  // While a thread's working, a slow drag holds the transcript still (it
  // stops following new lines while the button's down), and it follows
  // again on release, only if it was following before (Mike's pick, 12
  // Nov, "Pause while selecting").
  async function pausedWhileSelecting(label: string) {
    await app.do({ cmd: 'bottom' });
    await sleep(1500);
    // Measured just before the press: until then it follows.
    const s = await screen();
    const three = s.find('the last one');
    const r = await drag([three.left, three.mid], [three.right, three.mid], 0.05, 240, 0.016);
    const still = r.during.length > 3 && r.during.every((d) => !d.following);
    check(`${label}: a slow drag while new lines come in stays on its words`, r.after, three.line.text.trim(), r.during);
    console.log(`${still ? 'pass' : 'FAIL'}  ${label}: not following while the button's down\n      following while down: ${[...new Set(r.during.map((d) => d.following))].join(', ')}`);
    if (!still) failed += 1;
    await sleep(1500);
    const again = (await selection()).following && (await app.dump()).transcript.atBottom;
    console.log(`${again ? 'pass' : 'FAIL'}  ${label}: following again after letting go`);
    if (!again) failed += 1;
    // Scrolled up first, so not following: letting go leaves it where it is.
    await app.do({ cmd: 'scroll', to: 'top' });
    await sleep(800);
    if ((await app.dump()).transcript.atBottom) {
      console.log(`skip  ${label}: letting go when it wasn't following (the thread fits the window)`);
      return;
    }
    const top = await screen();
    const here = top.find('Here is where');
    await drag([here.left, here.mid], [here.right, here.mid], 0.05, 40, 0.016);
    await sleep(1500);
    const d = await app.dump();
    const stays = !(await selection()).following && !d.transcript.atBottom;
    console.log(`${stays ? 'pass' : 'FAIL'}  ${label}: letting go when it wasn't following leaves it where it was`);
    if (!stays) failed += 1;
    await app.do({ cmd: 'bottom' });
    await sleep(800);
  }

  async function anotherItem(label: string, s: Awaited<ReturnType<typeof screen>>) {
    await clear(s);
    const one = s.find('still active');
    const three = s.find('the last one');
    await drag([one.left, one.mid], [one.right, one.mid], 0.3, 60, 0.016);
    const r = await drag([three.left, three.mid], [three.right, three.mid], 0.3, 60, 0.016);
    if (process.env.SHOTS) {
      const file = path.join(process.env.SHOTS, `${label}-another-item.png`);
      execFileSync('screencapture', ['-x', '-R', `${Math.round(one.left - 20)},${Math.round(one.mid - 80)},520,200`, file]);
    }
    check(`${label}: a drag along another item after a selection`, r.after, three.line.text.trim(), r.during);
    barOver(`${label}: the bar over the other item`, r.after, three.line);
  }

  // A plain click on words after a selection puts it away, bar and all. In
  // Mike's recording (12 Nov) the bar came back over the click with nothing
  // highlighted.
  async function clickAway(label: string, s: Awaited<ReturnType<typeof screen>>) {
    const one = s.find('still active');
    const two = s.find('paused');
    await clear(s);
    await drag([one.left, one.mid], [one.right, one.mid], 0.05, 20, 0.015);
    for (let i = 0; i < 3; i += 1) {
      const got = await click([two.left + 30 + i * 8, two.mid], 1);
      const ok = got.text.trim() === '' && got.bar === null;
      if (!ok) failed += 1;
      console.log(`${ok ? 'pass' : 'FAIL'}  ${label}: a click on words after a selection (${i + 1})\n      got ${JSON.stringify(got.text)}${got.bar === null ? '' : ' +bar'}`);
    }
  }

  const asked = 'Where do the three branches stand today?';
  // A drag from my message on into the reply under it (the mock's canned
  // answer): the bar over my message's line, where that selection starts,
  // not over the reply's. The message is well above, so scrolled to first.
  async function fromMessage(label: string) {
    // In a thread of its own, as my message and the mock's long canned
    // answer to it push the first thread's reply around.
    const other = 'select-test-message';
    await app.do({ cmd: 'mock', method: 'thread', params: { id: other, title: 'Selecting from a message' } });
    await sleep(500);
    await app.do({ cmd: 'open', id: other });
    await sleep(800);
    await app.do({ cmd: 'send', text: asked });
    for (let i = 0; i < 100 && !(await app.dump()).transcript.rows.some((r) => r.kind === 'user'); i += 1) await sleep(50);
    await sleep(500);
    for (let i = 0; i < 600 && (await app.dump()).selection?.status === 'running'; i += 1) await sleep(50);
    const rows = (await app.dump()).transcript.rows;
    const userRowId = rows.find((r) => r.kind === 'user')!.id;
    const at = rows.findIndex((r) => r.id === userRowId);
    const cannedId = rows.slice(at).find((r) => r.kind === 'assistant')!.id;
    await app.do({ cmd: 'scroll', row: userRowId, offset: 160 });
    await sleep(800);
    const [wx, wy] = windowFrame(app.pid!);
    // Each row's own lines: the canned answer quotes my message ("You said:
    // ..."), so its words alone don't say which is which.
    const mineLines: Line[] = JSON.parse(answer(await app.do({ cmd: 'text-lines', row: userRowId })));
    const theirLines: Line[] = JSON.parse(answer(await app.do({ cmd: 'text-lines', row: cannedId })));
    const find = (ls: Line[], words: string) => {
      const l = ls.find((l) => l.text.includes(words));
      if (!l) throw new Error(`no line with ${words}: ${JSON.stringify(ls.map((l) => l.text))}`);
      return { mid: wy + l.y + Math.min(l.h, 26) / 2, left: wx + l.x + 1, right: wx + l.x + l.w - 2, line: l };
    };
    const mine = find(mineLines, 'Where do the three');
    const theirs = find(theirLines, 'You said');
    // A click clear of both first, under the reply's first line.
    await click([theirs.left + 40, theirs.mid + 400], 1);
    const r = await drag([mine.left, mine.mid], [theirs.right, theirs.mid], 0.3, 90, 0.016);
    const ok = r.after.text.trim().startsWith(asked) && r.after.text.includes('You said') && r.after.bar !== null;
    if (!ok) failed += 1;
    console.log(`${ok ? 'pass' : 'FAIL'}  ${label}: a drag from my message into the reply\n      got ${JSON.stringify(r.after.text)}${r.after.bar === null ? ' (no bar)' : ''}`);
    barOver(`${label}: the bar over my message, where that selection starts`, r.after, mine.line);
  }

  // The bar just over `line`, the selection's first: clear of it (the bar's
  // 29 points tall, 6 above the line), not over it as it was after a
  // Shift-click, and not stranded somewhere above either.
  const barOver = (name: string, got: Selection, line: Line) => {
    const ok = got.barTop !== null && got.barTop + 29 <= line.y + 2 && got.barTop >= line.y - 29 - 16;
    if (!ok) failed += 1;
    console.log(`${ok ? 'pass' : 'FAIL'}  ${name}\n      bar top ${got.barTop}, the line's top ${line.y}`);
  };

  // A Shift-click after a drag carries the selection on to it, three times
  // over, as one that wasn't drawn failed some times and not others.
  async function shiftClick(label: string, s: Awaited<ReturnType<typeof screen>>) {
    const one = s.find('still active');
    // Inside "paused", the next item's first word.
    const two = s.find('paused');
    for (let i = 1; i <= 3; i += 1) {
      await clear(s);
      await drag([one.left, one.mid], [one.right, one.mid], 0.05, 20, 0.015);
      const got = await click([two.left + 30, two.mid], 1, true);
      check(`${label}: Shift-click extends (${i})`, got, { startsWith: 'still active on the main branch\np' });
      barOver(`${label}: the bar over the selection's start after a Shift-click (${i})`, got, one.line);
    }
  }

  // ONLY=message runs the drag from a message alone.
  const all = process.env.ONLY !== 'message';
  if (all) await round('idle');
  // The same while the thread's working: a slow stream after the reply, its
  // spinner and working line redrawing all the while.
  await app.do({ cmd: 'mock', method: 'stream', params: { threadId, seconds: 120, hz: 1 } });
  await sleep(1500);
  if (all) await round('working');
  await fromMessage('idle');
  console.log(failed === 0 ? 'PASS' : `FAIL (${failed})`);
  if (failed) process.exitCode = 1;
} catch (error) {
  console.error(error instanceof Error ? error.stack : error);
  process.exitCode = 1;
} finally {
  await app.stop();
  process.exit();
}
