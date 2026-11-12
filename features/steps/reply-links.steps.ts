import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { Given, Then, When } from '@cucumber/cucumber';
import { tempFolder } from '../support/temp.ts';
import type { ThreadbearWorld } from '../support/world.ts';
import { okValue, type Dump, type Row } from '../support/driver.ts';
import { previewLine } from '../support/pane.ts';

// A reply's links (reply-links.feature): what a click, a ⌘-click and a
// right-click do, and the card that shows on a rest. Both apps take `peek`
// on a reply's link (its card, its menu, a click on it) and say what they'd
// have opened with `opened`, as a scripted app opens nothing for real.

type LinkWorld = ThreadbearWorld & { file?: string };

Given('a file {string} saying {string} in a folder of its own', function (this: LinkWorld, name: string, text: string) {
  // A folder of its own, gone after the scenario (support/temp.ts).
  const dir = tempFolder('tb-links-');
  this.file = path.join(dir, name);
  fs.writeFileSync(this.file, text.replaceAll('\\n', '\n'));
});

const file = (world: LinkWorld): string => {
  assert.ok(world.file, 'no file made for this scenario');
  return world.file;
};

/** The streamed reply's row, once its turn has finished. */
const streamed = (d: Dump): Row | undefined => {
  const done = d.transcript.rows.some((r) => r.kind === 'footer' && r.turn === 'turn_stream');
  return done ? [...d.transcript.rows].reverse().find((r) => r.kind === 'assistant' && r.turn === 'turn_stream') : undefined;
};

async function streamReply(world: ThreadbearWorld, text: string, read?: string): Promise<void> {
  const d = await world.app.dump();
  await world.app.do({ cmd: 'mock', method: 'stream', params: { threadId: d.selection?.id, text, hz: 60, ...(read ? { read } : {}) } });
  await world.until('the streamed reply, its turn finished', streamed, 20_000);
}

When('a reply linking {string} to {string} streams in', async function (this: LinkWorld, words: string, url: string) {
  await streamReply(this, `Here's [${words}](${url}) to look at.`);
});

When('a reply linking {string} to that file streams in', async function (this: LinkWorld, words: string) {
  await streamReply(this, `Here's [${words}](${file(this)}) to look at.`);
});

When('a reply linking {string} to {string} and {string} to that file streams in', async function (this: LinkWorld, page: string, url: string, words: string) {
  await streamReply(this, `See [${page}](${url}), and [${words}](${file(this)}) for the rest.`);
});

When('a reply that read that file and names it by itself in code streams in', async function (this: LinkWorld) {
  // Not in the thread's folder: found among the files its tools touched.
  await streamReply(this, `The notes are in \`${path.basename(file(this))}\`.`, file(this));
});

/** `peek` on the streamed reply's link with these words. */
async function peek(world: ThreadbearWorld, words: string, command: Record<string, unknown>): Promise<string> {
  const row = await world.until('the streamed reply', streamed);
  return world.app.do({ cmd: 'peek', row: row.id, words, ...command });
}

async function menuOffers(world: ThreadbearWorld, words: string, list: string) {
  const items = JSON.parse(okValue(await peek(world, words, { menu: true }))) as string[];
  for (const item of list.split(' | ')) assert.ok(items.includes(item), `no "${item}" in: ${items.join(' | ')}`);
}

Then('the menu of the link {string} offers {string}', async function (this: ThreadbearWorld, words: string, list: string) {
  await menuOffers(this, words, list);
});

// The Finder is the Mac's; Windows and Linux call theirs a folder.
Then('the menu of the link {string} offers {string} on a Mac, {string} elsewhere', async function (this: ThreadbearWorld, words: string, mac: string, elsewhere: string) {
  await menuOffers(this, words, process.platform === 'darwin' ? mac : elsewhere);
});

When('I choose {string} from the menu of the link {string}', async function (this: ThreadbearWorld, item: string, words: string) {
  await peek(this, words, { click: item });
});

When('I pick {string} under Default in the menu of the link {string}', async function (this: ThreadbearWorld, item: string, words: string) {
  await peek(this, words, { default: item });
});

When('I click the link {string}', async function (this: ThreadbearWorld, words: string) {
  await peek(this, words, { follow: true });
});

When('I ⌘-click the link {string}', async function (this: ThreadbearWorld, words: string) {
  await peek(this, words, { follow: true, command: true });
});

Then("the clipboard holds that file's path", async function (this: LinkWorld) {
  const want = file(this);
  const deadline = Date.now() + 5000;
  let last = '';
  while (Date.now() < deadline) {
    last = JSON.parse(okValue(await this.app.do({ cmd: 'clipboard' }))) as string;
    if (last === want) return;
    await new Promise((done) => setTimeout(done, 100));
  }
  assert.equal(last, want);
});

Then('the pane is showing that file', async function (this: LinkWorld) {
  const want = `file ${file(this).replaceAll('\\', '/')}`;
  await this.until(`the pane showing ${want}`, (d) => previewLine(d) === want);
});

Then("the pane isn't showing that file", async function (this: LinkWorld) {
  const d = await this.app.dump();
  assert.notEqual(previewLine(d), `file ${file(this).replaceAll('\\', '/')}`, 'the pane is showing it');
});

/** What the app would have handed the system to open, since last asked. */
async function opened(world: ThreadbearWorld): Promise<string[]> {
  return JSON.parse(okValue(await world.app.do({ cmd: 'opened' }))) as string[];
}

Then('that file was opened in its app', async function (this: LinkWorld) {
  const name = encodeURIComponent(path.basename(file(this)));
  const deadline = Date.now() + 5000;
  const seen: string[] = [];
  while (Date.now() < deadline) {
    seen.push(...(await opened(this)));
    if (seen.some((u) => u.startsWith('file://') && u.endsWith(`/${name}`))) return;
    await new Promise((done) => setTimeout(done, 100));
  }
  assert.fail(`it wasn't opened; what was: ${JSON.stringify(seen)}`);
});

Then('nothing was opened in another app', async function (this: ThreadbearWorld) {
  assert.deepEqual(await opened(this), []);
});

Then("the reply's links with a card are {string} to {string} and {string} to that file", async function (this: LinkWorld, page: string, url: string, words: string) {
  const row = await this.until('the streamed reply', streamed);
  const out = okValue(await this.app.do({ cmd: 'peek', row: row.id }));
  assert.equal(out, `${page}=${url} | ${words}=${file(this)}`);
});

Then('resting on the link {string} shows its card', async function (this: ThreadbearWorld, words: string) {
  assert.match(await peek(this, words, {}), /ok window/);
  await peek(this, words, { show: false });
});
