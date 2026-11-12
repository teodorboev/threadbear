import { Given, Then } from '@cucumber/cucumber';
import assert from 'node:assert/strict';
import type { Dump, Rect } from '../support/driver.ts';
import type { ThreadbearWorld } from '../support/world.ts';

// Both apps' dumps give `panelMotion`: where each panel's group was drawn when
// the layout last changed with its glide, and where it is now (or, in the
// GPUI app, where it's going), by panel. The GPUI app says too whether the
// glide is still running; the Swift app's runs in SwiftUI and doesn't say.

const area = (r: Rect) => r.width * r.height;
const settled = (d: Dump) => d.panelMotion && d.panelMotion.moving !== true;

Given('the panels have settled with {string} in them', async function (this: ThreadbearWorld, panel: string) {
  await this.until(`the panels settled with ${panel}`, (d) => settled(d) && d.panelMotion!.to[panel]);
});

Then('the panels glide, with {string} {word} than it was', async function (this: ThreadbearWorld, panel: string, way: string) {
  const m = await this.until(`${panel} gliding ${way}`, (d) => {
    const m = d.panelMotion;
    if (!m?.from[panel] || !m.to[panel]) return undefined;
    const grew = area(m.to[panel]) > area(m.from[panel]);
    return grew === (way === 'bigger') ? m : undefined;
  });
  // It started where the panel was drawn: inside the window, not from nothing.
  assert.ok(m.from[panel].width > 100 && m.from[panel].height > 100, `${panel} started as ${JSON.stringify(m.from[panel])}`);
  await this.until('the glide to end', (d) => settled(d));
});

Then('{string} glides in where it wasn\'t', async function (this: ThreadbearWorld, panel: string) {
  const d = await this.app.dump();
  const m = d.panelMotion!;
  assert.equal(m.from[panel], undefined, `${panel} was already there: ${JSON.stringify(m.from)}`);
  assert.ok(m.to[panel] && m.to[panel].width > 50 && m.to[panel].height > 50, `${panel} ended as ${JSON.stringify(m.to[panel])}`);
});

Then('{string} glides out', async function (this: ThreadbearWorld, panel: string) {
  const d = await this.app.dump();
  const m = d.panelMotion!;
  assert.ok(m.from[panel], `${panel} wasn't there: ${JSON.stringify(m.from)}`);
  assert.equal(m.to[panel], undefined, `${panel} is still there: ${JSON.stringify(m.to)}`);
});
