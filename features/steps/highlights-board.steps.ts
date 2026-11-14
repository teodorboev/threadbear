import { Then, When } from '@cucumber/cucumber';
import assert from 'node:assert/strict';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { sleep } from '../support/driver.ts';
import type { ThreadbearWorld } from '../support/world.ts';
import { checkBoardGestures } from '../tools/board-gestures.ts';
import type { GpuiDriver } from '../support/gpui.ts';

async function point(world: ThreadbearWorld, label: string, scroll = true) {
  let last: unknown;
  for (let n = 0; n < 20; n += 1) {
    try { return (await world.app.do({ cmd: 'label-point', label })).trim().split(/\s+/).slice(-2).map(Number); }
    catch (e) { last = e; }
    if (scroll && n % 3 === 2) {
      try { await world.app.do({ cmd: 'click', tip: 'Next board column' }); } catch {}
    }
    await sleep(120);
  }
  throw last;
}

When('I prepare a board question with a picture', async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'type', text: 'Please check ', replace: true });
  const sample = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../support/sample.png');
  await this.app.do({ cmd: 'paste-image', path: sample });
  await this.until('the draft picture', d => d.composer.images === 1);
});

When('I press the board card {string}', async function (this: ThreadbearWorld, title: string) {
  const [x, y] = await point(this, `Board card: ${title}`);
  await this.app.do({ cmd: 'click-at', x, y });
});

Then('the board card {string} is mentioned once after my words', async function (this: ThreadbearWorld, title: string) {
  const reference = `[${title}](https://www.notion.so/t3)`;
  await this.until('one mention after the draft', d => d.composer.text.startsWith('Please check ') && d.composer.text.split(reference).length === 2 && d.composer.text.endsWith(`${reference} `));
});

Then('the board question still has its picture', async function (this: ThreadbearWorld) {
  assert.equal((await this.app.dump()).composer.images, 1);
});

Then('the board card {string} is hidden', async function (this: ThreadbearWorld, title: string) {
  await assert.rejects(this.app.do({ cmd: 'label-point', label: `Board card: ${title}` }));
});

Then('the board card {string} can be seen', async function (this: ThreadbearWorld, title: string) {
  await point(this, `Board card: ${title}`);
});

When("I unfold the board's Done column", async function (this: ThreadbearWorld) {
  await point(this, 'Show 4 in Done');
  await this.app.do({ cmd: 'click', tip: 'Show 4 in Done' });
});

When("I fold the board's Done column", async function (this: ThreadbearWorld) {
  await point(this, 'Fold Done');
  await this.app.do({ cmd: 'click', tip: 'Fold Done' });
});

When('I open the working thread from the board card {string}', async function (this: ThreadbearWorld, title: string) {
  const [x, y] = await point(this, `Board card: ${title}`);
  await this.app.do({ cmd: 'click-at', x, y, right: true });
  const [mx, my] = await point(this, 'Open the Thread Working on It', false);
  await this.app.do({ cmd: 'click-at', x: mx, y: my });
});

Then('the board has opened the thread {string}', async function (this: ThreadbearWorld, id: string) {
  await this.until(`thread ${id}`, d => d.selection?.id === id);
});

When('the board gains five more cards', async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'mock', method: 'boardMore', params: { threadId: 'demo_button_colour' } });
  await point(this, '3 more');
});

When("I show all cards in the board's To do column", async function (this: ThreadbearWorld) {
  const [x, y] = await point(this, '3 more');
  await this.app.do({ cmd: 'click-at', x, y });
});

When("I show fewer cards in the board's To do column", async function (this: ThreadbearWorld) {
  const [x, y] = await point(this, 'Show fewer');
  await this.app.do({ cmd: 'click-at', x, y });
});

When('I check the board with real two-finger gestures', async function (this: ThreadbearWorld) {
  await checkBoardGestures(this.app as GpuiDriver);
});
