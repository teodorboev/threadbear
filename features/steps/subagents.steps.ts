// A sub-agent's own transcript (features/subagents.feature).
import { Given, Then, When } from '@cucumber/cucumber';
import assert from 'node:assert/strict';
import type { ThreadbearWorld } from '../support/world.ts';

const opened = new WeakMap<ThreadbearWorld, string>();

When('I open the sub-agent {string} of {string}', async function (this: ThreadbearWorld, title: string, thread: string) {
  const t = await this.until(`"${thread}" listing "${title}"`, (d) => {
    const row = this.thread(d, thread);
    return row?.subagents?.some((s) => s.title === title) ? row : undefined;
  });
  const sub = t.subagents!.find((s) => s.title === title)!;
  opened.set(this, await this.app.do({ cmd: 'subagent', thread: t.id, id: sub.id }));
});

Then("the transcript shows the sub-agent's brief {string}", async function (this: ThreadbearWorld, brief: string) {
  await this.until('the sub-agent open with its brief', (d) => Boolean(d.subagent) && d.transcript.rows.some((r) => r.kind === 'user' && r.text.startsWith(brief)));
});

When('I go back to the thread', async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'click', tip: 'Back to the thread' });
});

Then('the app says it has no record of that sub-agent', function (this: ThreadbearWorld) {
  assert.match(opened.get(this) ?? '', /no record/);
});

Given('the runner has no record of the sub-agent {string} of {string}', async function (this: ThreadbearWorld, title: string, thread: string) {
  const t = await this.until(`"${thread}" listing "${title}"`, (d) => {
    const row = this.thread(d, thread);
    return row?.subagents?.some((s) => s.title === title) ? row : undefined;
  });
  const sub = t.subagents!.find((s) => s.title === title)!;
  await this.app.do({ cmd: 'mock', method: 'forgetSubagent', params: { threadId: t.id, subagentId: sub.id } });
});
