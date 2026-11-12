// The hang watch (features/hang-watch.feature): both apps' `stall` blocks the
// main thread, and their `hangs` says what the watch noted beside the
// automation file (HangWatch.swift, desktop/gpui/src/hang_watch.rs).
import { Then, When } from '@cucumber/cucumber';
import assert from 'node:assert/strict';
import { sleep } from '../support/driver.ts';
import type { ThreadbearWorld } from '../support/world.ts';

type Stall = { name: string; recoveredAfter?: number | null; sampled: boolean; stuckIn?: string | null; frames?: string[] };

/** The newest stall that passes `check`, waited for: `sample` writes its file after it's done, and that can take a while for a debug build's symbols. */
async function stall(world: ThreadbearWorld, what: string, check: (s: Stall) => boolean, ms = 40_000): Promise<Stall> {
  const deadline = Date.now() + ms;
  let last: Stall[] = [];
  for (;;) {
    const out = await world.app.do({ cmd: 'hangs' });
    last = (JSON.parse(out.slice(out.indexOf('{'))) as { stalls: Stall[] }).stalls;
    const found = [...last].reverse().find(check);
    if (found) return found;
    if (Date.now() > deadline) throw new Error(`Never saw ${what}. The hang watch has: ${JSON.stringify(last)}`);
    await sleep(500);
  }
}

When("the app's main thread is stuck for {int} seconds", async function (this: ThreadbearWorld, seconds: number) {
  await this.app.do({ cmd: 'stall', seconds }, seconds + 30);
});

Then('the hang watch has noted a stall of about {int} seconds', async function (this: ThreadbearWorld, seconds: number) {
  const s = await stall(this, `a stall that ended`, (x) => typeof x.recoveredAfter === 'number');
  // From the beat that went out, up to a second before the stall began.
  assert.ok(Math.abs((s.recoveredAfter ?? 0) - seconds) <= 2, `it noted ${s.recoveredAfter} s`);
});

Then("its sample shows the main thread stuck in the app's scripted control", async function (this: ThreadbearWorld) {
  // Only macOS has `sample`: elsewhere the stall is noted without one.
  if (process.platform !== 'darwin') return 'skipped';
  const s = await stall(this, 'a sample of the stall', (x) => x.sampled);
  assert.match(s.stuckIn ?? '', /automation/i, `the sample has it stuck in ${s.stuckIn}, on the path ${JSON.stringify(s.frames)}`);
});
