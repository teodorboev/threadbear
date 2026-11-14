import { When, Then } from '@cucumber/cucumber';
import assert from 'node:assert/strict';
import type { ThreadbearWorld } from '../support/world.ts';

When('the thread count reaches {int} with {int} already remembered', async function (this: ThreadbearWorld, count: number, shown: number) {
  await this.app.do({ cmd: 'milestone', count, shown });
});

When('the thread count reaches {int} while I am away', async function (this: ThreadbearWorld, count: number) {
  await this.app.do({ cmd: 'milestone', count, shown: 100, active: false });
});

When('I return to see the milestone', async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'milestone', count: 1000, active: true });
});

Then('the milestone says {string}', async function (this: ThreadbearWorld, words: string) {
  await this.until(words, (d) => (d as { milestoneNote?: string }).milestoneNote === words);
  await this.app.do({ cmd: 'label-point', label: words });
});

Then('no milestone is showing', async function (this: ThreadbearWorld) {
  await new Promise((done) => setTimeout(done, 600));
  assert.ok(!(await this.app.dump() as { milestoneNote?: string }).milestoneNote);
});

When('I dismiss the milestone', async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'click', tip: 'Dismiss milestone' });
});
