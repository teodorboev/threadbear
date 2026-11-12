// Crash reports (features/crash-reports.feature): the GPUI app's `panic`
// panics on a thread of its own, and `crashes` lists what the panic hook kept
// beside the automation file (desktop/gpui/src/crash_reporting.rs).
import { Then, When } from '@cucumber/cucumber';
import assert from 'node:assert/strict';
import { sleep } from '../support/driver.ts';
import type { ThreadbearWorld } from '../support/world.ts';

type Panic = { message: string; location: string; frames: string[]; mainThread: boolean };

/** The newest panic the app has kept, waited for: the hook writes it as the thread unwinds. */
async function newestPanic(world: ThreadbearWorld, ms = 15_000): Promise<Panic> {
  const deadline = Date.now() + ms;
  for (;;) {
    const out = await world.app.do({ cmd: 'crashes' });
    const panics = (JSON.parse(out.slice(out.indexOf('{'))) as { panics: Panic[] }).panics;
    if (panics.length) return panics[panics.length - 1];
    if (Date.now() > deadline) throw new Error('The app kept no panic');
    await sleep(250);
  }
}

When('something in the app panics with the message:', async function (this: ThreadbearWorld, message: string) {
  await this.app.do({ cmd: 'panic', message });
});

Then('the app has kept that panic, written as:', async function (this: ThreadbearWorld, written: string) {
  const p = await newestPanic(this);
  assert.equal(p.message, written.trim());
  // It was another thread's: the app carried on, and still answers.
  assert.equal(p.mainThread, false);
  assert.equal((await this.app.dump()).connection, 'connected');
});

Then("the panic says where in the app's code it was", async function (this: ThreadbearWorld) {
  const p = await newestPanic(this);
  assert.match(p.location, /automation\.rs:\d+:\d+$/, `it says ${p.location}`);
  assert.ok(p.frames.some((f) => f.includes('automation')), `its frames: ${JSON.stringify(p.frames)}`);
  assert.ok(!p.frames.some((f) => f.includes('/Users')), 'no paths in its frames');
});
