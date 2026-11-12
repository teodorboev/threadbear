import { Given, Then, When } from '@cucumber/cucumber';
import assert from 'node:assert/strict';
import { sleep } from '../support/driver.ts';
import type { ThreadbearWorld } from '../support/world.ts';

// A chip's menu as text lines: `composer-menu` gives the same list on both
// apps, the Swift one after "ok ".
async function menuLines(world: ThreadbearWorld, menu: string): Promise<string[]> {
  const out = await world.app.do({ cmd: 'composer-menu', menu });
  return JSON.parse(out.slice(out.indexOf('['))) as string[];
}

Then('the {string} menu shows {string} ticked', async function (this: ThreadbearWorld, menu: string, title: string) {
  let lines: string[] = [];
  for (let tries = 0; tries < 40; tries += 1) {
    lines = await menuLines(this, menu);
    if (lines.find((l) => l.includes(title))?.startsWith('[x] ')) return;
    await sleep(150);
  }
  assert.fail(`the ${menu} menu has no ticked "${title}":\n${lines.join('\n')}`);
});

Then('the {string} menu has a row {string}', async function (this: ThreadbearWorld, menu: string, row: string) {
  const lines = await menuLines(this, menu);
  assert.ok(lines.some((l) => l.startsWith(row)), `no row "${row}" in the ${menu} menu:\n${lines.join('\n')}`);
});

// Once a pick has landed, which takes a moment.
Then('the {string} menu has no row {string}', async function (this: ThreadbearWorld, menu: string, row: string) {
  let lines: string[] = [];
  for (let tries = 0; tries < 40; tries += 1) {
    lines = await menuLines(this, menu);
    if (!lines.some((l) => l.startsWith(row))) return;
    await sleep(150);
  }
  assert.fail(`the ${menu} menu still has "${row}":\n${lines.join('\n')}`);
});

When('I pick {string} from the {string} menu', async function (this: ThreadbearWorld, title: string, menu: string) {
  await this.app.do({ cmd: 'composer-menu', menu, click: title });
});

Then('the composer\'s permission mode is {string}', async function (this: ThreadbearWorld, mode: string) {
  await this.until(`permission mode ${mode}`, (d) => (d.composer as { mode?: string }).mode === mode);
});

Then('the model chip says {string}', async function (this: ThreadbearWorld, chip: string) {
  await this.until(`the model chip saying "${chip}"`, (d) => (d.composer as { chip?: string }).chip === chip);
});

Then('the {string} menu says {string} and {string}', async function (this: ThreadbearWorld, menu: string, first: string, second: string) {
  const lines = await menuLines(this, menu);
  assert.deepEqual(lines.slice(0, 2), [first, second]);
});

// Send's right-click menu, through a real right-click on it in both apps.
async function sendMenu(world: ThreadbearWorld): Promise<string[]> {
  const out = await world.app.do({ cmd: 'composer-menu', menu: 'send' });
  return JSON.parse(out.slice(out.indexOf('['))) as string[];
}

Then('right-clicking Send offers {string}', async function (this: ThreadbearWorld, items: string) {
  assert.deepEqual(await sendMenu(this), items.split(' | '));
});

// With nothing of its own, the right-click is the panel's (Hide Title and
// the like), in both apps.
Then('right-clicking Send doesn\'t offer {string}', async function (this: ThreadbearWorld, item: string) {
  const items = await sendMenu(this);
  assert.ok(!items.includes(item), `it offers ${item}: ${items.join(', ')}`);
});

When('I right-click Send and choose {string}', async function (this: ThreadbearWorld, item: string) {
  await this.app.do({ cmd: 'composer-menu', menu: 'send', click: item });
});

Then('the schedule editor is open on a new schedule that sends {string}', async function (this: ThreadbearWorld, text: string) {
  type Editor = { isNew: boolean; text: string };
  const editor = await this.until('the schedule editor', (d) => (d as unknown as { scheduleEditor?: Editor }).scheduleEditor);
  assert.equal(editor.isNew, true, 'it opened on a schedule that exists');
  assert.equal(editor.text, text);
});

When('I open the context ring\'s popover', async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'composer-menu', menu: 'context', show: true });
});

// The breakdown comes after the runner answers, a moment after the popover
// opens, so this waits for the line.
Then('the context popover shows {string}', async function (this: ThreadbearWorld, line: string) {
  let lines: string[] = [];
  for (let tries = 0; tries < 40; tries += 1) {
    lines = await menuLines(this, 'context');
    if (lines.includes(line)) return;
    await sleep(150);
  }
  assert.fail(`the context popover has no "${line}":\n${lines.join('\n')}`);
});

// What a click on a picture in the box opened: `ok quicklook <index> of
// [paths]` from both apps' `composer-peek` (the Swift one's Quick Look is
// swapped for a note of what it would show; the GPUI one's viewer is real).
function opened(out: string, number: number, total: number): void {
  const found = /\bok quicklook (\d+) of (\[.*\])$/.exec(out);
  assert.ok(found, `the click said "${out}"`);
  assert.equal(Number(found[1]) + 1, number, `it opened picture ${Number(found[1]) + 1}, not ${number}`);
  assert.equal((JSON.parse(found[2]) as string[]).length, total, `it steps through ${found[2]}`);
}

Then('clicking chip {int} opens picture {int} of {int}', async function (this: ThreadbearWorld, chip: number, number: number, total: number) {
  opened(await this.app.do({ cmd: 'composer-peek', number: chip, press: true }), number, total);
});

Then('clicking the thumbnail of picture {int} opens picture {int} of {int}', async function (this: ThreadbearWorld, picture: number, number: number, total: number) {
  opened(await this.app.do({ cmd: 'composer-peek', strip: picture - 1 }), number, total);
});

// threads.update held a few seconds by the mock (mock.slow holds it with
// threads.send and threads.create).
Given('the runner takes its time to change threads', async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'mock', method: 'slow', params: { ms: 2500 } });
});

// One of the "+" menu's folders that isn't the thread's ("[ ] ~/dev/x").
When('I pick another folder from the {string} menu', async function (this: ThreadbearWorld & { folder?: string }, _menu: string) {
  const lines = await menuLines(this, 'plus');
  const other = lines.find((l) => l.startsWith('[ ] '));
  assert.ok(other, `no other folder in: ${lines.join(' | ')}`);
  this.folder = other.slice(4);
  await this.app.do({ cmd: 'composer-menu', menu: 'plus', click: this.folder });
});

Then('the {string} menu has that folder ticked', async function (this: ThreadbearWorld & { folder?: string }, _menu: string) {
  let lines: string[] = [];
  for (let tries = 0; tries < 40; tries += 1) {
    lines = await menuLines(this, 'plus');
    if (lines.includes(`[x] ${this.folder}`)) return;
    await sleep(150);
  }
  assert.fail(`${this.folder} isn't ticked: ${lines.join(' | ')}`);
});

type WithBusy = { busy?: string[] };
Then('the composer\'s {string} chip is spinning', async function (this: ThreadbearWorld, chip: string) {
  await this.until(`the ${chip} chip spinning`, (d) => ((d.composer as WithBusy).busy ?? []).includes(chip));
});

Then('soon no chip in the composer is spinning', async function (this: ThreadbearWorld) {
  await this.until('no chip spinning', (d) => ((d.composer as WithBusy).busy ?? []).length === 0, 10_000);
});

When('the window is {int} points wide', async function (this: ThreadbearWorld, width: number) {
  await this.app.do({ cmd: 'resize', w: width, h: 800 });
  // A frame or two for the box to follow the window.
  await sleep(400);
});

Then('the chips under the box fit inside it', async function (this: ThreadbearWorld) {
  await this.until('the chips inside the box', (d) => (d.composer as { chipsFit?: boolean }).chipsFit === true);
});
