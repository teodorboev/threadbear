// Pictures of the Settings window's sections, in both apps, for checking a
// section against the Swift one (features/tools/compare.ts does the threads).
//
//   node features/tools/settings-shot.ts <out folder> [section ...]
//
// Sections by their names (general, models, pools, runners, devices,
// keyboard); with none, all of them. TB_APPEARANCE sets the look. Needs both
// apps built and a desktop, so on Tinker Desk (compare.ts's header says how).
import fs from 'node:fs';
import path from 'node:path';
import { GpuiDriver } from '../support/gpui.ts';
import { SwiftDriver } from '../support/swift.ts';
import { sleep, type Driver } from '../support/driver.ts';

const [outArg, ...wanted] = process.argv.slice(2);
if (!outArg) throw new Error('usage: settings-shot.ts <out folder> [section ...]');
const out = path.resolve(outArg);
fs.mkdirSync(out, { recursive: true });
const sections = wanted.length ? wanted : ['general', 'models', 'pools', 'runners', 'devices', 'keyboard'];

async function shoot(name: 'swift' | 'gpui', app: Driver): Promise<void> {
  await app.start();
  try {
    for (let i = 0; i < 400; i += 1) {
      if ((await app.dump()).connection === 'connected') break;
      await sleep(50);
    }
    if (process.env.TB_APPEARANCE) await app.do({ cmd: 'appearance', mode: process.env.TB_APPEARANCE });
    for (const section of sections) {
      await app.do({ cmd: 'settings', tab: section });
      await sleep(1200);
      await app.do({ cmd: 'screenshot', settings: true, path: path.join(out, `${section}-${name}.png`) });
      console.log(`${name}: ${section}`);
    }
  } finally {
    await app.stop();
  }
}

await shoot('swift', new SwiftDriver());
await shoot('gpui', new GpuiDriver());
process.exit(0);
