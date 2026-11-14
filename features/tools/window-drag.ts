// Real mouse drags from along the top of the GPUI window, saying which of them
// move it. A scripted click can't show this: the window move is AppKit's, from a
// real mouse event. Takes over the pointer, so only on a machine nobody's using:
//
//   desktop/scripts/remote.sh tinker-desk --desk -- node features/tools/window-drag.ts
import { GpuiDriver } from '../support/gpui.ts';
import { sleep } from '../support/driver.ts';
import { movePointer, pointer, realDrag, realGripDrag, swiftTool, windowFrame, type Point } from './real-screen.ts';
import { ensurePerfLock } from './perf-lock.ts';

await ensurePerfLock();

const dragger = swiftTool('realdrag');

/** A click, checked just before it's posted. */
function click(at: Point): void {
  realDrag(dragger, at, at, 0, { pid: app.pid! });
}
/** A drag by a grip on the window, which may move it: checked just before it's posted. */
function gripDrag(from: Point, to: Point): void {
  // Moving straight away, as a hand does: a press held still is a panel drag.
  realGripDrag(dragger, from, to, 0.1, { pid: app.pid!, env: { RD_PRESS: '0.02' } });
}

const app = new GpuiDriver();
const pointerWas = pointer();
await app.start();
try {
  for (let i = 0; i < 300; i += 1) {
    const d = await app.dump();
    if (d.connection === 'connected' && d.sidebar.sections.some((s) => s.threads.length)) break;
    await sleep(50);
  }
  const id = (await app.dump()).sidebar.sections.flatMap((s) => s.threads).find((t) => t.title === 'Morning brief')!.id;
  await app.do({ cmd: 'open', id });
  await sleep(1200);
  const frame = () => windowFrame(app.pid!);
  // A click on a blank part of the transcript first, to make the window key.
  const [x0, y0, w0, h0] = frame();
  console.log(`window ${w0} x ${h0}`);
  click([x0 + 700, y0 + 480]);
  await sleep(500);
  const spots: [string, number, number][] = [
    ['activity bar, top', 24, 14],
    ['beside the traffic lights', 92, 14],
    ['top of the thread list', 200, 14],
    ['thread bar, over the title', 400, 14],
    ['thread bar, blank part', 620, 14],
    ['thread bar, blank part lower down', 620, 30],
    ['Highlights bar, blank part', 1100, 14],
  ];
  let stayed = 0;
  for (const [name, dx, dy] of spots) {
    const [x, y] = frame();
    gripDrag([x + dx, y + dy], [x + dx + 70, y + dy + 50]);
    await sleep(700);
    const [nx, ny] = frame();
    console.log(`${nx !== x || ny !== y ? 'moved' : 'STAYED'}  ${name}  (${x},${y}) -> (${nx},${ny})`);
    if (nx === x && ny === y) stayed += 1;
    if ((await app.dump()).header?.renaming) {
      console.log(`  RENAMING after the drag from ${name}: a drag isn't a click`);
      stayed += 1;
      await app.do({ cmd: 'menu-key', key: 'escape' });
    }
    if (nx !== x || ny !== y) {
      // Back where it was, by the same grip.
      gripDrag([nx + dx, ny + dy], [x + dx, y + dy]);
      await sleep(700);
    }
  }
  // A plain click on the title still renames.
  const [x, y] = frame();
  click([x + 400, y + 14]);
  await sleep(700);
  const renaming = (await app.dump()).header?.renaming === true;
  console.log(renaming ? 'a click on the title renames' : 'CLICK ON THE TITLE DID NOT RENAME');
  console.log(stayed === 0 && renaming ? 'PASS' : 'FAIL');
} catch (error) {
  console.error(error instanceof Error ? error.message : error);
  process.exitCode = 1;
} finally {
  try {
    await app.stop();
  } catch (error) {
    // A failed stop is a failed run, even after every check passed.
    console.error(error instanceof Error ? error.message : error);
    process.exitCode = 1;
  } finally {
    try {
      movePointer(pointerWas);
    } catch {
      // Putting it back is a courtesy; the run's result stands.
    }
    process.exit();
  }
}
