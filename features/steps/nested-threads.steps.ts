import assert from 'node:assert/strict';
import { Then, When } from '@cucumber/cucumber';
import type { Dump, Thread } from '../support/driver.ts';
import type { ThreadbearWorld } from '../support/world.ts';

// Threads nested under the thread that started them (the nestedThreads
// experiment). A starter's row in `dump` has `group`: its count's words,
// whether it's open, and its threads while open.
const head = (d: Dump, title: string): Thread | undefined => d.sidebar.sections.flatMap((s) => s.threads).find((t) => t.title === title);
const nestedIn = (d: Dump, starter: string, title: string) => head(d, starter)?.group?.threads.find((t) => t.title === title);

Then('the group under {string} says {string}', async function (this: ThreadbearWorld, starter: string, words: string) {
  await this.until(`"${words}" on "${starter}"`, (d) => head(d, starter)?.group?.words === words);
});

Then('the group under {string} is open', async function (this: ThreadbearWorld, starter: string) {
  await this.until(`the group under "${starter}" open`, (d) => head(d, starter)?.group?.open === true);
});

Then('the group under {string} lists no threads', async function (this: ThreadbearWorld, starter: string) {
  await this.until(`the group under "${starter}" folded`, (d) => head(d, starter)?.group?.open === false && head(d, starter)?.group?.threads.length === 0);
});

Then('the group under {string} lists {string} asking {string}', async function (this: ThreadbearWorld, starter: string, title: string, question: string) {
  await this.until(`"${title}" under "${starter}" asking`, (d) => {
    const t = nestedIn(d, starter, title);
    return t?.question === true && t.line === question;
  });
});

Then('the group under {string} lists {string} saying {string}', async function (this: ThreadbearWorld, starter: string, title: string, line: string) {
  await this.until(`"${title}" under "${starter}" saying "${line}"`, (d) => {
    const t = nestedIn(d, starter, title);
    return t?.question === false && t.line === line;
  });
});

Then('the group under {string} has {int} finished', async function (this: ThreadbearWorld, starter: string, count: number) {
  await this.until(`${count} finished under "${starter}"`, (d) => head(d, starter)?.group?.finished === count);
});

Then('the row {string} heads no group', async function (this: ThreadbearWorld, starter: string) {
  const d = await this.app.dump();
  assert.ok(head(d, starter), `no row "${starter}"`);
  assert.equal(head(d, starter)?.group ?? null, null);
});

When('I fold the group under {string}', async function (this: ThreadbearWorld, starter: string) {
  const t = await this.until(`"${starter}" heading a group`, (d) => (head(d, starter)?.group ? head(d, starter) : undefined));
  await this.app.do({ cmd: 'group', id: t.id, open: false });
});

When('I open the group under {string}', async function (this: ThreadbearWorld, starter: string) {
  const t = await this.until(`"${starter}" heading a group`, (d) => (head(d, starter)?.group ? head(d, starter) : undefined));
  await this.app.do({ cmd: 'group', id: t.id, open: true });
});

// The row's real right-click menu, left open, then the line picked from it.
// Opened first, so the list brings its row into view for the click.
When('I choose {string} on the row {string}', async function (this: ThreadbearWorld, item: string, title: string) {
  const t = await this.until(`a thread called "${title}"`, (d) => this.thread(d, title));
  await this.app.do({ cmd: 'open', id: t.id });
  await this.until(`"${title}" in view`, (d) => this.thread(d, title)?.inView === true);
  await this.app.do({ cmd: 'realMenu', tipPrefix: `${title}\n`, right: true, keep: true }, 20);
  await this.app.do({ cmd: 'menu-popover', click: item });
});

Then('the open thread says {string}', async function (this: ThreadbearWorld, text: string) {
  await this.until(`"${text}" above the transcript`, (d) => d.selection?.startedBy?.text === text);
});
