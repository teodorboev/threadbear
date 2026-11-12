// Pictures of the design review window in both apps, for checking the GPUI
// one against the Swift one (settings-shot.ts does Settings, compare.ts the
// threads). Opens the mock's "Compare account picker designs" review, or for
// the states starting "shots" one with several shots an option, a page and a
// clip (support/review-demo.ts), and takes the window in each state below.
//
//   node features/tools/review-shot.ts <out folder> [state ...]
//
// States by name (see `states`); with none, all of them. TB_APPEARANCE sets
// the look, TB_APPS=swift or gpui takes one app only. Needs the apps built
// and a desktop, so on Tinker Desk (compare.ts's header says how). Each pair
// also gets an overlay (pixel-overlay.swift) where both apps made one.
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { GpuiDriver } from '../support/gpui.ts';
import { SwiftDriver } from '../support/swift.ts';
import { okValue, sleep, type Driver } from '../support/driver.ts';
import { SHOTS_REVIEW, shotsReview } from '../support/review-demo.ts';

const [outArg, ...wanted] = process.argv.slice(2);
if (!outArg) throw new Error('usage: review-shot.ts <out folder> [state ...]');
const out = path.resolve(outArg);
fs.mkdirSync(out, { recursive: true });

/** Each state's `review` commands, run in order on a fresh window. */
const states: Record<string, Record<string, unknown>[]> = {
  first: [{}],
  second: [{ show: 1 }],
  all: [{ mode: 'All' }],
  picked: [{ pick: ['B', 'C'], reply: 'B, with the pool picker from C.' }, { mode: 'All' }],
  none: [{ none: true }, { mode: 'All' }],
  actual: [{ show: 0, actual: true }],
  reference: [{ show: 3 }],
  answered: [{ pick: 'C', reply: 'C, please.' }, { submit: true }, { mode: 'All' }],
  // A's second shot, with the film strip of its three; all of them; B's page
  // and C's clip on the stage, as stills here with a button to open them.
  shots: [{ show: 0, shot: 1 }],
  'shots-all': [{ mode: 'All' }],
  'shots-page': [{ show: 1, shot: 1 }],
  'shots-video': [{ show: 2 }],
};
const names = wanted.length ? wanted : Object.keys(states);
const apps = (process.env.TB_APPS ?? 'swift,gpui').split(',');

async function shoot(name: 'swift' | 'gpui', app: Driver): Promise<void> {
  await app.start();
  try {
    let id = '';
    for (let i = 0; i < 400 && !id; i += 1) {
      const d = await app.dump();
      id = d.sidebar.sections.flatMap((s) => s.threads).find((t) => t.title === 'Compare account picker designs')?.id ?? '';
      if (!id) await sleep(50);
    }
    if (process.env.TB_APPEARANCE) await app.do({ cmd: 'appearance', mode: process.env.TB_APPEARANCE });
    for (const state of names) {
      // A fresh copy of the thread each time: an answered review stays answered.
      await app.do({ cmd: 'open', id });
      for (let i = 0; i < 200; i += 1) {
        const d = await app.dump();
        if (d.transcript.state === 'loaded' && d.selection?.id === id) break;
        await sleep(50);
      }
      let review = '';
      const shots = state.startsWith('shots');
      // A fresh copy each time, as the mock's review.updated replaces it.
      if (shots) {
        await app.do({ cmd: 'mock', method: 'review', params: { threadId: id, review: shotsReview() } });
        for (let i = 0; i < 200; i += 1) {
          const d = await app.dump();
          if (d.transcript.rows.some((r) => r.kind === 'review' && r.text.includes('Picker, step by step'))) break;
          await sleep(50);
        }
      }
      for (const step of states[state] ?? []) {
        const answer = JSON.parse(okValue(await app.do({ cmd: 'review', ...(shots ? { id: SHOTS_REVIEW } : {}), ...step }, 30))) as { id: string };
        review = answer.id;
      }
      await sleep(1500);
      await app.do({ cmd: 'screenshot', review, path: path.join(out, `${state}-${name}.png`) });
      await app.do({ cmd: 'review', ...(shots ? { id: SHOTS_REVIEW } : {}), close: true });
      console.log(`${name}: ${state}`);
    }
  } finally {
    await app.stop();
  }
}

if (apps.includes('swift')) await shoot('swift', new SwiftDriver());
if (apps.includes('gpui')) await shoot('gpui', new GpuiDriver());

const overlay = path.join(path.dirname(new URL(import.meta.url).pathname), 'pixel-overlay.swift');
for (const state of names) {
  const [swift, gpui] = ['swift', 'gpui'].map((n) => path.join(out, `${state}-${n}.png`));
  if (os.platform() === 'darwin' && fs.existsSync(swift) && fs.existsSync(gpui)) {
    try {
      execFileSync('swift', [overlay, 'overlay', swift, gpui, path.join(out, `${state}-overlay.png`)], { stdio: 'ignore' });
    } catch {
      // Pictures of different sizes; the pair is still there to look at.
    }
  }
}
process.exit(0);
