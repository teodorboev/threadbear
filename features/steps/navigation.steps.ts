// Back and forward, Next Inbox Thread, shortcuts.json and the menu bar.
import { Given, Then, When } from '@cucumber/cucumber';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { okValue, type Dump } from '../support/driver.ts';
import type { ThreadbearWorld } from '../support/world.ts';

When("I press the mouse's {string} button", async function (this: ThreadbearWorld, button: string) {
  await this.app.do({ cmd: 'mouse-button', button });
});

When('I open the first thread in the Inbox', async function (this: ThreadbearWorld) {
  const d = await this.app.dump();
  const inbox = d.sidebar.sections.find((s) => s.name === 'Inbox')?.threads ?? [];
  assert.ok(inbox.length > 1, 'too little in the Inbox');
  await this.open(inbox[0].title);
});

Then('the second thread in the Inbox is open', async function (this: ThreadbearWorld) {
  const d = await this.app.dump();
  const inbox = d.sidebar.sections.find((s) => s.name === 'Inbox')?.threads ?? [];
  const second = inbox[1].id;
  await this.until('the second inbox thread to open', (now) => now.selection?.id === second);
});

When('shortcuts.json gives {string} the key {string}', async function (this: ThreadbearWorld, command: string, key: string) {
  const out = await this.app.do({ cmd: 'data-dir' });
  const dir = okValue(out);
  assert.ok(fs.existsSync(dir), `no data folder: ${dir}`);
  fs.writeFileSync(path.join(dir, 'shortcuts.json'), JSON.stringify({ [command]: key }));
});

Then('the {string} menu has {string} on {string}', async function (this: ThreadbearWorld, menu: string, title: string, key: string) {
  let last = '';
  const deadline = Date.now() + 8000;
  for (;;) {
    const out = JSON.parse(okValue(await this.app.do({ cmd: 'menus' }))) as { title: string; items: { title: string; key?: string }[] }[];
    const item = out.find((m) => m.title === menu)?.items.find((i) => i.title === title);
    last = JSON.stringify(item ?? null);
    if (item && item.key === key) return;
    assert.ok(Date.now() < deadline, `${title} in ${menu}: ${last}`);
    await new Promise((r) => setTimeout(r, 200));
  }
});

Then('the {string} menu has {string}', async function (this: ThreadbearWorld, menu: string, title: string) {
  const out = JSON.parse(okValue(await this.app.do({ cmd: 'menus' }))) as { title: string; items: { title: string }[] }[];
  assert.ok(out.find((m) => m.title === menu)?.items.some((i) => i.title === title), `${title} isn't in the ${menu} menu: ${JSON.stringify(out.find((m) => m.title === menu)?.items.map((i) => i.title))}`);
});

Then("the {string} menu hasn't {string}", async function (this: ThreadbearWorld, menu: string, title: string) {
  const out = JSON.parse(okValue(await this.app.do({ cmd: 'menus' }))) as { title: string; items: { title: string }[] }[];
  const items = out.find((m) => m.title === menu)?.items.map((i) => i.title);
  assert.ok(items, `no ${menu} menu`);
  assert.ok(!items.includes(title), `${title} is in the ${menu} menu: ${JSON.stringify(items)}`);
});

// The app menu is the first one, whatever it's called (the dev build's is
// named for its instance).
async function appMenuKey(world: ThreadbearWorld, title: string, key: string) {
  const out = JSON.parse(okValue(await world.app.do({ cmd: 'menus' }))) as { title: string; items: { title: string; key?: string }[] }[];
  const item = out[0]?.items.find((i) => i.title === title);
  assert.equal(item?.key, key, `${title} in the app menu: ${JSON.stringify(item ?? null)}`);
}

Then('the app menu has {string} on {string}', async function (this: ThreadbearWorld, title: string, key: string) {
  await appMenuKey(this, title, key);
});

// Quit is ⌘Q on a Mac, and Control-Q on Windows and Linux, as apps there
// have it (shell.rs, standard_key_display).
Then('the app menu has {string} on {string} on a Mac, {string} elsewhere', async function (this: ThreadbearWorld, title: string, mac: string, elsewhere: string) {
  await appMenuKey(this, title, process.platform === 'darwin' ? mac : elsewhere);
});

Then("the open thread isn't {string}", async function (this: ThreadbearWorld, title: string) {
  await this.until(`"${title}" to be gone from the panel`, (d) => d.selection?.title !== title);
});

// The sidebar's keys (`sidebar.keys`). The arrows are slash-palette.steps.ts's
// "I press the down arrow": both go wherever the keyboard is.

// A real click through the window, on the row's middle, found by its tooltip
// (its title, then its folder).
When('I click the row {string}', async function (this: ThreadbearWorld, title: string) {
  const t = await this.until(`a row called "${title}"`, (d) => this.thread(d, title));
  await this.app.do({ cmd: 'click', tipPrefix: `${title}\n` });
  await this.until(`"${title}" open`, (d) => d.selection?.id === t.id);
});

When('I click the first row in the sidebar', async function (this: ThreadbearWorld) {
  const d = await this.until('a row in the sidebar', (now) => now.sidebar.sections.find((s) => !s.collapsed && s.threads.length)?.threads[0]);
  await this.app.do({ cmd: 'click', tipPrefix: `${d.title}\n` });
  await this.until(`"${d.title}" open`, (now) => now.selection?.id === d.id);
});

Then('the Threads panel has the keyboard', async function (this: ThreadbearWorld) {
  await this.until('the Threads panel with the keyboard', (d) => d.sidebar.keyboard === true);
});

/** The threads the arrows walk, top to bottom: every section's but a folded
 * one's, and only the Archived rows it draws. */
function arrowOrder(d: Dump): string[] {
  return d.sidebar.sections.flatMap((s) => (s.collapsed ? [] : s.threads.slice(0, s.shown ?? s.threads.length).map((t) => t.id)));
}

Then('the thread below {string} is open', async function (this: ThreadbearWorld, title: string) {
  const d = await this.app.dump();
  const order = arrowOrder(d);
  const at = order.indexOf(this.thread(d, title)?.id ?? '');
  assert.ok(at >= 0 && at + 1 < order.length, `nothing below "${title}": ${JSON.stringify(order)}`);
  await this.until('the thread below it open', (now) => now.selection?.id === order[at + 1]);
});

When('I press the down arrow until the last thread in the sidebar is open', async function (this: ThreadbearWorld) {
  const order = arrowOrder(await this.app.dump());
  const last = order[order.length - 1];
  for (let i = 0; i < order.length + 2; i += 1) {
    if ((await this.app.dump()).selection?.id === last) return;
    await this.app.do({ cmd: 'arrow', dir: 'down' });
  }
  await this.until('the last thread open', (d) => d.selection?.id === last);
});

When('I go down the Inbox to its last thread with Next Inbox Thread', async function (this: ThreadbearWorld) {
  const inbox = (await this.app.dump()).sidebar.sections.find((s) => s.name === 'Inbox')?.threads ?? [];
  const last = inbox[inbox.length - 1]?.id;
  assert.ok(last, 'nothing in the Inbox');
  for (let i = 0; i < inbox.length + 2; i += 1) {
    if ((await this.app.dump()).selection?.id === last) return;
    await this.app.do({ cmd: 'menu-key', key: 'down', modifiers: ['cmd', 'opt'] });
  }
  await this.until('the Inbox\'s last thread open', (d) => d.selection?.id === last);
});

// As short as the window goes, so the threads run past the foot of the list.
Given('the window is short', async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'resize', w: 1000, h: 480 });
});

Then("the open thread's row is in view in the sidebar", async function (this: ThreadbearWorld) {
  await this.until('the open row in view', (d) => {
    const id = d.selection?.id;
    return d.sidebar.sections.some((s) => s.threads.some((t) => t.id === id && t.inView === true));
  });
});
