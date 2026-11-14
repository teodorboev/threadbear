import assert from 'node:assert/strict';
import { Given, Then, When } from '@cucumber/cucumber';
import type { ThreadbearWorld } from '../support/world.ts';

When("the runner's next list includes malformed fields and rows", async function (this: ThreadbearWorld) {
  const d = await this.app.dump();
  const id = this.thread(d, 'Morning brief')?.id;
  assert.ok(id);
  await this.app.do({ cmd: 'mock', method: 'summaryList', params: { value: { threads: [
    { id, title: 'Recovered morning brief', cwd: '/tmp', status: 'idle', contextTokens: 'wrong type', sample: [] },
    false,
    { id: 42 },
  ] } } });
});

Then('the sidebar shows the recovered thread title', async function (this: ThreadbearWorld) {
  await this.until('the readable thread from the new list', (d) => Boolean(this.thread(d, 'Recovered morning brief')));
});

When('the runner restarts', async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'mock', method: 'restart', params: {} });
});

Then('the app is connected again', async function (this: ThreadbearWorld) {
  // The mock answers `restart` first and drops every connection about 100 ms
  // later, then stays down for a few seconds. A dump taken straight after the
  // step above still says "connected", so wait for the app to see the drop
  // before waiting for it to come back, or this passes without a reconnect.
  await this.until('the connection to drop', (d) => d.connection !== 'connected');
  await this.until('the app connected again', (d) => d.connection === 'connected', 30_000);
});

Then('the transcript still shows the thread {string}', async function (this: ThreadbearWorld, title: string) {
  await this.until(`"${title}" still open with its rows`, (d) => d.selection?.title === title && d.transcript.rows.length > 0);
});

When('the runner goes away without a word for {int} seconds', async function (this: ThreadbearWorld, seconds: number) {
  // No "restarting" notice first, as a runner that crashed or was stopped.
  await this.app.do({ cmd: 'mock', method: 'goAway', params: { downMs: seconds * 1000 } });
});

// The app has seen the connection go (the mock answers first and drops it a
// moment later), for what's done while it's away.
When('the app has lost the runner', async function (this: ThreadbearWorld) {
  await this.until('the connection to drop', (d) => d.connection !== 'connected');
});

Then('the bar says {string}', async function (this: ThreadbearWorld, text: string) {
  await this.until(`the bar saying "${text}"`, (d) => d.banner?.text === text && d.banner?.style === 'warning', 15_000);
});

Given("the thread {string} can't be read once", async function (this: ThreadbearWorld, title: string) {
  const d = await this.app.dump();
  const id = this.thread(d, title)?.id;
  if (!id) throw new Error(`no thread "${title}"`);
  await this.app.do({ cmd: 'mock', method: 'failEvents', params: { threadId: id, times: 1 } });
});

When('I open the thread {string}, which fails to load', async function (this: ThreadbearWorld, title: string) {
  const t = await this.until(`a thread called "${title}"`, (d) => this.thread(d, title));
  // The Swift app's `open` answers with an error when the thread doesn't
  // load, which is what this scenario is for: what it shows is checked next.
  await this.app.do({ cmd: 'open', id: t.id }).catch(() => undefined);
});

Then("the transcript says it couldn't be loaded", async function (this: ThreadbearWorld) {
  await this.until('the transcript failed', (d) => (d.transcript?.state ?? '').startsWith('failed:'));
});

When('I press Try again on the transcript', async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'click', tip: 'Load the thread again' });
});

Then('the transcript shows the thread {string}', async function (this: ThreadbearWorld, title: string) {
  await this.until(`"${title}" loaded with its rows`, (d) => d.selection?.title === title && d.transcript?.state === 'loaded' && d.transcript.rows.length > 0);
});

When('the runner restarts and is gone for {int} seconds', async function (this: ThreadbearWorld, seconds: number) {
  // With the "restarting now" notice first, as the real runner's restarts.
  await this.app.do({ cmd: 'mock', method: 'restart', params: { downMs: seconds * 1000 } });
});

Then('the transcript shows my message {string} just once', async function (this: ThreadbearWorld, words: string) {
  // Landed (no longer "waiting" or "sending"), and not sent twice.
  await this.until(`"${words}" landed`, (d) => d.transcript.rows.some((r) => r.text === words));
  await new Promise((r) => setTimeout(r, 1500));
  const again = await this.app.dump();
  const count = again.transcript.rows.filter((r) => typeof r.text === 'string' && r.text.endsWith(words)).length;
  assert.equal(count, 1, `"${words}" shows ${count} times: ${JSON.stringify(again.transcript.rows.slice(-4).map((r) => r.text))}`);
});

Given('the runner takes {int} seconds to answer each message', async function (this: ThreadbearWorld, seconds: number) {
  await this.app.do({ cmd: 'mock', method: 'slow', params: { ms: seconds * 1000 } });
});

When('the runner says it will restart once {string} finishes', async function (this: ThreadbearWorld, title: string) {
  const d = await this.app.dump();
  const id = this.thread(d, title)?.id;
  if (!id) throw new Error(`no thread "${title}"`);
  await this.app.do({ cmd: 'mock', method: 'restartPending', params: { waitingFor: [id] } });
});

Then('the bar says quietly {string}', async function (this: ThreadbearWorld, text: string) {
  await this.until(`the quiet bar saying "${text}"`, (d) => d.banner?.text === text && d.banner?.style === 'quiet', 15_000);
});

Then("the bar's tip says {string}", async function (this: ThreadbearWorld, tip: string) {
  await this.until(`the bar's tip "${tip}"`, (d) => d.banner?.detail === tip);
});

When('the runner warns {string}', async function (this: ThreadbearWorld, message: string) {
  await this.app.do({ cmd: 'mock', method: 'notice', params: { level: 'warning', message } });
});

When("the runner's update of Claude Code fails with {string}", async function (this: ThreadbearWorld, message: string) {
  await this.app.do({ cmd: 'mock', method: 'clis', params: { failed: { provider: 'claude', message } } });
});

When('I dismiss the bar', async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'click', tip: 'Hide this until it happens again' });
});

Then("the app's log says {string}", async function (this: ThreadbearWorld, words: string) {
  const deadline = Date.now() + 10_000;
  let log = '';
  while (Date.now() < deadline) {
    log = await this.app.appLog();
    if (log.includes(words)) return;
    await new Promise((r) => setTimeout(r, 250));
  }
  assert.fail(`the app's log doesn't say "${words}":\n${log.slice(-2000)}`);
});

/** Words only the connection's raw errors have. */
const RAW = /ws:\/\/|os error|IO error|Trying again in/;

function shown(d: Record<string, unknown>): string {
  return JSON.stringify({ banner: d.banner ?? null, error: d.error ?? null, notes: d.notes ?? [] });
}

Then("until it's back the window says only that the runner is restarting", async function (this: ThreadbearWorld) {
  const deadline = Date.now() + 30_000;
  for (;;) {
    const d = await this.app.dump();
    const text = d.banner?.text ?? '';
    assert.ok(!RAW.test(shown(d as unknown as Record<string, unknown>)), `the window showed a raw error mid-restart: ${shown(d as unknown as Record<string, unknown>)}`);
    assert.ok(!text || /restart/i.test(text), `the bar said "${text}" mid-restart`);
    if (d.connection === 'connected') return;
    assert.ok(Date.now() < deadline, 'the app never reconnected');
    await new Promise((r) => setTimeout(r, 200));
  }
});

Then('for {int} seconds there\'s no banner', async function (this: ThreadbearWorld, seconds: number) {
  const until = Date.now() + seconds * 1000;
  while (Date.now() < until) {
    const d = await this.app.dump();
    assert.ok(!d.banner, `the bar already says "${d.banner?.text}"`);
    await new Promise((r) => setTimeout(r, 200));
  }
});

Then("nothing in the window shows the connection's raw error", async function (this: ThreadbearWorld) {
  const d = await this.app.dump();
  const all = shown(d as unknown as Record<string, unknown>);
  assert.ok(!RAW.test(all), `raw error on show: ${all}`);
});
