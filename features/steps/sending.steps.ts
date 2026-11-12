import { Given, Then, When } from '@cucumber/cucumber';
import assert from 'node:assert/strict';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import type { ThreadbearWorld } from '../support/world.ts';

// A tiny real picture, for the steps that need a file (the runner reads it).
const samplePicture = path.join(path.dirname(fileURLToPath(import.meta.url)), '..', 'support', 'sample.png');

When('I type {string} in the composer', async function (this: ThreadbearWorld, text: string) {
  await this.app.do({ cmd: 'type', text });
});

When('I press Return', async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'return' });
});

When('I press Shift-Return', async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'return', shift: true });
});

When('I send {string}', async function (this: ThreadbearWorld, text: string) {
  await this.app.do({ cmd: 'send', text });
});

// `menu-key` is the Mac app's Stop item and the GPUI app's Stop binding.
When('I stop the turn', async function (this: ThreadbearWorld) {
  await this.until('the thread running', (d) => d.selection?.status === 'running');
  await this.app.do({ cmd: 'menu-key', key: '.', modifiers: ['cmd'] });
});

Then('the composer is empty', async function (this: ThreadbearWorld) {
  await this.until('an empty composer', (d) => d.composer.text === '');
});

Then('the composer says {string}', async function (this: ThreadbearWorld, text: string) {
  await this.until(`the composer saying "${text}"`, (d) => d.composer.text === text);
});

When('I paste {string} into the composer', async function (this: ThreadbearWorld, words: string) {
  await this.app.do({ cmd: 'paste', text: words.replace(/\\n/g, '\n') });
});

Then('the composer says the lines {string}', async function (this: ThreadbearWorld, lines: string) {
  const wanted = lines.replace(/\\n/g, '\n');
  await this.until(`the composer saying ${JSON.stringify(wanted)}`, (d) => d.composer.text === wanted);
});

Then('the composer holds a new line after {string}', async function (this: ThreadbearWorld, text: string) {
  await this.until(`"${text}" and a new line`, (d) => d.composer.text === `${text}\n`);
});

Then('the reply streams in and the thread goes back to idle', async function (this: ThreadbearWorld) {
  await this.until('the mock\'s reply', (d) => d.transcript.rows.some((r) => r.kind === 'assistant' && r.text.startsWith('You said:')), 30_000);
  await this.until('the thread idle again', (d) => d.selection?.status === 'idle', 30_000);
});

Then('the thread isn\'t running any more', async function (this: ThreadbearWorld) {
  await this.until('the thread stopped', (d) => d.selection?.status !== 'running', 30_000);
});

When('I paste the picture into the composer', async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'paste-image', path: samplePicture });
});

// A point in the thread's transcript on both apps' windows.
When('I drop the picture on the transcript', async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'drop', path: samplePicture, x: 650, y: 300 });
});

Then('the composer holds {int} picture(s)', async function (this: ThreadbearWorld, count: number) {
  await this.until(`${count} picture(s) in the composer`, (d) => d.composer.images === count);
});

Then('hovering chip {int} shows its picture', async function (this: ThreadbearWorld, number: number) {
  const result = await this.app.do({ cmd: 'composer-peek', number });
  assert.match(result, /ok/);
});

// Longer than the half second after the last keystroke that a draft is
// written in (drafts.rs, DraftStore).
When('I wait a moment', async function (this: ThreadbearWorld) {
  await new Promise((done) => setTimeout(done, 1200));
});

Given('the runner holds each message {int} seconds before its turn starts', async function (this: ThreadbearWorld, seconds: number) {
  await this.app.do({ cmd: 'mock', method: 'slow', params: { queueMs: seconds * 1000 } });
});

Then("the transcript's last row reads {string}", async function (this: ThreadbearWorld, text: string) {
  await this.until(`the last row reading "${text}"`, (d) => d.transcript.rows.at(-1)?.text === text);
});

// The whole box copied or cut, and pasted back, as ⌘A then ⌘C or ⌘X, and ⌘V
// (both apps' `composer-clip`).
When('I cut everything in the composer', async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'composer-clip', action: 'cut' });
});

When('I copy everything in the composer', async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'composer-clip', action: 'copy' });
});

When(/^I paste back what I (?:cut|copied)$/, async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'composer-clip', action: 'paste' });
});

When('I clear the composer', async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'type', text: '', replace: true });
  await this.until('an empty composer', (d) => d.composer.text === '');
});

// What "Copy thread id" gives: the bare id.
When('I paste the id of the thread {string} into the composer', async function (this: ThreadbearWorld, title: string) {
  const t = await this.until(`a thread called "${title}"`, (d) => this.thread(d, title));
  await this.app.do({ cmd: 'paste', text: t.id });
});

// The chips in the box, in order (`composer-chips` on both apps).
async function composerChips(world: ThreadbearWorld): Promise<string[]> {
  const out = await world.app.do({ cmd: 'composer-chips' });
  return JSON.parse(out.slice(out.indexOf('['))) as string[];
}

Then("the composer's chips are {string}", async function (this: ThreadbearWorld, list: string) {
  const wanted = list.split(' | ');
  let chips: string[] = [];
  for (let tries = 0; tries < 40; tries += 1) {
    chips = await composerChips(this);
    if (JSON.stringify(chips) === JSON.stringify(wanted)) return;
    await new Promise((done) => setTimeout(done, 150));
  }
  assert.deepEqual(chips, wanted);
});

Then('the composer has no chips', async function (this: ThreadbearWorld) {
  assert.deepEqual(await composerChips(this), []);
});

// The draft holds the link the chip stands for, which is what's sent.
Then('the composer holds the link to the thread {string}', async function (this: ThreadbearWorld, title: string) {
  const t = await this.until(`a thread called "${title}"`, (d) => this.thread(d, title));
  await this.until(`the link to "${title}" in the composer`, (d) => d.composer.text === `[${title}](thread:${t.id})`);
});
