import { Then } from '@cucumber/cucumber';
import assert from 'node:assert/strict';
import type { Dump } from '../support/driver.ts';
import type { ThreadbearWorld } from '../support/world.ts';

// The GPUI app's dump says how the last thread opened (`opening`,
// open_motion.rs): clicked, the way the arrows went, from the new-thread page,
// or still, under the archive flight.

/** Waits for the open thread to be the one the dump says opened, `how`. */
async function openedAs(world: ThreadbearWorld, how: string): Promise<NonNullable<Dump['opening']>> {
  return world.until(`the open thread opened as "${how}"`, (d) => (d.opening && d.selection && d.opening.thread === d.selection.id && d.opening.how === how ? d.opening : undefined));
}

Then('the open thread eases in as clicked', async function (this: ThreadbearWorld) {
  await openedAs(this, 'click');
});

Then('the open thread eases in from below', async function (this: ThreadbearWorld) {
  await openedAs(this, 'down');
});

Then('the open thread eases in from above', async function (this: ThreadbearWorld) {
  await openedAs(this, 'up');
});

Then('the open thread has no motion of its own', async function (this: ThreadbearWorld) {
  await openedAs(this, 'still');
});

Then('its box drops from the new-thread page to its place', async function (this: ThreadbearWorld) {
  await openedAs(this, 'newPage');
  // Measured once the thread's box is laid out; nothing drops with Reduce Motion on.
  const d = await this.until('the box laid out under the new thread', (now) => (now.reduceMotion || typeof now.opening?.boxDrop === 'number' ? now : undefined));
  if (d.reduceMotion) return;
  // The page's box sits 40% of the way down; the thread's is at the bottom.
  assert.ok((d.opening?.boxDrop ?? 0) < -100, `the box dropped ${d.opening?.boxDrop} points`);
});
