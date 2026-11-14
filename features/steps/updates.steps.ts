// The Updates panel (features/updates.feature). Its lines are what it draws:
// "# App", "# Runner", "# From Mike" and "# CLIs" for its headings, a CLI as
// "<what it has> (<its update setting>) [<its buttons>]", and what's under a
// line indented by two spaces.
import { Then, When } from '@cucumber/cucumber';
import assert from 'node:assert/strict';
import { okValue, type Dump } from '../support/driver.ts';
import type { ThreadbearWorld } from '../support/world.ts';

When('the runner starts building main at {string} for {string}', async function (this: ThreadbearWorld, commit: string, subject: string) {
  await this.app.do({ cmd: 'mock', method: 'updates', params: { state: 'building', commit, subject } });
});

When('Mike has a release of 10 Nov with 3 new changes', async function (this: ThreadbearWorld) {
  const release = {
    tag: 'release/2026-11-10',
    commit: '0123456789abcdef0123456789abcdef01234567',
    changes: 3,
    subjects: ['Runners panel in the GPUI app', 'Updates panel', 'Connectors panel'],
    notes: [{ tag: 'release/2026-11-10', lines: ['Runners in the app'] }],
  };
  await this.app.do({ cmd: 'mock', method: 'updates', params: { state: 'idle', upstream: { state: 'available', release } } });
});

When('my copy has 2 changes of its own to the old app', async function (this: ThreadbearWorld) {
  const changes = [
    { commit: 'abc1234def567890abc1234def567890abc1234d', short: 'abc1234', subject: 'Orange dock badge', date: '2026-11-02T09:00:00+08:00', files: ['dock/Sources/Badge.swift'] },
    { commit: '0123456789abcdef0123456789abcdef01234567', short: '0123456', subject: 'Bigger sidebar font', date: '2026-10-20T10:00:00+08:00', files: ['app/Sources/Threadbear/Views/Sidebar.swift'] },
  ];
  await this.app.do({ cmd: 'mock', method: 'updates', params: { state: 'idle', swiftChanges: { state: 'ready', checkout: '/Users/me/threadbear', changes } } });
});

// The app's own update, made up (`updates` with `preview`, as both apps'
// screenshots use): staged is def5678 over abc1234.
When("the app's next version, def5678, is ready", async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'updates', preview: 'staged' });
});

When("the app's next version is waiting for a Mac to build it on", async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'updates', preview: 'waiting' });
});

When("the app's next version is waiting for main to settle", async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'updates', preview: 'settling' });
});

When('no Mac is trusted to build the app\'s next version', async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'updates', preview: 'no-builder' });
});

// The app menu is the first one, whatever it's called. SwiftUI rebuilds its
// menus a moment after what they show changes, so this looks a few times.
Then('the app menu has {string}', async function (this: ThreadbearWorld, title: string) {
  let titles: string[] = [];
  for (let i = 0; i < 20; i += 1) {
    const out = JSON.parse(okValue(await this.app.do({ cmd: 'menus' }))) as { title: string; items: { title: string }[] }[];
    titles = out[0]?.items.map((m) => m.title) ?? [];
    if (titles.includes(title)) return;
    await new Promise((done) => setTimeout(done, 250));
  }
  assert.fail(`${title} isn't in the app menu: ${JSON.stringify(titles)}`);
});

// What a restart onto the staged version by itself waits for, as the GPUI
// app's once-a-second look sees it (`updates` with `restartCheck`). Whether
// the app's in front and how long the desktop's been idle are the machine's,
// so only what holds the restart off is checked.
type RestartCheck = { staged: boolean; loaded: boolean; hasDrafts: boolean; held: boolean; otherWindowInput: boolean };

async function restartWaits(world: ThreadbearWorld, what: string, wanted: (c: RestartCheck) => boolean) {
  let seen: RestartCheck | undefined;
  for (let i = 0; i < 100; i += 1) {
    seen = JSON.parse(await world.app.do({ cmd: 'updates', restartCheck: true })) as RestartCheck;
    if (wanted(seen)) return;
    await new Promise((done) => setTimeout(done, 100));
  }
  assert.fail(`a restart should wait for ${what}, but the app saw ${JSON.stringify(seen)}`);
}

Then('a restart onto it waits for nothing', async function (this: ThreadbearWorld) {
  await restartWaits(this, 'nothing', (c) => c.staged && c.loaded && !c.hasDrafts && !c.held && !c.otherWindowInput);
});

Then('a restart onto it waits for the words typed', async function (this: ThreadbearWorld) {
  await restartWaits(this, 'the words typed', (c) => c.hasDrafts);
});

Then('a restart onto it waits for the open menu', async function (this: ThreadbearWorld) {
  await restartWaits(this, 'the open menu', (c) => c.held && !c.hasDrafts);
});

// Quitting ends every shell, so a busy or typed-in one holds it off.
Then('a restart onto it waits for the Terminal', async function (this: ThreadbearWorld) {
  await restartWaits(this, 'the Terminal', (c) => c.held && !c.hasDrafts);
});

When('I type {string} in the Terminal without pressing Return', async function (this: ThreadbearWorld, text: string) {
  await this.until('a shell running', (d) => d.terminal?.shell === 'running');
  await this.app.do({ cmd: 'terminal', keys: text });
});

// What a restart the app made itself leaves for the next launch, and that
// launch putting it back (`updates` with `restore`), as a scripted app
// never restarts.
// The thread's id by its title, kept from the Inbox: the Scheduled list
// doesn't list it.
const restored = new Map<string, string>();

When('the app comes back from restarting onto its next version on the Scheduled list with {string} open', async function (this: ThreadbearWorld, title: string) {
  const thread = await this.until(`a thread called "${title}"`, (d) => this.thread(d, title));
  restored.set(title, thread.id);
  await this.app.do({ cmd: 'updates', restore: { mode: 'scheduled', selection: thread.id, front: false } });
});

// What a restart would save now, then put back as a new launch would
// (`fresh`: no thread's place remembered, none kept loaded), with the given
// thread open on the Inbox.
When('the app comes back from restarting onto its next version with {string} open', async function (this: ThreadbearWorld, title: string) {
  const thread = await this.until(`a thread called "${title}"`, (d) => this.thread(d, title));
  const state = JSON.parse(okValue(await this.app.do({ cmd: 'updates', restartState: true })));
  await this.app.do({ cmd: 'updates', restore: { ...state, mode: 'inbox', selection: thread.id, front: false }, fresh: true });
  await this.until(`"${title}" open`, (d) => d.selection?.id === thread.id);
});

When('the app restarts onto its next version with {string} open', function () {
  throw new Error('Manual: on the installed app, open the thread, then Restart to Update from the app menu, and watch the window stay put showing it');
});

Then('the window stays on screen showing {string} throughout', function () {
  throw new Error('Manual: the window never goes or flashes, and comes back on the same thread at the same place');
});

Then('the sidebar shows the Scheduled list with {string} open', async function (this: ThreadbearWorld, title: string) {
  const id = restored.get(title);
  assert.ok(id, `"${title}" wasn't put back`);
  const where = (d: Dump) => ({ mode: (d.sidebar as { mode?: string }).mode, open: d.selection?.id });
  await this.until('the Scheduled list with the thread open', (d) => where(d).mode === 'scheduled' && where(d).open === id);
  // And still so a moment later: opening it mustn't go back to the Inbox.
  await new Promise((done) => setTimeout(done, 800));
  const later = where(await this.app.dump());
  assert.deepEqual(later, { mode: 'scheduled', open: id });
});

When('I right-click the Updates icon', async function (this: ThreadbearWorld) {
  const out = await this.app.do({ cmd: 'updates', rightClick: true });
  assert.match(out, /popover shown/);
});

// The popover's line, as both apps' `updates` say it: "popover: shown, <line>".
Then('the update popover says {string}', async function (this: ThreadbearWorld, line: string) {
  let lines: string[] = [];
  for (let i = 0; i < 20; i += 1) {
    lines = JSON.parse(okValue(await this.app.do({ cmd: 'updates' }))) as string[];
    if (lines.includes(`popover: shown, ${line}`)) return;
    await new Promise((done) => setTimeout(done, 200));
  }
  assert.fail(`the popover doesn't say "${line}": ${JSON.stringify(lines)}`);
});

When('the web runtime needs a security update', async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'mock', method: 'updates', params: { state: 'idle', webRuntimeProblem: "Web runtime: CEF is over two weeks behind stable. Chromium's sandbox is off." } });
});
