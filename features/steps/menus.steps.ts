// The menu bar's standard menus (features/menus.feature): Edit, File > Close,
// Window, and About. Both apps' `menus` read what the menu bar has, AppKit's
// own items included (the GPUI app's add them from the real menu on a Mac).
import { Then, When } from '@cucumber/cucumber';
import assert from 'node:assert/strict';
import { okValue, sleep } from '../support/driver.ts';
import type { ThreadbearWorld } from '../support/world.ts';

type Menu = { title: string; items: { title: string }[] };

// Each title in the menu, in this order, with anything else between them.
// SwiftUI rebuilds its menus a moment after what they show changes, and
// AppKit adds its own items once the menu bar is up, so this looks a few times.
Then('the {string} menu lists {string}', async function (this: ThreadbearWorld, menu: string, list: string) {
  const wanted = list.split(' | ');
  let titles: string[] = [];
  for (let i = 0; i < 20; i += 1) {
    const out = JSON.parse(okValue(await this.app.do({ cmd: 'menus' }))) as Menu[];
    titles = out.find((m) => m.title === menu)?.items.map((x) => x.title) ?? [];
    let at = 0;
    if (wanted.every((w) => (at = titles.indexOf(w, at)) >= 0)) return;
    await sleep(250);
  }
  assert.fail(`the ${menu} menu doesn't list ${wanted.join(', ')} in that order: ${JSON.stringify(titles)}`);
});

When('I put the {string} panel in a window of its own', async function (this: ThreadbearWorld, panel: string) {
  await this.app.do({ cmd: 'float', id: `threadbear.${panel}` });
});

// Both apps' `den` chooses About Threadbear in the app menu. The GPUI app's
// window opens just after its answer, so it's asked again until it says
// what's in it; the Swift one answers once the window's up.
async function about(world: ThreadbearWorld): Promise<string> {
  let out = '';
  for (let i = 0; i < 20; i += 1) {
    out = okValue(await world.app.do({ cmd: 'den' }));
    if (out !== 'opening') return out;
    await sleep(150);
  }
  assert.fail(`the About window didn't open: ${out}`);
}

When('I choose {string} in the app menu', async function (this: ThreadbearWorld, title: string) {
  assert.equal(title, 'About Threadbear', 'only About Threadbear has a command of its own');
  this.lastAnswer = await about(this);
});

Then('the About window is open', function (this: ThreadbearWorld) {
  assert.ok(this.lastAnswer !== undefined, 'About Threadbear wasn\'t chosen');
});

Then('the About window says {string}', async function (this: ThreadbearWorld, line: string) {
  const lines = (await about(this)).split(' | ');
  assert.ok(lines.includes(line), `the About window doesn't say "${line}": ${JSON.stringify(lines)}`);
});

// By hand on Tinker Desk's desktop (see the feature file): these steps only
// exist so the scenario reads.
When('I close the main window with ⌘W', function () {
  return 'pending';
});

Then('the app quits without asking', function () {
  return 'pending';
});
