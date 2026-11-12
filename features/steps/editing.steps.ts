// Editing one of Mike's messages (features/editing.feature).
import { Given, Then, When } from '@cucumber/cucumber';
import assert from 'node:assert/strict';
import { sleep } from '../support/driver.ts';
import type { ThreadbearWorld } from '../support/world.ts';

const edited = new WeakMap<ThreadbearWorld, { seq: number; text: string }>();

When('I edit my first message', async function (this: ThreadbearWorld) {
  const d = await this.until('a message of mine', (d) => d.transcript.rows.find((r) => r.kind === 'user' && r.id.startsWith('u:')));
  const out = await this.app.do({ cmd: 'edit-message', row: d.id });
  assert.match(out, /ok editing/);
  edited.set(this, { seq: d.seq ?? 0, text: d.text });
});

Then('the composer holds my first message to edit', async function (this: ThreadbearWorld) {
  const want = edited.get(this)!;
  await this.until('the message in the box, being edited', (d) => d.composer.editing === want.seq && d.composer.text.trim().length > 0 && want.text.startsWith(d.composer.text.trim().slice(0, 20)));
});

Then("the composer isn't editing", async function (this: ThreadbearWorld) {
  await this.until('no edit under way', (d) => d.composer.editing == null, 20_000);
});

When("I replace what's in the composer with {string}", async function (this: ThreadbearWorld, text: string) {
  await this.app.do({ cmd: 'type', text, replace: true });
});

Then('the transcript has my message {string}', async function (this: ThreadbearWorld, text: string) {
  await this.until(`my message "${text}"`, (d) => d.transcript.rows.some((r) => r.kind === 'user' && r.text.startsWith(text)), 30_000);
});

Then('the composer is still editing my first message', async function (this: ThreadbearWorld) {
  const want = edited.get(this)!;
  await this.until('the edit still under way', (d) => d.composer.editing === want.seq);
});

// Long enough for an allow to have reached the mock runner and the card to
// have gone, as it does in a second after a click on Allow.
Then('the approval for {string} is still waiting', async function (this: ThreadbearWorld, command: string) {
  await sleep(2_000);
  const d = await this.app.dump();
  // An answered one stays a row for a moment, its text then ending in
  // "(allow)" or "(deny)", before the work it let run folds it away.
  const row = d.transcript.rows.find((r) => r.kind === 'approval' && r.text.includes(command));
  assert.ok(row, `the approval for "${command}" was answered and has gone`);
  assert.doesNotMatch(row.text, /\((allow|deny|answered)\)\s*$/, `the approval for "${command}" was answered`);
});

// mock.cardAnswer: a daily brief's shape (thread 3fdaaeec, 12 Nov). A card,
// Mike's answer to it with his words under the pick, then a message another
// thread's agent sent in during that answer's turn, and the reply.
Given('a thread where I answered a card and another thread\'s agent wrote in after, called {string}', async function (this: ThreadbearWorld, title: string) {
  const id = 'thread_card_answer';
  await this.app.do({ cmd: 'mock', method: 'thread', params: { id, title } });
  await this.app.do({ cmd: 'mock', method: 'cardAnswer', params: { threadId: id } });
  await this.until(`"${title}" in the sidebar`, (d) => this.thread(d, title));
});

// A real click on the pencil, through the window, the row hovered first as
// the pencil only shows on hover: the pencil by the answer was drawn and did
// nothing when pressed.
When('I click the pencil by my message {string}', async function (this: ThreadbearWorld, words: string) {
  const d = await this.until(`my message "${words}"`, (d) => d.transcript.rows.find((r) => r.kind === 'user' && r.id.startsWith('u:') && r.text.includes(words)));
  const out = await this.app.do({ cmd: 'edit-message', row: d.id, click: true });
  assert.match(out, /ok editing/);
  edited.set(this, { seq: d.seq ?? 0, text: d.text });
});

Then('the composer holds that message to edit, with {string} in it', async function (this: ThreadbearWorld, words: string) {
  const want = edited.get(this)!;
  await this.until(`the message in the box, being edited`, (d) => d.composer.editing === want.seq && d.composer.text.includes(words));
});

// What the bubble draws, not what the message holds: the GPUI bubble showed
// the question and the pick and dropped the words Mike wrote under them.
Then('my answer to {string} shows {string} and {string}', async function (this: ThreadbearWorld, question: string, pick: string, words: string) {
  const d = await this.until(`my answer to "${question}"`, (d) => d.transcript.rows.find((r) => r.kind === 'user' && r.id.startsWith('u:') && r.text.includes(question)));
  const drawn = await this.app.do({ cmd: 'message-words', row: d.id });
  for (const want of [question, pick, words]) assert.ok(drawn.includes(want), `the bubble shows ${drawn}, without "${want}"`);
});
