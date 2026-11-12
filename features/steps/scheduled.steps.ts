// The Scheduled view: its rows (in the sidebar's thread shape), the page
// beside it, and the editor (features/scheduled.feature).
import { Given, Then, When } from '@cucumber/cucumber';
import assert from 'node:assert/strict';
import type { Thread } from '../support/driver.ts';
import type { ThreadbearWorld } from '../support/world.ts';

/** The page in place of the thread, and the editor, from either app's dump. */
type Extra = {
  homePage?: { title: string; status?: string | null; stats: { value: string; label: string }[] } | null;
  scheduleEditor?: { isNew: boolean; text: string; once: boolean; cron?: string | null } | null;
};

async function row(world: ThreadbearWorld, title: string): Promise<Thread> {
  return world.until(`a schedule called "${title}"`, (d) => world.thread(d, title));
}

Then('the schedule {string} runs {string}', async function (this: ThreadbearWorld, title: string, when: string) {
  await this.until(`"${title}" saying ${when}`, (d) => this.thread(d, title)?.trailing === when);
});

// The line under the title: when it runs, the other Mac it runs on, then where it sends.
Then('the schedule {string} sends to {string}', async function (this: ThreadbearWorld, title: string, target: string) {
  const r = await row(this, title);
  assert.ok(r.place?.endsWith(` · ${target}`), `"${title}" says: ${r.place}`);
});

Then('the schedule {string} runs on {string} and sends to {string}', async function (this: ThreadbearWorld, title: string, runner: string, target: string) {
  const r = await row(this, title);
  assert.ok(r.place?.endsWith(` · ${runner} · ${target}`), `"${title}" says: ${r.place}`);
});

Then('the schedule {string} notes {string}', async function (this: ThreadbearWorld, title: string, note: string) {
  await this.until(`"${title}" noting "${note}"`, (d) => this.thread(d, title)?.waiting === note);
});

Then('the Scheduled page is in place of the thread', async function (this: ThreadbearWorld) {
  await this.until('the Scheduled page', (d) => (d as unknown as Extra).homePage?.title === 'Scheduled');
});

Then('no page is in place of the thread', async function (this: ThreadbearWorld) {
  await this.until('the thread back in place of the page', (d) => !(d as unknown as Extra).homePage);
});

Then("the page says what's next, starting {string}", async function (this: ThreadbearWorld, start: string) {
  const page = await this.until('the Scheduled page', (d) => (d as unknown as Extra).homePage);
  assert.ok(page.status?.startsWith(start), `it says: ${page.status}`);
});

Then("the page's tiles include {string}", async function (this: ThreadbearWorld, list: string) {
  const page = await this.until('the Scheduled page with its tiles', (d) => (d as unknown as Extra).homePage?.stats?.length && (d as unknown as Extra).homePage);
  const tiles = page.stats.map((s) => `${s.value} ${s.label}`);
  for (const tile of list.split(', ')) assert.ok(tiles.includes(tile), `${tile} not in: ${tiles.join(', ')}`);
});

/** Threads' ids by title, noted while the inbox lists them: the Scheduled view lists schedules instead. */
const noted = new WeakMap<ThreadbearWorld, Map<string, string>>();

Given('the thread {string} is in the inbox', async function (this: ThreadbearWorld, title: string) {
  const t = await this.until(`"${title}" in the inbox`, (d) => this.thread(d, title));
  if (!noted.has(this)) noted.set(this, new Map());
  noted.get(this)!.set(title, t.id);
});

// What a schedule's target does: the thread, with the sidebar left as it is.
When('I pick the thread {string} from where I am', async function (this: ThreadbearWorld, title: string) {
  const id = noted.get(this)?.get(title);
  assert.ok(id, `note "${title}" in the inbox first`);
  await this.app.do({ cmd: 'select', id });
});

When('I open the schedule {string} in the editor', async function (this: ThreadbearWorld, title: string) {
  const r = await row(this, title);
  await this.app.do({ cmd: 'schedule', id: r.id });
});

Then('the schedule editor is open on a new schedule that repeats on {string}', async function (this: ThreadbearWorld, cron: string) {
  const editor = await this.until('the schedule editor', (d) => (d as unknown as Extra).scheduleEditor);
  assert.equal(editor.isNew, true, 'it opened on a schedule that exists');
  assert.equal(editor.once, false, 'it opened on Once');
  assert.equal(editor.cron, cron);
});

Then('the schedule editor is open on {string} repeating on {string}', async function (this: ThreadbearWorld, text: string, cron: string) {
  const editor = await this.until('the schedule editor', (d) => (d as unknown as Extra).scheduleEditor);
  assert.equal(editor.isNew, false, 'it opened on a new schedule');
  assert.equal(editor.text, text);
  assert.equal(editor.cron, cron);
});

Then('the schedule editor is open on the schedule that sends {string}', async function (this: ThreadbearWorld, text: string) {
  const editor = await this.until('the schedule editor', (d) => (d as unknown as Extra).scheduleEditor);
  assert.equal(editor.isNew, false, 'it opened on a new schedule');
  assert.equal(editor.text, text);
});
