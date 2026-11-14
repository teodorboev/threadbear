import assert from 'node:assert/strict';
import { Given, Then, When } from '@cucumber/cucumber';
import type { ThreadbearWorld } from '../support/world.ts';
import type { Dump, Row } from '../support/driver.ts';

// A thread an agent can name by its id: the demo threads' ids aren't the
// shape a reply's ids are (a UUID, or its first 8 characters).
Given('there\'s a thread called {string} with the id {string}', async function (this: ThreadbearWorld, title: string, id: string) {
  await this.app.do({ cmd: 'mock', method: 'thread', params: { id, title } });
  await this.until(`"${title}" in the sidebar`, (d) => this.thread(d, title));
});

/** The streamed reply's row, once its turn has finished. */
const streamed = (d: Dump): Row | undefined => {
  const done = d.transcript.rows.some((r) => r.kind === 'footer' && r.turn === 'turn_stream');
  return done ? [...d.transcript.rows].reverse().find((r) => r.kind === 'assistant' && r.turn === 'turn_stream') : undefined;
};

When('a reply naming the thread {word} streams in', async function (this: ThreadbearWorld, word: string) {
  const d = await this.app.dump();
  const text = `The release notes are being checked in ${word}. Its plan is in \`${word}\` too.`;
  await this.app.do({ cmd: 'mock', method: 'stream', params: { threadId: d.selection?.id, text, hz: 60 } });
  await this.until('the streamed reply, its turn finished', streamed, 20_000);
});

Then('the reply shows the thread chip {string}', async function (this: ThreadbearWorld, word: string) {
  await this.until(`a reply with the chip "${word}"`, (d) => streamed(d)?.chips?.includes(`thread: ${word}`));
});

/** The newest row with a thread chip with these words. */
async function rowWithChip(world: ThreadbearWorld, word: string): Promise<Row> {
  return world.until(`a row with the chip "${word}"`, (d) => [...d.transcript.rows].reverse().find((r) => r.chips?.includes(`thread: ${word}`)));
}

/** `chip` on the row with that chip. A message just sent is shown while
 * it's on its way, then swapped for the runner's own row, so a row found a
 * moment ago may have gone: it's looked for again. */
async function onChip(world: ThreadbearWorld, word: string, command: Record<string, unknown>): Promise<string> {
  for (let attempt = 0; ; attempt += 1) {
    const row = await rowWithChip(world, word);
    try {
      return await world.app.do({ cmd: 'chip', row: row.id, word, ...command });
    } catch (error) {
      if (attempt >= 5 || !/no (such )?row/.test(String(error))) throw error;
      await new Promise((done) => setTimeout(done, 300));
    }
  }
}

Then('resting on the thread chip {string} shows its card', async function (this: ThreadbearWorld, word: string) {
  assert.match(await onChip(this, word, { show: true }), /ok window/);
  await onChip(this, word, { show: false });
});

When('I press the thread chip {string}', async function (this: ThreadbearWorld, word: string) {
  assert.match(await onChip(this, word, { click: true }), /ok opened/);
});

// What the runner writes when another thread's agent sends this one a
// message (send_message): its first line names that thread, by the first 8
// characters of its id as the runner writes it (threadbear-mcp.ts `from`).
// With the whole id here, the GPUI app's chip passed while every real one
// was drawn as plain words (13 Nov).
When('the agent in {string} sends this thread a message', async function (this: ThreadbearWorld, title: string) {
  const sender = await this.until(`a thread called "${title}"`, (d) => this.thread(d, title));
  await this.app.do({ cmd: 'send', text: `[Sent by the agent in the thread "${title}" (${sender.id.slice(0, 8)})]` });
});

Then('the message is headed by the thread chip {string}', async function (this: ThreadbearWorld, title: string) {
  await this.until(`a message headed by the chip "${title}"`, (d) => d.transcript.rows.some((r) => r.kind === 'user' && r.chips?.includes(`thread: ${title}`)));
});
