import assert from 'node:assert/strict';
import { Then, When } from '@cucumber/cucumber';
import type { ThreadbearWorld } from '../support/world.ts';
import type { Dump, Row } from '../support/driver.ts';
import { previewLine } from '../support/pane.ts';

// Words selected in a reply (selected-words.feature): the bar over them, and
// a copy that keeps their links. Both apps take `select-text`, which selects
// a message's words (all of a short reply's, here) and shows the bar, or
// presses one of its buttons.

/** The streamed reply's row, once its turn has finished. */
const streamed = (d: Dump): Row | undefined => {
  const done = d.transcript.rows.some((r) => r.kind === 'footer' && r.turn === 'turn_stream');
  return done ? [...d.transcript.rows].reverse().find((r) => r.kind === 'assistant' && r.turn === 'turn_stream') : undefined;
};

When('a reply saying {string} streams in', async function (this: ThreadbearWorld, text: string) {
  const d = await this.app.dump();
  await this.app.do({ cmd: 'mock', method: 'stream', params: { threadId: d.selection?.id, text, hz: 60 } });
  await this.until('the streamed reply, its turn finished', streamed, 20_000);
});

async function select(world: ThreadbearWorld, words: string, action?: string): Promise<string> {
  const row = await world.until('the streamed reply', streamed);
  return world.app.do({ cmd: 'select-text', row: row.id, words, ...(action ? { action } : {}) });
}

When('I select {string} in the reply', async function (this: ThreadbearWorld, words: string) {
  this.lastAnswer = await select(this, words);
});

Then('the bar shows over the words', function (this: ThreadbearWorld) {
  assert.match(this.lastAnswer ?? '', /ok window/);
});

When('I press {string} on the bar over {string}', async function (this: ThreadbearWorld, action: string, words: string) {
  await select(this, words, action);
});

When('I press ⌘C', async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'menu-key', key: 'c', modifiers: ['cmd'] });
});

Then('the composer starts with a quote of {string}', async function (this: ThreadbearWorld, words: string) {
  await this.until(`"> ${words}" in the composer`, (d) => d.composer.text.startsWith(`> ${words}`));
});

Then('the pane explains {string} with {string}', async function (this: ThreadbearWorld, words: string, answer: string) {
  // The Swift dump's line: "explain “words” | the answer" once it's in.
  await this.until(`an explanation of "${words}"`, (d) => previewLine(d)?.startsWith(`explain “${words}”`) && previewLine(d)?.includes(answer), 20_000);
});
