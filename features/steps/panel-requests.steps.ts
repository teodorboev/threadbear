// An agent's show_panel (features/panel-requests.feature), through the
// mock's showPanel and panelState (docs/protocol.md, "Showing panels").
import { Then, When } from '@cucumber/cucumber';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { highlightsFetching, previewLine } from '../support/pane.ts';
import type { ThreadbearWorld } from '../support/world.ts';

const pinned = new WeakMap<ThreadbearWorld, string>();

const requested = new WeakMap<ThreadbearWorld, { threadId: string; requestId: string }>();

// As the agent's pin tool with show: a panel request naming the pin.
When('the agent of {string} pinned a file of its own and showed it', async function (this: ThreadbearWorld, title: string) {
  const threadId = (await this.until(`a thread called "${title}"`, (d) => this.thread(d, title)))?.id;
  assert.ok(threadId, `no thread called "${title}"`);
  const file = path.join(fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'tb-pin-'))), 'shown-notes.md');
  fs.writeFileSync(file, '# Notes\n\nShown by the agent.\n');
  pinned.set(this, file);
  await this.app.do({ cmd: 'mock', method: 'pin', params: { threadId, path: file, show: true } });
  const state = await this.app.do({ cmd: 'mock', method: 'panelState', params: { threadId } });
  const request = JSON.parse(state.replace(/^(\w+ )?ok /, '')).request;
  assert.ok(request, 'the pin has no pending show request');
  requested.set(this, { threadId, requestId: request.id });
});

Then('the pane is showing the pinned file', async function (this: ThreadbearWorld) {
  const file = pinned.get(this);
  assert.ok(file, 'nothing was pinned');
  await this.until(`the pane to show ${path.basename(file)}`, (d) => previewLine(d)?.endsWith(path.basename(file)), 30_000);
  fs.rmSync(path.dirname(file), { recursive: true, force: true });
});

When('the agent continues replying while the pinned file loads', async function (this: ThreadbearWorld) {
  const threadId = (await this.app.dump()).selection?.id;
  assert.ok(threadId, 'no thread open');
  await this.app.do({ cmd: 'mock', method: 'replaceReply', params: { threadId, text: 'The pinned notes are ready.' } });
  await this.until('the updated reply', (d) => d.transcript.rows.some((r) => r.text === 'The pinned notes are ready.'));
});

When('the agent unpins that file before I look', async function (this: ThreadbearWorld) {
  const { threadId } = requested.get(this)!;
  await this.app.do({ cmd: 'mock', method: 'unpin', params: { threadId, highlightId: `file:${pinned.get(this)}` } });
});

When('the agent keeps working while I open the file', async function (this: ThreadbearWorld) {
  const { threadId } = requested.get(this)!;
  await this.app.do({ cmd: 'mock', method: 'stream', params: { threadId, seconds: 60, hz: 10, tools: true } });
});

When('the next two highlights fetches will fail', async function (this: ThreadbearWorld) {
  const { threadId } = requested.get(this)!;
  await this.app.do({ cmd: 'mock', method: 'failHighlights', params: { threadId, count: 2 } });
});

When('I look away until the failed highlights fetch stops', async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'focus', on: false });
  for (let i = 0; i < 150; i += 1) {
    if ((await panelState(this)).highlightsFailed > 0 && !highlightsFetching(await this.app.dump())) return;
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  assert.fail('the failed highlights fetch did not stop while away');
});

Then('the pane has no file preview', async function (this: ThreadbearWorld) {
  assert.equal(previewLine(await this.app.dump()), undefined);
  const file = pinned.get(this)!;
  fs.rmSync(path.dirname(file), { recursive: true, force: true });
});

When("I'm looking at the app", async function (this: ThreadbearWorld) {
  // A scripted app is never in front: this says Mike is looking.
  await this.app.do({ cmd: 'focus' });
});

async function askToShow(world: ThreadbearWorld, panel: string, reload: boolean): Promise<void> {
  const d = await world.app.dump();
  const threadId = d.selection?.id;
  assert.ok(threadId, 'no thread open');
  const out = await world.app.do({ cmd: 'mock', method: 'showPanel', params: { threadId, panel, ...(reload ? { reload: true } : {}) } });
  const requestId = JSON.parse(out.replace(/^(\w+ )?ok /, '')).request.id as string;
  requested.set(world, { threadId, requestId });
}

When('the agent asks to show the {string} panel', async function (this: ThreadbearWorld, panel: string) {
  await askToShow(this, panel, false);
});

// show_panel with reload: an agent that just changed the panel's page.
When('the agent asks to show the {string} panel, reloaded', async function (this: ThreadbearWorld, panel: string) {
  await askToShow(this, panel, true);
});

Then('the dock has the {string} panel', async function (this: ThreadbearWorld, panel: string) {
  await this.until(`the ${panel} panel in the dock`, (d) => new RegExp(`\\b${panel}\\b`).test(d.dock ?? ''));
});

async function panelState(world: ThreadbearWorld): Promise<{ request: { id: string } | null; shown: string[]; highlightsFailed: number }> {
  const asked = requested.get(world);
  assert.ok(asked, 'no panel was asked for');
  const out = await world.app.do({ cmd: 'mock', method: 'panelState', params: { threadId: asked.threadId } });
  return JSON.parse(out.replace(/^(\w+ )?ok /, ''));
}

Then('the runner heard the panel was shown', async function (this: ThreadbearWorld) {
  const asked = requested.get(this)!;
  for (let i = 0; i < 200; i += 1) {
    const state = await panelState(this);
    if (state.shown.includes(asked.requestId) || state.request === null) return;
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  assert.fail('the app never said it showed the panel');
});

Then('the runner still has the request waiting', async function (this: ThreadbearWorld) {
  const asked = requested.get(this)!;
  const state = await panelState(this);
  assert.equal(state.request?.id, asked.requestId, 'the request went');
  assert.ok(!state.shown.includes(asked.requestId), 'the app said it showed a panel the thread hasn\'t got');
});
