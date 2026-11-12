// Swipes on a sidebar row, and rows dragged with the mouse. Both apps' `swipe`
// sends made-up trackpad scrolls across a row the content's way (left
// negative) through the app's own handling of them, and says how far along
// the row got ("offset -50 armed false"), then whether letting go archived
// it. The GPUI app's `row-drag` is the mouse.
import { Then, When } from '@cucumber/cucumber';
import assert from 'node:assert/strict';
import type { ThreadbearWorld } from '../support/world.ts';

async function rowId(world: ThreadbearWorld, title: string): Promise<string> {
  const t = await world.until(`a row called "${title}"`, (d) => world.thread(d, title));
  return t.id;
}

async function swipeRow(world: ThreadbearWorld, title: string, dx: number, lift: boolean): Promise<void> {
  world.swipedRow = await rowId(world, title);
  world.lastAnswer = await world.app.do({ cmd: 'swipe', id: world.swipedRow, dx, lift });
}

When('I swipe the row {string} {int} points left', async function (this: ThreadbearWorld, title: string, points: number) {
  await swipeRow(this, title, -points, true);
});

When('I start swiping the row {string} {int} points left', async function (this: ThreadbearWorld, title: string, points: number) {
  await swipeRow(this, title, -points, false);
});

// More of the swipe under way, on the row it started on.
When('I swipe it {int} points further', async function (this: ThreadbearWorld, points: number) {
  assert.ok(this.swipedRow, 'no swipe under way');
  this.lastAnswer = await this.app.do({ cmd: 'swipe', id: this.swipedRow, dx: -points, lift: false });
});

When('I lift my fingers off the row', async function (this: ThreadbearWorld) {
  assert.ok(this.swipedRow, 'no swipe under way');
  this.lastAnswer = await this.app.do({ cmd: 'swipe', id: this.swipedRow, dx: 0, lift: true });
});

Then('the swipe went past the line', function (this: ThreadbearWorld) {
  assert.match(this.lastAnswer ?? '', /armed true/);
});

Then('the swipe stopped short of the line', function (this: ThreadbearWorld) {
  assert.match(this.lastAnswer ?? '', /armed false.*not archived/);
});

Then('the row is {int} points along, short of the line', function (this: ThreadbearWorld, points: number) {
  assert.match(this.lastAnswer ?? '', new RegExp(`offset -${points} armed false`));
});

Then('the row is past the line', function (this: ThreadbearWorld) {
  assert.match(this.lastAnswer ?? '', /armed true/);
});

When('I drag the row {string} {int} points left with the mouse', async function (this: ThreadbearWorld, title: string, points: number) {
  const id = await rowId(this, title);
  this.lastAnswer = await this.app.do({ cmd: 'row-drag', id, dx: -points, dy: 0 });
});

// A point in the thread's transcript on both apps' windows, as the picture's drop.
When('I drag the row {string} onto the transcript', async function (this: ThreadbearWorld, title: string) {
  const id = await rowId(this, title);
  this.lastAnswer = await this.app.do({ cmd: 'row-drag', id, x: 650, y: 300 });
});
