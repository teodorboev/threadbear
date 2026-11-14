// The Browser pane and BrowserBear's handoff (browser-pane.feature), against
// the stand-in BrowserBear in support/browserbear.ts.
import { Given, Then, When } from '@cucumber/cucumber';
import assert from 'node:assert/strict';
import { okValue, sleep, type Dump } from '../support/driver.ts';
import type { ThreadbearWorld } from '../support/world.ts';

type Page = { title: string; host: string; buttons: string[] };
type BrowserWorld = ThreadbearWorld & { pages?: Map<string, number> };

const bear = (world: ThreadbearWorld) => {
  assert.ok(world.bear, 'the scenario has no stand-in BrowserBear (tag it @browserbear)');
  return world.bear;
};

const tabOf = (world: BrowserWorld, title: string): number => {
  const tab = world.pages?.get(title);
  assert.ok(tab, `no page called "${title}"`);
  return tab;
};

Given('BrowserBear has {string} at {string} open for {string}', async function (this: BrowserWorld, title: string, url: string, threadTitle: string) {
  const thread = await this.until(`a thread called "${threadTitle}"`, (d) => this.thread(d, threadTitle));
  this.pages ??= new Map();
  const tabId = 1000 + this.pages.size;
  this.pages.set(title, tabId);
  bear(this).open({ tabId, url, title, thread: thread.id, state: 'working', action: 'reading the page', openedAt: Date.now() });
});

When('the agent hands {string} to me', function (this: BrowserWorld, title: string) {
  bear(this).handOver(tabOf(this, title));
});

const browserButton = (d: Dump) => d.rails?.right.find((b) => b.title === 'Browser');

Then("the right rail's Browser button shows {int} page(s)", async function (this: ThreadbearWorld, pages: number) {
  await this.until(`the Browser button counting ${pages}`, (d) => browserButton(d)?.badge === pages);
});

When("I click the right rail's Browser button", async function (this: ThreadbearWorld) {
  await this.until('the Browser button on the right rail', browserButton);
  await this.app.do({ cmd: 'click', tipPrefix: "This thread's browser" });
});

// The pane's pages while it's on show (GPUI's dump has them as `panels.browser`).
const listed = (d: Dump): Page[] | undefined => {
  const b = (d as Dump & { panels?: { browser?: { pages?: Page[] } } }).panels?.browser;
  return b ? b.pages ?? [] : undefined;
};

Then('the Browser pane lists {string} from {string} with {string}', async function (this: ThreadbearWorld, title: string, host: string, buttons: string) {
  await this.until(`"${title}" (${host}) in the Browser pane with ${buttons}`, (d) => listed(d)?.some((p) => p.title === title && p.host === host && p.buttons.join(', ') === buttons));
});

const tips: Record<string, string> = {
  'Show window': "Bring this page's window to the front",
  Done: "Tell the agent you've finished here",
};

When('I press {string} on {string}', async function (this: ThreadbearWorld, button: string, title: string) {
  await this.until(`"${title}" with ${button} in the Browser pane`, (d) => listed(d)?.some((p) => p.title === title && p.buttons.includes(button)));
  await this.app.do({ cmd: 'click', tipPrefix: tips[button] });
});

async function heard(world: ThreadbearWorld, what: string, check: (h: { cmd: string; args: Record<string, unknown>; thread?: string }) => boolean): Promise<void> {
  for (let i = 0; i < 100; i += 1) {
    if (bear(world).heard.some(check)) return;
    await sleep(100);
  }
  assert.fail(`BrowserBear never heard ${what}; it heard ${JSON.stringify(bear(world).heard)}`);
}

Then('BrowserBear was asked to show that page for {string}', async function (this: BrowserWorld, threadTitle: string) {
  const thread = this.thread(await this.app.dump(), threadTitle);
  const tab = [...(this.pages?.values() ?? [])][0];
  await heard(this, `panelShow for tab ${tab}`, (h) => h.cmd === 'panelShow' && h.args.tab === tab && h.args.profile === 'agent-chrome' && h.thread === thread?.id);
});

Then("BrowserBear was told I'm done with {string}", async function (this: BrowserWorld, title: string) {
  const tab = tabOf(this, title);
  await heard(this, `markDone for tab ${tab}`, (h) => h.cmd === 'markDone' && h.args.tab === tab);
});

Then('BrowserBear was asked to hand over {string}', async function (this: ThreadbearWorld, url: string) {
  await heard(this, `a handoff of ${url}`, (h) => h.cmd === 'handoff' && h.args.tab === url);
  // Its answer is back by now; a beat for the app to have opened nothing.
  await sleep(500);
});

Then('{string} was opened in the browser', async function (this: ThreadbearWorld, url: string) {
  const seen: string[] = [];
  for (let i = 0; i < 60; i += 1) {
    seen.push(...(JSON.parse(okValue(await this.app.do({ cmd: 'opened' }))) as string[]));
    if (seen.includes(url)) return;
    await sleep(100);
  }
  assert.fail(`${url} wasn't opened; what was: ${JSON.stringify(seen)}`);
});
