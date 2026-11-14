import { Given, Then, When } from '@cucumber/cucumber';
import assert from 'node:assert/strict';
import { sleep } from '../support/driver.ts';
import type { ThreadbearWorld } from '../support/world.ts';

async function drawn(world: ThreadbearWorld, label: string) {
  let error: unknown;
  for (let tries = 0; tries < 30; tries += 1) {
    try { await world.app.do({ cmd: 'label-point', label }); return; }
    catch (e) { error = e; await sleep(120); }
  }
  throw error;
}

Given('the backup thread has a phone message with only a picture', async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'mock', method: 'phoneOnlyPicture', params: { threadId: 'bruce_backup' } });
});

Then('the phone origin mark is drawn', async function (this: ThreadbearWorld) {
  await drawn(this, 'Sent from the phone app');
});

Then('no phone origin mark is drawn', async function (this: ThreadbearWorld) {
  await assert.rejects(this.app.do({ cmd: 'label-point', label: 'Sent from the phone app' }));
});

When('I press the phone origin mark', async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'click', tip: 'Sent from the phone app' });
});

Then('the phone origin explanation is drawn', async function (this: ThreadbearWorld) {
  await drawn(this, 'Phone message origin');
});
