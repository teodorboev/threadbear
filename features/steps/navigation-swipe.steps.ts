// Two fingers across the window. Both apps' `page-swipe` sends made-up
// trackpad scrolls across the middle of the window the content's way (right
// and down positive; right is Back) through the app's own handling of them,
// and says how far along it got ("back progress 0.45 armed false"), then
// where letting go left the window (the Swift app's answers start with the
// command's name). Both dumps' `pageSwipe` is the card.
import { Then, When } from '@cucumber/cucumber';
import assert from 'node:assert/strict';
import type { ThreadbearWorld } from '../support/world.ts';

async function swipeWindow(world: ThreadbearWorld, dx: number, dy: number, lift: boolean): Promise<void> {
  world.lastAnswer = await world.app.do({ cmd: 'page-swipe', dx, dy, lift });
}

When('I swipe the window {int} points right', async function (this: ThreadbearWorld, points: number) {
  await swipeWindow(this, points, 0, true);
});

When('I swipe the window {int} points left', async function (this: ThreadbearWorld, points: number) {
  await swipeWindow(this, -points, 0, true);
});

When('I swipe the window {int} points down', async function (this: ThreadbearWorld, points: number) {
  await swipeWindow(this, 0, points, true);
});

When('I start swiping the window {int} points right', async function (this: ThreadbearWorld, points: number) {
  await swipeWindow(this, points, 0, false);
});

When('I start swiping the window {int} points left', async function (this: ThreadbearWorld, points: number) {
  await swipeWindow(this, -points, 0, false);
});

When('I swipe the window {int} points further right', async function (this: ThreadbearWorld, points: number) {
  await swipeWindow(this, points, 0, false);
});

When('I lift my fingers off the window', async function (this: ThreadbearWorld) {
  await swipeWindow(this, 0, 0, true);
});

Then('the page swipe went past the line', function (this: ThreadbearWorld) {
  assert.match(this.lastAnswer ?? '', /\bok (back|forward) .*armed true/);
});

Then('the page swipe stopped short of the line', function (this: ThreadbearWorld) {
  assert.match(this.lastAnswer ?? '', /\bok (back|forward) .*armed false/);
});

Then('the page swipe never started', function (this: ThreadbearWorld) {
  assert.match(this.lastAnswer ?? '', /\bok no swipe/);
});

// The card's lines ("Back to", the thread, its place), and whether it's past the line.
Then('the card in the window says {string}, short of the line', async function (this: ThreadbearWorld, lines: string) {
  const wanted = lines.split('|').map((s) => s.trim());
  await this.until(`the card saying ${lines}`, (d) => d.pageSwipe && wanted.every((w, i) => d.pageSwipe!.lines[i] === w) && !d.pageSwipe.armed);
});

Then('the card in the window says {string}', async function (this: ThreadbearWorld, lines: string) {
  const wanted = lines.split('|').map((s) => s.trim());
  await this.until(`the card saying ${lines}`, (d) => d.pageSwipe && wanted.every((w, i) => d.pageSwipe!.lines[i] === w));
});

Then('the card in the window is past the line', async function (this: ThreadbearWorld) {
  await this.until('the card past the line', (d) => d.pageSwipe?.armed === true);
});

Then("there's no card in the window", async function (this: ThreadbearWorld) {
  await this.until('no card', (d) => !d.pageSwipe);
});
