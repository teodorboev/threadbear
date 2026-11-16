// The GPUI app's part of mac-shots.sh: on the dev runner in <runner home>,
// opens each scenario's thread, checks its rows show the words the scenario
// must show, and saves a picture. Prints JSON: each scenario's file and the
// words it didn't find. It drives the app through the same driver as the
// behaviour specs (features/support/gpui.ts), on a real runner rather than
// the mock (TB_RUNNER_HOME), with the binary in TB_GPUI_BIN.
//
//   node scripts/parity/gpui-shots.ts <seed.json> <out dir>
import fs from 'node:fs';
import path from 'node:path';
import { execFileSync } from 'node:child_process';
import { GpuiDriver } from '../../features/support/gpui.ts';
import { sleep } from '../../features/support/driver.ts';
import { ensurePerfLock } from '../../features/tools/perf-lock.ts';

await ensurePerfLock();

const [seedFile, out] = process.argv.slice(2);
const seed = JSON.parse(fs.readFileSync(seedFile, 'utf8')) as { threads: { name: string; threadId: string; shows: string[] }[] };
const app = new GpuiDriver();
const results: { name: string; file: string; missing: string[] }[] = [];
try {
  await app.start();
  await app.do({ cmd: 'appearance', mode: 'dark' });
  for (const [i, t] of seed.threads.entries()) {
    let missing = t.shows;
    let dump = '';
    // The relay's list and the thread's rows take a moment to come through Convex.
    for (let tries = 0; tries < 20 && missing.length; tries++) {
      try {
        await app.do({ cmd: 'open', id: t.threadId });
        dump = JSON.stringify(await app.dump());
        missing = t.shows.filter((w) => !dump.includes(w));
      } catch {}
      if (missing.length) await sleep(1000);
    }
    const file = `mac-${i + 1}.png`;
    await app.shot(path.join(out, file));
    // What's needed to tell the app's drawing from the data when words are
    // missing: the app's rows and the runner's row for the thread (where a
    // done card's loose ends come from), beside the picture.
    if (missing.length) {
      let row: unknown = null;
      try {
        const list = JSON.parse(execFileSync(path.join(process.env.TB_RUNNER_HOME!, 'tb'), ['rpc', 'remoteThreads.list'], { encoding: 'utf8' }));
        row = (list.threads ?? list.result?.threads ?? []).find((r: { threadId?: string }) => r.threadId === t.threadId) ?? null;
      } catch (e) { row = String(e); }
      fs.writeFileSync(path.join(out, `mac-${i + 1}.debug.json`), JSON.stringify({ missing, row, dump: dump ? JSON.parse(dump) : null }, null, 2));
    }
    results.push({ name: t.name, file, missing });
  }
} finally {
  await app.stop();
}
console.log(JSON.stringify(results, null, 2));
// The driver's children can hold the event loop open after stop.
process.exit(0);
