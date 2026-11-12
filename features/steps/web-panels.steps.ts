// A patch's web panel and the Convex panel (features/web-panels.feature): the
// page beside the thread, kept between visits, its links to other sites
// opening outside it, the bar's Back, Reload and Open in browser, and the
// Convex dashboard given its deploy key.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { Given, Then, When } from '@cucumber/cucumber';
import type { ThreadbearWorld } from '../support/world.ts';
import { okValue, type Dump } from '../support/driver.ts';
import { pane } from '../support/pane.ts';
import { sites } from '../support/web-runtime.ts';

type PageInfo = { key: string; url?: string; title?: string; address?: string; loading?: boolean; loads?: number; canGoBack?: boolean; hidden?: boolean; muted?: boolean };
type ConvexInfo = { project?: string | null; label?: string; deployment?: string; choices?: string[]; note?: string | null; failure?: string | null; code?: string | null; signInFailure?: string | null };
type WebPane = { url?: string; shows: string; note?: string; buttons: string[]; page?: PageInfo; convex?: ConvexInfo };

/** The shown panel's page as the pane draws it (`panels.pane.web`). */
const web = (d: Dump) => (pane(d) as { web?: WebPane } | undefined)?.web;
/** Every page the app keeps (`webPanels.pages`, GPUI's own). */
const pages = (d: Dump) => ((d as { webPanels?: { pages: PageInfo[] } }).webPanels?.pages ?? []);

/** A page loading the first time starts Chromium as well. */
const LOADED_MS = 60_000;

async function threadId(world: ThreadbearWorld): Promise<string> {
  const d = await world.app.dump();
  assert.ok(d.selection?.id, 'a thread should be open');
  return d.selection.id;
}

Given("the thread's url panel shows the page server", async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'mock', method: 'urlPanel', params: { threadId: await threadId(this), url: sites().home, title: 'Preview' } });
});

Given("the thread's url panel shows {string}", async function (this: ThreadbearWorld, url: string) {
  await this.app.do({ cmd: 'mock', method: 'urlPanel', params: { threadId: await threadId(this), url, title: 'Preview' } });
});

Given('the thread has the Convex panel', async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'mock', method: 'convexPanel', params: { threadId: await threadId(this) } });
});

Given('the thread has the Convex panel, with a deploy key', async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'mock', method: 'convexPanel', params: { threadId: await threadId(this), key: 'ok' } });
});

Given('the thread has the Convex panel, signed out of Convex', async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'mock', method: 'convexPanel', params: { threadId: await threadId(this), key: 'signedOut' } });
});

Then('the panel shows the page {string}', async function (this: ThreadbearWorld, title: string) {
  await this.until(`the panel showing "${title}"`, (d) => web(d)?.shows === 'page' && web(d)?.page?.title === title && web(d)?.page?.loading === false && web(d)?.page?.hidden === false, LOADED_MS);
});

let loadsBefore = 0;

Then("the panel's page is out of sight and its sound is off", async function (this: ThreadbearWorld) {
  const url = sites().home;
  await this.until('the page hidden and muted', (d) => pages(d).some((p) => p.key === url && p.hidden === true && p.muted === true));
  const d = await this.app.dump();
  loadsBefore = pages(d).find((p) => p.key === url)?.loads ?? 0;
});

Then('the panel shows the page {string} without loading it again', async function (this: ThreadbearWorld, title: string) {
  await this.until(`"${title}" back, as it was`, (d) => web(d)?.page?.title === title && web(d)?.page?.hidden === false && web(d)?.page?.muted === false);
  const d = await this.app.dump();
  assert.equal(web(d)?.page?.loads, loadsBefore, 'the page kept between visits loads once');
});

When("I click the panel page's link/button {string}", async function (this: ThreadbearWorld, selector: string) {
  // With a person's gesture behind it (DevTools' userGesture), which is
  // what decides whether a link leaves the panel.
  const out = await this.app.do({ cmd: 'webPanel', do: 'click', selector }, 30);
  assert.ok(!out.startsWith('error'), out);
});

/** What the app would have opened this scenario: a scripted app opens
 * nothing and keeps a list, which the `opened` command hands over and
 * empties, so it's gathered here. */
async function openedSoFar(world: ThreadbearWorld): Promise<string[]> {
  const w = world as ThreadbearWorld & { webOpened?: string[] };
  w.webOpened ??= [];
  w.webOpened.push(...(JSON.parse(okValue(await world.app.do({ cmd: 'opened' }))) as string[]));
  return w.webOpened;
}

async function waitOpened(world: ThreadbearWorld, url: string) {
  const deadline = Date.now() + 15_000;
  for (;;) {
    const seen = await openedSoFar(world);
    if (seen.includes(url)) return;
    if (Date.now() > deadline) assert.fail(`${url} wasn't opened; what was: ${JSON.stringify(seen)}`);
    await new Promise((done) => setTimeout(done, 150));
  }
}

Then('the link to the other site opened in the browser', async function (this: ThreadbearWorld) {
  await waitOpened(this, `${sites().other}/video`);
});

Then("the page server's page opened in the browser", async function (this: ThreadbearWorld) {
  await waitOpened(this, sites().home);
});

/** A script in the panel's page, run with no click behind it, as a
 * page's own timer or a compromised script would run it. */
async function pageScript(world: ThreadbearWorld, js: string) {
  const out = await world.app.do({ cmd: 'webPanel', do: 'eval', js: `void (${js})` });
  assert.ok(!out.startsWith('error'), out);
}

When('the panel\'s page opens a window to the other site by itself', async function (this: ThreadbearWorld) {
  await pageScript(this, `window.open(${JSON.stringify(`${sites().other}/by-itself`)})`);
});

When('the panel\'s page opens a window on its own site by itself', async function (this: ThreadbearWorld) {
  await pageScript(this, "window.open(location.origin + '/second')");
});

When("the panel's page clicks its own link {string} by itself", async function (this: ThreadbearWorld, selector: string) {
  // A script's click is a click event, not a person's: Chromium's gesture
  // flag says so, which is what decides.
  await pageScript(this, `document.querySelector(${JSON.stringify(selector)}).click()`);
});

When('the panel\'s page sends itself to the other site by itself', async function (this: ThreadbearWorld) {
  await pageScript(this, `location.href = ${JSON.stringify(`${sites().other}/by-itself`)}`);
});

When("I click the panel page's link {string} twice in a moment", async function (this: ThreadbearWorld, selector: string) {
  const out = await this.app.do({ cmd: 'webPanel', do: 'click', selector, times: 2 }, 30);
  assert.ok(!out.startsWith('error'), out);
});

When("I drop a file from the Finder on the panel's page", async function (this: ThreadbearWorld) {
  // A page of its own, so if it loaded the panel's title would say so.
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'tb-dropped-'));
  const file = path.join(dir, 'dropped.html');
  fs.writeFileSync(file, '<!doctype html><title>Dropped</title><p>A file from this Mac.</p>');
  const out = await this.app.do({ cmd: 'webPanel', do: 'drop', file });
  assert.ok(!out.startsWith('error'), out);
});

/** Everything the app would have opened over the next two seconds (or
 * `ms`), on top of what it already had: long enough for a page's own timers
 * and a drop to have done whatever they'd do. */
async function openedAfterAMoment(world: ThreadbearWorld, ms = 2000): Promise<string[]> {
  const until = Date.now() + ms;
  let seen: string[] = [];
  while (Date.now() < until) {
    seen = await openedSoFar(world);
    await new Promise((done) => setTimeout(done, 200));
  }
  return seen;
}

Then('nothing was opened in the browser', async function (this: ThreadbearWorld) {
  const seen = await openedAfterAMoment(this);
  assert.deepEqual(seen, [], `it opened ${JSON.stringify(seen)}`);
});

Then('only the first of them went to the browser', async function (this: ThreadbearWorld) {
  // The page's last try is a timer's, a few seconds after the click.
  const seen = await openedAfterAMoment(this, 4000);
  assert.deepEqual(seen, [`${sites().other}/video?0`], `it opened ${JSON.stringify(seen)}`);
});

for (const [times, count] of [['once', 1], ['twice', 2]] as const) {
  Then(`the link to the other site opened in the browser ${times}`, async function (this: ThreadbearWorld) {
    const seen = await openedAfterAMoment(this);
    const want = `${sites().other}/video`;
    assert.equal(seen.filter((u) => u === want).length, count, `it opened ${JSON.stringify(seen)}`);
  });
}

Then('{string} opened in the browser', async function (this: ThreadbearWorld, url: string) {
  await waitOpened(this, url);
});

Then("Convex's docs opened in the browser, not in the panel", async function (this: ThreadbearWorld) {
  await waitOpened(this, `${sites().other}/docs`);
});

Then("the panel's bar has {string}", async function (this: ThreadbearWorld, tip: string) {
  await this.until(`the bar offering "${tip}"`, (d) => web(d)?.buttons.includes(tip) === true);
});

Then("the panel's bar has no {string}", async function (this: ThreadbearWorld, tip: string) {
  await this.until(`the bar without "${tip}"`, (d) => web(d) !== undefined && !web(d)!.buttons.includes(tip));
});

Then('the panel asks to install the web runtime', async function (this: ThreadbearWorld) {
  await this.until('the question', (d) => web(d)?.shows === 'prompt');
});

Then('the panel says {string}', async function (this: ThreadbearWorld, words: string) {
  await this.until(`the panel saying "${words}"`, (d) => web(d)?.note === words);
});

When('I say Not now to the web runtime', async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'webRuntime', do: 'notNow' });
});

Then('the Convex panel is for the project {string}, showing {string}', async function (this: ThreadbearWorld, project: string, choice: string) {
  await this.until(`${project} on ${choice}`, (d) => {
    const c = web(d)?.convex;
    return c?.project === project && `${c?.label} · ${c?.deployment}` === choice;
  });
});

Then("the Convex panel's menu offers {string}", async function (this: ThreadbearWorld, choices: string) {
  const want = choices.split(' | ');
  await this.until(`the menu offering ${choices}`, (d) => JSON.stringify(web(d)?.convex?.choices) === JSON.stringify(want));
});

Then('the Convex panel says what the runner said: {string}', async function (this: ThreadbearWorld, words: string) {
  await this.until(`the panel saying "${words}"`, (d) => web(d)?.shows === 'failed' && web(d)?.convex?.failure === words);
});

Then('the Convex panel offers to sign in to Convex', async function (this: ThreadbearWorld) {
  await this.until('Sign in to Convex', (d) => web(d)?.shows === 'failed' && web(d)?.convex?.failure === "The runner's Convex CLI isn't signed in.");
});

Then("Convex's sign-in page opened in the browser", async function (this: ThreadbearWorld) {
  await waitOpened(this, 'https://auth.convex.dev/device');
});

Then('the Convex panel waits for the sign-in with the code {string}', async function (this: ThreadbearWorld, code: string) {
  await this.until(`waiting with ${code}`, (d) => web(d)?.shows === 'signingIn' && web(d)?.convex?.code === code);
});

When('I choose the deployment {string} in the Convex panel', async function (this: ThreadbearWorld, deployment: string) {
  // As a pick from its menu: the menu itself is a native one, which a script
  // can't reach.
  const out = await this.app.do({ cmd: 'webPanel', do: 'choose', deployment });
  assert.ok(!out.startsWith('error'), out);
});

When("the panel's page has the keyboard", async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'webPanel', do: 'focus' });
  await this.until('the page with the keyboard', (d) => (web(d)?.page as { hasKeyboard?: boolean } | undefined)?.hasKeyboard === true);
});

// Through AppKit, to whoever has the window's keyboard, as a person's keys
// go (the app's `native-keys`); a script's own keystrokes would go straight
// into the app and never meet a page.
When('I type {string} on the keyboard', async function (this: ThreadbearWorld, text: string) {
  await this.app.do({ cmd: 'native-keys', text });
});

// A text box of its own at the top of the page, with the page's focus, and
// the page given the keyboard as a click in that box gives it.
When("a box in the panel's page has the keyboard", async function (this: ThreadbearWorld) {
  const js = "(() => { const box = document.createElement('input'); box.id = 'typed-here'; document.body.prepend(box); box.focus(); return true; })()";
  const out = await this.app.do({ cmd: 'webPanel', do: 'eval', js });
  assert.ok(!out.startsWith('error'), out);
  await this.app.do({ cmd: 'webPanel', do: 'focus' });
  await this.until('the page with the keyboard', (d) => (web(d)?.page as { hasKeyboard?: boolean } | undefined)?.hasKeyboard === true);
});

Then("the box in the panel's page says {string}", async function (this: ThreadbearWorld, text: string) {
  const deadline = Date.now() + 10_000;
  for (;;) {
    const out = await this.app.do({ cmd: 'webPanel', do: 'eval', js: "document.getElementById('typed-here').value" });
    const value = (JSON.parse(okValue(out)) as { value?: string }).value ?? '';
    if (value === text) return;
    if (Date.now() > deadline) {
      const d = await this.app.dump();
      assert.fail(`the page's box says ${JSON.stringify(value)}; the keyboard is with ${String((d as { keyboard?: string }).keyboard)}; the composer says ${JSON.stringify(d.composer.text)}`);
    }
    await new Promise((done) => setTimeout(done, 200));
  }
});

Then('the app has the keyboard', async function (this: ThreadbearWorld) {
  await this.until("the app's own view with the keyboard", (d) => (d as { keyboard?: string }).keyboard === 'app');
});

When('I press ⌘A', async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'menu-key', key: 'a', modifiers: ['cmd'] });
});

Then("the page's own text is selected", async function (this: ThreadbearWorld) {
  const deadline = Date.now() + 10_000;
  for (;;) {
    const out = await this.app.do({ cmd: 'webPanel', do: 'eval', js: 'window.getSelection().toString()' });
    const selected = (JSON.parse(okValue(out)) as { value?: string }).value ?? '';
    if (selected.includes('A video on another site') && selected.includes('The video, in a new window')) return;
    if (Date.now() > deadline) {
      const d = await this.app.dump();
      const edits = (d as { webPanels?: { edits?: string[] } }).webPanels?.edits;
      const state = okValue(await this.app.do({ cmd: 'webPanel', do: 'eval', js: 'JSON.stringify({ focus: document.hasFocus(), ranges: getSelection().rangeCount, active: document.activeElement && document.activeElement.tagName })' }));
      assert.fail(`the page's selection is ${JSON.stringify(selected)}; the pages took ${JSON.stringify(edits)}; the page has the keyboard: ${String((web(d)?.page as { hasKeyboard?: boolean } | undefined)?.hasKeyboard)}; in the page: ${state}`);
    }
    await new Promise((done) => setTimeout(done, 200));
  }
});

Then('the runner was asked for a new deploy key for {string}', async function (this: ThreadbearWorld, deployment: string) {
  const id = await threadId(this);
  const deadline = Date.now() + 15_000;
  for (;;) {
    const out = await this.app.do({ cmd: 'mock', method: 'convexKeyRequests', params: { threadId: id } });
    const asked = (JSON.parse(okValue(out)) as { keyRequests: { deployment?: string; renew: boolean }[] }).keyRequests;
    if (asked.some((r) => r.deployment === deployment && r.renew)) return;
    if (Date.now() > deadline) assert.fail(`never asked to renew ${deployment}'s key: ${JSON.stringify(asked)}`);
    await new Promise((done) => setTimeout(done, 200));
  }
});
