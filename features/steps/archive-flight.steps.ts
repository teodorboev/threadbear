import { Then, When } from '@cucumber/cucumber';
import assert from 'node:assert/strict';
import type { ThreadbearWorld } from '../support/world.ts';

// Both apps' dumps give the last flight (`flight`): where the page started
// (the transcript), where it aimed, and once it had landed, where the
// heading's box was then.

When('I hibernate the open thread {string} for an hour', async function (this: ThreadbearWorld, title: string) {
  const t = await this.until(`a thread called "${title}"`, (d) => this.thread(d, title));
  await this.app.do({ cmd: 'snooze', id: t.id, seconds: 3600 });
});

Then('the page of {string} flies from the transcript into the {string} heading', async function (this: ThreadbearWorld, title: string, to: string) {
  const t = await this.until(`a thread called "${title}"`, (d) => this.thread(d, title));
  this.flownFrom = t.id;
  const dest = to === 'archived' ? 'archive' : to;
  const f = await this.until(`${title}'s page landed in its heading`, (d) => (d.flight?.thread === t.id && d.flight.to === dest && d.flight.landed ? d.flight : undefined));
  // It started as the transcript: a page, not a row, right of the sidebar.
  assert.ok(f.start.width > 200 && f.start.height > 150, `started as ${JSON.stringify(f.start)}`);
  assert.ok(f.onHeading && f.end, `aimed at ${JSON.stringify(f.end)}, not the heading`);
  assert.ok(f.start.x > f.end.x, `started at ${f.start.x}, left of where it aimed (${f.end.x})`);
  // And ended in the heading's box, as it was once the list had closed up.
  const box = f.landedBox;
  assert.ok(box, 'no heading box when it landed');
  const slack = 3;
  const inside = f.end.x >= box.x - slack && f.end.x <= box.x + box.width + slack && f.end.y >= box.y - slack && f.end.y <= box.y + box.height + slack;
  assert.ok(inside, `aimed at ${JSON.stringify(f.end)}, the box was at ${JSON.stringify(box)}`);
});

Then('another thread opens in its place', async function (this: ThreadbearWorld) {
  const was = this.flownFrom;
  await this.until('the next thread open', (d) => !!d.selection && d.selection.id !== was);
});
