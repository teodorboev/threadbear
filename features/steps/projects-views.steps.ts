import assert from 'node:assert/strict';
import { Then, When } from '@cucumber/cucumber';
import { okValue, type Dump } from '../support/driver.ts';
import type { ThreadbearWorld } from '../support/world.ts';

// The Projects view, the page of cards and a project's page. The dump's
// `project` is the open page (its title, folders and threads by id), and
// `search` with `projects` answers with the cards the search leaves.
type WithPage = Dump & { project?: { title: string; folders?: string[]; threads?: string[] } | null };

const page = (d: Dump) => (d as WithPage).project;

When('I search the projects for {string}', async function (this: ThreadbearWorld, text: string) {
  this.lastAnswer = await this.app.do({ cmd: 'search', projects: true, text });
});

Then('the Projects page shows the cards {string}', function (this: ThreadbearWorld, list: string) {
  const shown = JSON.parse(okValue(this.lastAnswer ?? '')) as string[];
  assert.deepEqual(shown, list.split(', '));
});

Then('the sidebar\'s project {string} lists {string}', async function (this: ThreadbearWorld, title: string, thread: string) {
  await this.until(`"${thread}" under "${title}"`, (d) => d.sidebar.sections.some((s) => s.name === title && s.threads.some((t) => t.title === thread)));
});

Then('the project page {string} is open', async function (this: ThreadbearWorld, title: string) {
  await this.until(`the page for "${title}"`, (d) => page(d)?.title === title);
});

Then('no project page is open', async function (this: ThreadbearWorld) {
  await this.until('no project page', (d) => !page(d));
});

Then('the project page lists the folders {string}', async function (this: ThreadbearWorld, list: string) {
  await this.until(`the folders ${list}`, (d) => (page(d)?.folders ?? []).join(', ') === list);
});

Then('the project page lists {int} threads', async function (this: ThreadbearWorld, count: number) {
  await this.until(`${count} threads on the page`, (d) => page(d)?.threads?.length === count);
});

// A tile's tip starts with the project's name, then the thread's folder,
// which differs from one Mac to another.
When('I click the project tile {string}', async function (this: ThreadbearWorld, title: string) {
  await this.app.do({ cmd: 'click', tipPrefix: `${title}, in ` });
});

When('I press New Thread on the project\'s page', async function (this: ThreadbearWorld) {
  await this.until('a project page', (d) => Boolean(page(d)));
  await this.app.do({ cmd: 'click', tip: 'Start a thread in this project, in its folder, with its goals from the first message' });
});

When('I press the control {string}', async function (this: ThreadbearWorld, tip: string) {
  await this.app.do({ cmd: 'click', tip });
});

Then('the right rail has no {string} button', async function (this: ThreadbearWorld, title: string) {
  // Once the rail has something, so a rail not drawn yet doesn't pass.
  await this.until(`no "${title}" on the right rail`, (d) => {
    const right = d.rails?.right ?? [];
    return right.length > 0 && !right.some((b) => b.title === title);
  });
});
