// Steps more than one feature uses. Files starting with _ don't own a
// feature (features/README.md).
import { Given, Then, When } from '@cucumber/cucumber';
import assert from 'node:assert/strict';
import type { ThreadbearWorld } from '../support/world.ts';

Given('the app is connected to a runner with the demo threads', async function (this: ThreadbearWorld) {
  await this.until('the app connected, with its threads listed', (d) => d.connection === 'connected' && d.sidebar.sections.some((s) => s.threads.length > 0), 30_000);
});

When('I open the thread {string}', async function (this: ThreadbearWorld, title: string) {
  await this.open(title);
});

// `then.starts` is on a dump's row text, which the Swift app cuts short.
Then('the transcript shows my message {string}', async function (this: ThreadbearWorld, words: string) {
  await this.until(`my message "${words}"`, (d) => d.transcript.rows.some((r) => r.kind === 'user' && r.text.startsWith(words)));
});

Then('the transcript doesn\'t show my message {string}', async function (this: ThreadbearWorld, words: string) {
  const d = await this.app.dump();
  assert.equal(d.transcript.rows.some((r) => r.kind === 'user' && r.text.startsWith(words)), false, 'it was sent');
});

Then('the transcript shows a reply starting {string}', async function (this: ThreadbearWorld, words: string) {
  await this.until(`a reply starting "${words}"`, (d) => d.transcript.rows.some((r) => r.kind === 'assistant' && r.text.startsWith(words)));
});

// The activity bar's Runners, Updates and Connectors panels (runners,
// updates and connectors.feature, and rails.feature for their buttons). Each
// panel's lines are what it draws, as both apps' dumps give them.
const panelKey = (name: string) => name.toLowerCase() as 'runners' | 'updates' | 'connectors';

When('I click {string} at the foot of the activity bar', async function (this: ThreadbearWorld, title: string) {
  // Connectors and Runners are only there once the runner has said, which can
  // come after the threads. Their tooltips start with the title, then a line
  // on what's going on ("Runners\nBoth online").
  await this.until(`${title} in the activity bar`, (d) => d.rails?.activity.some((b) => b.title === title));
  await this.app.do({ cmd: 'click', tipPrefix: `${title}\n` });
});

When('I open the {word} panel', async function (this: ThreadbearWorld, name: string) {
  await this.until(`${name} in the activity bar`, (d) => d.rails?.activity.some((b) => b.title === name));
  await this.app.do({ cmd: 'click', tipPrefix: `${name}\n` });
  await this.until(`the ${name} panel on show`, (d) => d[panelKey(name)]?.shown === true);
});

Then('the {word} panel is open', async function (this: ThreadbearWorld, name: string) {
  await this.until(`the ${name} panel on show`, (d) => d[panelKey(name)]?.shown === true);
});

Then('the {word} panel is closed', async function (this: ThreadbearWorld, name: string) {
  await this.until(`the ${name} panel gone`, (d) => d[panelKey(name)]?.shown === false);
});

Then('the {word} panel lists {string}', async function (this: ThreadbearWorld, name: string, line: string) {
  await this.until(`"${line}" in the ${name} panel`, (d) => d[panelKey(name)]?.lines.includes(line));
});

When('I press {string} in the {word} panel', async function (this: ThreadbearWorld, tip: string, name: string) {
  await this.until(`the ${name} panel on show`, (d) => d[panelKey(name)]?.shown === true);
  await this.app.do({ cmd: 'click', tipPrefix: tip });
});

// What the mock runner says about the gateway and its own update
// (mock.connectors and mock.updates, app/Tools/mock-runner/server.mjs).

When("the gateway can't be reached", async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'mock', method: 'connectors', params: { state: 'unreachable', error: 'timed out' } });
});

When('every service on the gateway is signed in', async function (this: ThreadbearWorld) {
  const services = [['notion', 'Notion'], ['sentry', 'Sentry'], ['cloudflare', 'Cloudflare'], ['deepwiki', 'DeepWiki']].map(([id, title]) => ({ id, title, state: 'connected' }));
  await this.app.do({ cmd: 'mock', method: 'connectors', params: { state: 'ok', services } });
});

When('the runner says its launcher has changed', async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'mock', method: 'updates', params: { state: 'idle', launcherChanged: true } });
});

When("the runner says Threadbear Runner.app is behind main", async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'mock', method: 'updates', params: { state: 'idle', launcherChanged: true, launcherReason: "Threadbear Runner.app was built from older launcher sources than main's." } });
});

When('the runner says it will reinstall itself once no thread is busy', async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'mock', method: 'updates', params: { state: 'idle', launcherChanged: true, reinstall: { state: 'waiting', auto: true } } });
});
