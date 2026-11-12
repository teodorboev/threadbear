import { Then, When } from '@cucumber/cucumber';
import assert from 'node:assert/strict';
import type { ThreadbearWorld } from '../support/world.ts';

// The palette's commands come with the first `threads.commands` answer, so
// everything here waits for what it reads.
const split = (list: string) => list.split(' | ').map((s) => s.trim());

When('I type {string} in the composer, replacing what\'s there', async function (this: ThreadbearWorld, text: string) {
  await this.app.do({ cmd: 'type', text, replace: true });
});

When('I press the down arrow', async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'arrow', dir: 'down' });
});

When('I press the up arrow', async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'arrow', dir: 'up' });
});

When('I press Tab', async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'tab' });
});

When('I press Escape', async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'escape' });
});

// The tooltip each category's button has (SlashPaletteView.categoryTip).
const categoryTips: Record<string, string> = {
  All: 'Every command this thread can run',
  Threadbear: "Threadbear's own, which change this thread's settings",
  'Claude Code': "Claude Code's own commands",
  Skills: 'Your skills and this folder\'s',
  Recent: 'The ones you picked lately',
};

When('I click the palette\'s {string} category', async function (this: ThreadbearWorld, category: string) {
  const tip = categoryTips[category];
  assert.ok(tip, `no category called ${category}`);
  await this.until('the palette open', (d) => d.palette);
  await this.app.do({ cmd: 'click', tip });
});

Then('the palette is open on {string}', async function (this: ThreadbearWorld, category: string) {
  await this.until(`the palette open on ${category}`, (d) => d.palette?.category === category);
});

Then('the palette is closed', async function (this: ThreadbearWorld) {
  await this.until('the palette closed', (d) => !d.palette);
});

Then('the palette\'s categories say {string}', async function (this: ThreadbearWorld, list: string) {
  const wanted = split(list);
  await this.until(`the categories ${list}`, (d) => {
    const shown = d.palette?.categories.map((c) => `${c.title} ${c.count}`) ?? [];
    return shown.length === wanted.length && shown.every((s, i) => s === wanted[i]);
  });
});

Then('the palette lists {string} first', async function (this: ThreadbearWorld, command: string) {
  await this.until(`the palette led by ${command}`, (d) => d.palette?.commands[0] === command);
});

Then('the palette has the command {string}', async function (this: ThreadbearWorld, command: string) {
  await this.until(`${command} in the palette`, (d) => d.palette?.commands.includes(command));
});

Then('the palette\'s commands are {string}', async function (this: ThreadbearWorld, list: string) {
  const wanted = split(list);
  await this.until(`the commands ${list}`, (d) => {
    const shown = d.palette?.commands ?? [];
    return shown.length === wanted.length && shown.every((s, i) => s === wanted[i]);
  });
});

Then('the palette\'s pick is {string}', async function (this: ThreadbearWorld, command: string) {
  await this.until(`${command} picked`, (d) => d.palette?.picked === command);
});

Then('the palette\'s details say the usage {string}', async function (this: ThreadbearWorld, usage: string) {
  await this.until(`the usage ${usage}`, (d) => d.palette?.detail?.usage === usage);
});

Then('the palette\'s details say they\'re from {string}', async function (this: ThreadbearWorld, origin: string) {
  await this.until(`from ${origin}`, (d) => d.palette?.detail?.origin === origin);
});

// The palette's size and columns as drawn, in points (the GPUI dump's
// `palette.size` and `palette.columns`), noted to compare after a drag.
type Geometry = { size: [number, number]; columns: [number, number] };
const noted = new WeakMap<ThreadbearWorld, Geometry>();

async function geometry(world: ThreadbearWorld): Promise<Geometry> {
  return world.until('the palette drawn', (d) => {
    const p = (d as unknown as { palette?: { size?: [number, number]; columns?: [number, number] } }).palette;
    return p?.size && p.columns ? { size: p.size, columns: p.columns } : undefined;
  });
}

When('I note the palette\'s size', async function (this: ThreadbearWorld) {
  noted.set(this, await geometry(this));
});

When('I drag the palette\'s top edge {int} points up', async function (this: ThreadbearWorld, points: number) {
  await this.app.do({ cmd: 'palette-drag', edge: 'top', dx: 0, dy: -points });
});

When('I drag the palette\'s right edge {int} points right', async function (this: ThreadbearWorld, points: number) {
  await this.app.do({ cmd: 'palette-drag', edge: 'right', dx: points, dy: 0 });
});

// A side grows both sides at once, so the palette stays centred on the box.
Then('the palette is {int} points wider than it was', async function (this: ThreadbearWorld, points: number) {
  const before = noted.get(this);
  assert.ok(before, 'note the palette\'s size first');
  await this.until(`the palette ${points} points wider`, (d) => {
    const size = (d as unknown as { palette?: { size?: [number, number] } }).palette?.size;
    return size && near(size[0], before.size[0] + points) && near(size[1], before.size[1]);
  });
});

When('I drag the list\'s edge {int} points right', async function (this: ThreadbearWorld, points: number) {
  await this.app.do({ cmd: 'palette-drag', edge: 'list', dx: points, dy: 0 });
});

When('I choose {string} from the palette\'s right-click menu', async function (this: ThreadbearWorld, item: string) {
  await this.app.do({ cmd: 'palette-menu', click: item });
});

// Within a point: the sizes are kept as dragged, which can be fractional.
const near = (a: number, b: number) => Math.abs(a - b) < 1.5;

Then(/^the palette is (?:still )?(\d+) points taller than it was$/, async function (this: ThreadbearWorld, points: string) {
  const before = noted.get(this);
  assert.ok(before, 'note the palette\'s size first');
  await this.until(`the palette ${points} points taller`, (d) => {
    const size = (d as unknown as { palette?: { size?: [number, number] } }).palette?.size;
    return size && near(size[1], before.size[1] + Number(points)) && near(size[0], before.size[0]);
  });
});

Then('the palette is the size it was', async function (this: ThreadbearWorld) {
  const before = noted.get(this);
  assert.ok(before, 'note the palette\'s size first');
  await this.until('the palette back to its size', (d) => {
    const size = (d as unknown as { palette?: { size?: [number, number] } }).palette?.size;
    return size && near(size[0], before.size[0]) && near(size[1], before.size[1]);
  });
});

Then('the list is {int} points wider than it was', async function (this: ThreadbearWorld, points: number) {
  const before = noted.get(this);
  assert.ok(before, 'note the palette\'s size first');
  await this.until(`the list ${points} points wider`, (d) => {
    const columns = (d as unknown as { palette?: { columns?: [number, number] } }).palette?.columns;
    return columns && near(columns[1], before.columns[1] + points);
  });
});
