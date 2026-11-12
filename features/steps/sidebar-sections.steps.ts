import assert from 'node:assert/strict';
import { Given, Then, When } from '@cucumber/cucumber';
import type { Dump } from '../support/driver.ts';
import type { ThreadbearWorld } from '../support/world.ts';

const section = (d: Dump, name: string) => d.sidebar.sections.find((s) => s.name === name);

Then('the {string} section isn\'t folded', async function (this: ThreadbearWorld, name: string) {
  await this.until(`${name} open`, (d) => section(d, name)?.collapsed === false);
});

Then('the {string} heading says {string}', async function (this: ThreadbearWorld, name: string, count: string) {
  await this.until(`${name} heading saying ${count}`, (d) => String(section(d, name)?.count ?? '') === count);
});

Then('the {string} heading says nothing', async function (this: ThreadbearWorld, name: string) {
  await this.until(`${name} heading with no count`, (d) => section(d, name) && (section(d, name)?.count ?? null) === null);
});

// As a person would: the heading's button, whose tooltip says what it'll do.
When('I click the {string} heading', async function (this: ThreadbearWorld, name: string) {
  const d = await this.app.dump();
  await this.app.do({ cmd: 'click', tip: `${section(d, name)?.collapsed ? 'Show' : 'Hide'} ${name}` });
});

Given('there are {int} more archived threads', async function (this: ThreadbearWorld, count: number) {
  await this.app.do({ cmd: 'mock', method: 'fillArchived', params: { count } });
  await this.until(`${count} more archived threads`, (d) => (section(d, 'Archived')?.threads.length ?? 0) >= count);
});

Then('the Archived section draws {int} of its {int} threads', async function (this: ThreadbearWorld, shown: number, total: number) {
  await this.until(`${shown} of ${total} archived rows`, (d) => section(d, 'Archived')?.shown === shown && section(d, 'Archived')?.threads.length === total);
});

When('I ask for more of the Archived section', async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'archive-more' });
});

Given('the Inbox is sorted by what needs me first', async function (this: ThreadbearWorld) {
  await this.until('the default sort', (d) => d.sidebar.sort === 'needsYou');
});

When('I sort the Inbox oldest first', async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'sort', order: 'oldest' });
});

Then('the Inbox is sorted oldest first', async function (this: ThreadbearWorld) {
  await this.until('the oldest-first sort', (d) => d.sidebar.sort === 'oldest');
});

// The sort button's menu through a real click on it ("Sort: Needs you
// first" is its tip), its lines as they read, "[x] " before a ticked one.
When('I open the Inbox\'s sort menu', async function (this: ThreadbearWorld) {
  const said = await this.app.do({ cmd: 'realMenu', tipPrefix: 'Sort: ', ticks: true });
  const at = said.indexOf('[');
  assert.ok(at >= 0, `no menu in: ${said}`);
  this.menuTitles = (JSON.parse(said.slice(at)) as string[]).filter((line) => line !== '---');
});

Then('the sort menu ticks {string} and nothing else', function (this: ThreadbearWorld, title: string) {
  const sortMenu = this.menuTitles ?? [];
  assert.deepEqual(sortMenu.filter((line) => line.startsWith('[x] ')), [`[x] ${title}`], `the menu read: ${sortMenu.join(' | ')}`);
  assert.equal(sortMenu.length, 3, `the menu read: ${sortMenu.join(' | ')}`);
});
