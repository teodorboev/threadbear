// The activity log (features/activity-log.feature): both apps' `activity`
// sets the Settings switch, gives the log a click on a control by its tip
// without pressing it, and says today's lines after " | "
// (ActivityRecorder.swift, desktop/gpui/src/activity.rs).
import { Given, Then, When } from '@cucumber/cucumber';
import assert from 'node:assert/strict';
import { sleep } from '../support/driver.ts';
import type { ThreadbearWorld } from '../support/world.ts';

type Line = { kind: string; target?: string; panel?: string; thread?: string; new?: boolean; window?: string };

/** What the log has today: "ok on 3 lines today | {...} | {...}". */
async function today(world: ThreadbearWorld): Promise<{ on: boolean; count: number; lines: Line[]; text: string }> {
  const out = await world.app.do({ cmd: 'activity', last: 200 });
  const [head, ...rest] = out.split(' | ');
  // The Swift app's answer has the command's name in front ("activity ok on …").
  const m = /\bok (on|off) (\d+) lines today/.exec(head);
  assert.ok(m, `the app said ${out}`);
  return { on: m[1] === 'on', count: Number(m[2]), lines: rest.map((l) => JSON.parse(l) as Line), text: rest.join('\n') };
}

/** Waits for a line that passes `check`: the writer and a key's line each take a moment. */
async function line(world: ThreadbearWorld, what: string, check: (l: Line) => boolean, ms = 10_000): Promise<Line> {
  const deadline = Date.now() + ms;
  for (;;) {
    const { lines } = await today(world);
    const found = lines.find(check);
    if (found) return found;
    if (Date.now() > deadline) throw new Error(`The activity log never had ${what}. It has:\n${lines.map((l) => JSON.stringify(l)).join('\n')}`);
    await sleep(250);
  }
}

Given('the activity log is on', async function (this: ThreadbearWorld) {
  const out = await this.app.do({ cmd: 'activity', on: true });
  assert.match(out, /\bok on\b/);
});

Given('the activity log is off', async function (this: ThreadbearWorld) {
  const out = await this.app.do({ cmd: 'activity', on: false });
  assert.match(out, /\bok off\b/);
});

When('I click the control with the tip {string}, for the activity log', async function (this: ThreadbearWorld, tip: string) {
  await this.app.do({ cmd: 'activity', click: tip });
});

Then('the activity log has a click on {string} in the {word} panel', async function (this: ThreadbearWorld, tip: string, panel: string) {
  const l = await line(this, `a click on "${tip}"`, (x) => x.kind === 'click' && x.target === tip);
  assert.equal(l.panel, panel);
  assert.equal(l.window, 'main');
});

When('the Settings window comes to the front, for the activity log', async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'activity', window: 'settings' });
});

Then('the activity log has the window {string}', async function (this: ThreadbearWorld, name: string) {
  await line(this, `the window "${name}" coming to the front`, (x) => x.kind === 'window' && x.window === name);
});

Then('the activity log has a message sent in {string}', async function (this: ThreadbearWorld, title: string) {
  const d = await this.app.dump();
  const id = this.thread(d, title)?.id;
  assert.ok(id, `no thread called "${title}"`);
  await line(this, `a send in "${title}"`, (x) => x.kind === 'send' && x.thread === id && !x.new);
});

Then('the activity log has nothing of {string}', async function (this: ThreadbearWorld, words: string) {
  const { text } = await today(this);
  assert.ok(!text.includes(words), `the log has the words: ${text}`);
});

Then('the activity log has nothing from today', async function (this: ThreadbearWorld) {
  // Long enough for a line to have been written, were one coming.
  await sleep(500);
  const { on, count } = await today(this);
  assert.equal(on, false);
  assert.equal(count, 0);
});
