// Spelling in the message box (features/spelling.feature): the GPUI app's
// `composer-spelling` (the words marked now) and `composer-right-click` (the box's
// right-click menu as titles, "Submenu › Item", ticked ones with "✓ ").
import { Given, Then, When } from '@cucumber/cucumber';
import assert from 'node:assert/strict';
import { okValue, sleep } from '../support/driver.ts';
import type { ThreadbearWorld } from '../support/world.ts';

async function menu(world: ThreadbearWorld, at: string): Promise<string[]> {
  return JSON.parse(okValue(await world.app.do({ cmd: 'composer-right-click', at }))) as string[];
}

// The marks follow each edit; this looks a few times while they settle.
Then('the composer marks only {string} as misspelled', async function (this: ThreadbearWorld, word: string) {
  let marks: string[] = [];
  for (let i = 0; i < 20; i += 1) {
    marks = JSON.parse(okValue(await this.app.do({ cmd: 'composer-spelling' }))) as string[];
    if (JSON.stringify(marks) === JSON.stringify([`${word} (spelling)`])) return;
    await sleep(150);
  }
  assert.fail(`the box marks ${JSON.stringify(marks)}, not only "${word}"`);
});

Then('the composer\'s right-click menu on {string} offers {string}', async function (this: ThreadbearWorld, at: string, item: string) {
  const titles = await menu(this, at);
  assert.ok(titles.includes(item), `the menu on "${at}" has no "${item}": ${JSON.stringify(titles)}`);
});

Then('the composer\'s right-click menu on {string} has {string}', async function (this: ThreadbearWorld, at: string, list: string) {
  const titles = (await menu(this, at)).map((t) => t.replace(/\(off\) |✓ /g, ''));
  for (const item of list.split(' | ')) {
    assert.ok(titles.includes(item), `the menu on "${at}" has no "${item}": ${JSON.stringify(titles)}`);
  }
});

When('I choose {string} from the composer\'s right-click menu on {string}', async function (this: ThreadbearWorld, item: string, at: string) {
  await this.app.do({ cmd: 'composer-right-click', at, choose: item });
});

// A switch the test Mac may have either way: ticked if it isn't yet.
Given('{string} is ticked in the composer\'s right-click menu', async function (this: ThreadbearWorld, item: string) {
  const ticked = (titles: string[]) => titles.some((t) => t === item.replace(/([^›]*› )?(.*)/, (_, sub, name) => `${sub ?? ''}✓ ${name}`));
  if (!ticked(await menu(this, ''))) {
    await this.app.do({ cmd: 'composer-right-click', choose: item });
  }
  assert.ok(ticked(await menu(this, '')), `"${item}" didn't tick`);
});
