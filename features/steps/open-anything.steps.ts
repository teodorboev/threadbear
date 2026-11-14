import { Then, When } from '@cucumber/cucumber';
import type { ThreadbearWorld } from '../support/world.ts';

// ⌘K's box (desktop/gpui/src/open_anything.rs). The dump's `anything` has
// each group's entries as their lines read: the title, a panel's count, then
// " | " and what's on the right ("Inbox | ⌘1").

Then('the Open Anything box is up', async function (this: ThreadbearWorld) {
  await this.until('the Open Anything box up', (d) => !!d.anything);
});

Then('the Open Anything box is down', async function (this: ThreadbearWorld) {
  await this.until('the Open Anything box down', (d) => !d.anything);
});

When('I type {string} in the Open Anything box', async function (this: ThreadbearWorld, text: string) {
  await this.app.do({ cmd: 'anything', text });
});

When('I press Return in the Open Anything box', async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'return' });
});

When('I press ⌘Return in the Open Anything box', async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'menu-key', key: 'enter', modifiers: ['cmd'] });
});

Then('the box lists {string} under {string}', async function (this: ThreadbearWorld, line: string, group: string) {
  await this.until(`"${line}" under ${group}`, (d) => d.anything?.groups.find((g) => g.title === group)?.items.includes(line));
});

Then("the box's pick is {string}", async function (this: ThreadbearWorld, title: string) {
  await this.until(`${title} picked in the box`, (d) => d.anything?.picked === title);
});

Then("the box's first thread is {string}, archived", async function (this: ThreadbearWorld, title: string) {
  await this.until(`${title} first under Threads, archived`, (d) => d.anything?.groups.find((g) => g.title === 'Threads')?.items[0]?.startsWith(`${title} | archived · `));
});

Then('the box lists the thread {string}', async function (this: ThreadbearWorld, title: string) {
  await this.until(`"${title}" under Threads`, (d) => d.anything?.groups.find((g) => g.title === 'Threads')?.items.some((i) => i === title || i.startsWith(`${title} | `)));
});

// A key that would open it, given a moment to have done so.
Then('the Open Anything box stays down', async function (this: ThreadbearWorld) {
  await new Promise((r) => setTimeout(r, 1000));
  await this.until('the Open Anything box down', (d) => !d.anything);
});
