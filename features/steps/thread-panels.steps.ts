// The panels round the thread: the Threads panel folded away with ⌘/, a
// thread in a panel of its own (Open in New Panel, Move to Main Panel, the
// main panel's note), and tabs opened beside the thread for what it shows.
import { Given, Then, When } from '@cucumber/cucumber';
import assert from 'node:assert/strict';
import type { Dump } from '../support/driver.ts';
import type { ThreadbearWorld } from '../support/world.ts';

Then('the Threads panel is folded away', async function (this: ThreadbearWorld) {
  await this.until('the Threads panel off screen', (d) => d.threadsShowing === false);
});

Then('the Threads panel is showing', async function (this: ThreadbearWorld) {
  await this.until('the Threads panel on screen', (d) => d.threadsShowing === true);
});

async function threadId(world: ThreadbearWorld, title: string): Promise<string> {
  const t = await world.until(`a thread called "${title}"`, (d) => world.thread(d, title));
  return t.id;
}

const panelOf = (d: Dump, id: string) => d.threadPanels?.find((p) => p.id === id);

When('I open {string} in a new panel', async function (this: ThreadbearWorld, title: string) {
  // As the row's Open in New Panel does (the Swift app's row menu is a real
  // menu, which a script can read but not pick from).
  await this.app.do({ cmd: 'open-panel', id: await threadId(this, title) });
});

When('I move {string} to the main panel', async function (this: ThreadbearWorld, title: string) {
  await this.app.do({ cmd: 'move-to-main', id: await threadId(this, title) });
});

When('I pick {string} in the sidebar', async function (this: ThreadbearWorld, title: string) {
  // As a click on its row does, without waiting for it to load in the main panel.
  await this.app.do({ cmd: 'select', id: await threadId(this, title) });
});

// The note's buttons, by their tooltips, which both apps find controls by.
const noteButtons: Record<string, string> = {
  'Go to it': 'Bring its panel to the front',
  'Show it here': 'Close its panel and show the thread in the main panel',
};

When("I press {string} on the main panel's note", async function (this: ThreadbearWorld, label: string) {
  const tip = noteButtons[label];
  assert.ok(tip, `the note has no ${label} button`);
  await this.app.do({ cmd: 'click', tip });
});

Then('{string} is in a panel of its own, showing its transcript', async function (this: ThreadbearWorld, title: string) {
  const id = await threadId(this, title);
  await this.until(`${title} in a panel of its own, its transcript loaded`, (d) => {
    const panel = panelOf(d, id);
    return panel?.showing && panel.transcript?.state === 'loaded' && panel.transcript.rows.length > 0 && (d.dock ?? '').includes(`thread:${id}`);
  });
});

Then("{string} isn't in a panel of its own", async function (this: ThreadbearWorld, title: string) {
  const id = await threadId(this, title);
  await this.until(`${title}'s own panel gone`, (d) => !panelOf(d, id) && !(d.dock ?? '').includes(`thread:${id}`));
});

Then('the new-thread page shows in the main panel', async function (this: ThreadbearWorld) {
  await this.until('the new-thread page', (d) => !d.selection && d.composer.draft === 'new');
});

// No click gets there (see the feature file), so the script puts it there.
Given("the main panel has {string} selected while it's in a panel of its own", async function (this: ThreadbearWorld, title: string) {
  await this.app.do({ cmd: 'open-panel', id: await threadId(this, title), keepSelected: true });
});

Then('the main panel says {string} is open in its own panel', async function (this: ThreadbearWorld, title: string) {
  const id = await threadId(this, title);
  const d = await this.until(`the note that ${title} is open in its own panel`, (d) => d.openElsewhere === id && d);
  assert.equal(d.selection?.id, id, 'the main panel has it selected');
});

When('the agent asks to show the {string} panel as a tab', async function (this: ThreadbearWorld, panel: string) {
  const d = await this.app.dump();
  const threadId = d.selection?.id;
  assert.ok(threadId, 'no thread open');
  await this.app.do({ cmd: 'mock', method: 'showPanel', params: { threadId, panel, where: 'tab' } });
});
