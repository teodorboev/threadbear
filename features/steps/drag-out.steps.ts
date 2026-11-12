// Pictures and files dragged out with the mouse: the GPUI app's `file-drag`,
// through its own event dispatch.
import { When } from '@cucumber/cucumber';
import assert from 'node:assert/strict';
import { sleep } from '../support/driver.ts';
import { pane } from '../support/pane.ts';
import type { ThreadbearWorld } from '../support/world.ts';

// A point in the thread's transcript on both apps' windows, as the picture's drop.
const transcript = { x: 650, y: 300 };

When('I drag picture {int} of the row of {int} pictures onto the transcript', async function (this: ThreadbearWorld, number: number, count: number) {
  // On screen first: a drag starts from where the picture's drawn.
  const row = await this.until('a row of pictures', (d) => d.transcript.rows.filter((r) => r.kind === 'images').at(-1));
  await this.app.do({ cmd: 'scroll', row: row.id, offset: 0 });
  await this.until('the pictures on screen', (d) => d.transcript.rows.find((r) => r.id === row.id && r.onScreen !== false));
  await sleep(300);
  this.lastAnswer = await this.app.do({ cmd: 'file-drag', tile: number - 1, count, ...transcript });
  assert.match(this.lastAnswer, /dropped/);
});

When('I drag the highlight {string} onto the transcript', async function (this: ThreadbearWorld, title: string) {
  await this.until(`a highlight called "${title}"`, (d) => pane(d)?.sections?.some((s) => s.rows.some((r) => r.title === title)));
  this.lastAnswer = await this.app.do({ cmd: 'file-drag', highlight: title, ...transcript });
  assert.match(this.lastAnswer, /dropped/);
});

When('I drag the picture in the pane onto the transcript', async function (this: ThreadbearWorld) {
  this.lastAnswer = await this.app.do({ cmd: 'file-drag', pane: true, ...transcript });
  assert.match(this.lastAnswer, /dropped/);
});
