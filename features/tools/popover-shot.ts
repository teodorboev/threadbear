// Pictures of a composer chip's popover (the model picker, the login picker,
// the context ring's) in both apps, for checking one against the Swift one.
// The Swift app's popover is a window of its own, taken by its number; the
// GPUI app's is drawn in its main window, which is taken whole.
//
//   node features/tools/popover-shot.ts <out folder> <thread title> <menu> ['<json command>' ...]
//
// `menu` is model, account or context, as `composer-menu` takes them. Commands
// after it run once the thread's open, before the popover does, each as the
// apps' automation takes them, or {"sleep":500}. TB_APPEARANCE sets the look;
// `@remote-threads`' threads need TB_REMOTE_THREADS=1. Needs both apps built
// and a desktop, so on Tinker Desk (compare.ts's header says how).
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { GpuiDriver } from '../support/gpui.ts';
import { SwiftDriver } from '../support/swift.ts';
import { sleep, type Driver } from '../support/driver.ts';

const [outArg, title, menu, ...steps] = process.argv.slice(2);
if (!outArg || !title || !menu) throw new Error('usage: popover-shot.ts <out folder> <thread title> <menu> [command ...]');
const out = path.resolve(outArg);
fs.mkdirSync(out, { recursive: true });
if (process.env.TB_REMOTE_THREADS) process.env.TB_MOCK_REMOTE = '1';

async function shoot(name: 'swift' | 'gpui', app: Driver): Promise<void> {
  await app.start();
  try {
    let id = '';
    for (let i = 0; i < 400 && !id; i += 1) {
      const d = await app.dump();
      id = d.sidebar.sections.flatMap((s) => s.threads).find((t) => t.title === title)?.id ?? '';
      if (!id) await sleep(50);
    }
    if (!id) throw new Error(`${name}: no thread called ${title}`);
    if (process.env.TB_APPEARANCE) await app.do({ cmd: 'appearance', mode: process.env.TB_APPEARANCE });
    await app.do({ cmd: 'open', id });
    await sleep(1500);
    for (const raw of steps) {
      const step = JSON.parse(raw) as Record<string, unknown>;
      if (typeof step.sleep === 'number') await sleep(step.sleep);
      else await app.do(step);
    }
    const said = await app.do({ cmd: 'composer-menu', menu, show: true });
    // A breakdown or a picker that reads from the runner, a moment to come.
    await sleep(1500);
    const file = path.join(out, `${menu}-${name}.png`);
    const window = /ok window (\d+)/.exec(said)?.[1];
    if (name === 'swift' && window && window !== '0') execFileSync('screencapture', ['-x', '-o', `-l${window}`, file]);
    else await app.shot(file);
    console.log(`${name}: ${file}`);
  } finally {
    await app.stop();
  }
}

await shoot('swift', new SwiftDriver());
await shoot('gpui', new GpuiDriver());
process.exit(0);
