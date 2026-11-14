// A side panel pinned to its thread (Pin, on the tab's right-click menu):
// desktop/gpui/src/pinned_panels.rs. The menu's own items are driven and read
// through `panel-menu`; the pinned panels are the dump's `pinnedPanels`.
import { Then, When } from '@cucumber/cucumber';
import assert from 'node:assert/strict';
import type { Dump } from '../support/driver.ts';
import type { ThreadbearWorld } from '../support/world.ts';
import { pane } from '../support/pane.ts';

type Pinned = { id: string; thread: string; panel: string; title: string; tip?: string; available: boolean; showing: boolean; rows?: string[] | null };
const pins = (d: Dump): Pinned[] => (d as unknown as { pinnedPanels?: Pinned[] }).pinnedPanels ?? [];
const byTitle = (d: Dump, title: string) => pins(d).find((p) => p.title === title);

async function menu(world: ThreadbearWorld, panel: string, click?: string): Promise<string[]> {
  // The driver hands back a result that isn't a string as its JSON.
  return JSON.parse(await world.app.do({ cmd: 'panel-menu', panel, ...(click ? { click } : {}) })) as string[];
}

Then("the {string} panel's menu offers {string}", async function (this: ThreadbearWorld, panel: string, item: string) {
  assert.deepEqual(await menu(this, panel), [item]);
});

Then("the {string} panel's menu offers nothing", async function (this: ThreadbearWorld, panel: string) {
  assert.deepEqual(await menu(this, panel), []);
});

When("I choose {string} from the {string} panel's menu", async function (this: ThreadbearWorld, item: string, panel: string) {
  await menu(this, panel, item);
});

When("I choose {string} from the pinned {string} panel's menu", async function (this: ThreadbearWorld, item: string, title: string) {
  const pin = await this.until(`"${title}" pinned`, (d) => byTitle(d, title));
  await menu(this, pin.id, item);
});

Then('{string} is pinned to {string} and showing', async function (this: ThreadbearWorld, title: string, thread: string) {
  const t = await this.until(`a thread called "${thread}"`, (d) => this.thread(d, thread));
  const pin = await this.until(`"${title}" pinned to "${thread}", available and showing`, (d) => {
    const p = byTitle(d, title);
    return p && p.thread === t.id && p.available && p.showing ? p : undefined;
  });
  assert.equal(pin.tip, `Pinned to “${thread}”`);
});

Then('the pinned {string} panel lists {string}', async function (this: ThreadbearWorld, title: string, rows: string) {
  const want = rows.split(' | ');
  await this.until(`the pinned "${title}" listing ${rows}`, (d) => {
    const got = byTitle(d, title)?.rows ?? [];
    return want.every((r) => got.includes(r));
  });
});

// Unpin put the ordinary panel back where the pinned one was, following the
// selection again: it's the section showing for its thread.
Then('the pane is showing {string}', async function (this: ThreadbearWorld, id: string) {
  await this.until(`the pane to be showing ${id}`, (d) => pane(d)?.section === id);
});

Then('nothing is pinned', async function (this: ThreadbearWorld) {
  await this.until('no pinned panels', (d) => pins(d).length === 0);
});
