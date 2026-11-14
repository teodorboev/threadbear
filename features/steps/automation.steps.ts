import { Then, When } from '@cucumber/cucumber';
import assert from 'node:assert/strict';
import type { ThreadbearWorld } from '../support/world.ts';

When('I give that thread a Convex panel', async function (this: ThreadbearWorld) {
  const id = (await this.app.dump()).selection?.id;
  assert.ok(id);
  await this.app.do({ cmd: 'mock', method: 'convexPanel', params: { threadId: id } });
  await this.until('the Convex panel', (d) => (d as unknown as { panels?: { items?: { id: string }[] } }).panels?.items?.some((p) => p.id === 'convex.dashboard'));
});

When('I select the production deployment through the script', async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'convex', deployment: 'busy-bear-456' });
});

Then('the script reports the production Convex dashboard', async function (this: ThreadbearWorld) {
  const result = await this.app.do({ cmd: 'convex' });
  assert.match(String(result), /^ok busy-bear-456 browser=https:\/\/dashboard\.convex\.dev\//);
});

Then('the script refuses an unknown deployment', async function (this: ThreadbearWorld) {
  await assert.rejects(this.app.do({ cmd: 'convex', deployment: 'not-offered-000' }), /unknown deployment/);
  assert.match(String(await this.app.do({ cmd: 'convex' })), /^ok busy-bear-456 /);
});

When('I size the preview window to {int} by {int}', async function (this: ThreadbearWorld, width: number, height: number) {
  await this.app.do({ cmd: 'size', w: width, h: height });
});

Then('the script reports a window id and size {int} by {int}', async function (this: ThreadbearWorld, width: number, height: number) {
  const result = await this.app.do({ cmd: 'window' });
  assert.match(String(result), new RegExp('^ok [0-9]+ ' + width + 'x' + height + '$'));
});

When('I click Librarian by its coordinates', async function (this: ThreadbearWorld) {
  const result = await this.app.do({ cmd: 'label-point', label: 'Librarian', prefix: true });
  const match = /^ok ([-0-9.]+) ([-0-9.]+)$/.exec(String(result));
  assert.ok(match, String(result));
  await this.app.do({ cmd: 'point', x: Number(match[1]), y: Number(match[2]) });
});

When('I prepare anonymous logins with {int} percent room', async function (this: ThreadbearWorld, percent: number) {
  await this.app.do({ cmd: 'account-ids' });
  await this.app.do({ cmd: 'room', percent });
});

Then('the screenshot logins have anonymous names and {int} percent room', async function (this: ThreadbearWorld, percent: number) {
  const usage = await this.app.do({ cmd: 'usage' });
  const text = typeof usage === 'string' ? usage : JSON.stringify(usage);
  assert.ok(!text.includes('@'), text);
  assert.ok(text.includes(`${percent}%`), text);
});

Then('the script can read the streaming text', async function (this: ThreadbearWorld) {
  const result = await this.app.do({ cmd: 'streaming' });
  const text = typeof result === 'string' ? result.replace(/^ok /, '') : JSON.stringify(result);
  assert.equal(typeof JSON.parse(text), 'object');
});

Then("the script can copy a row's debug information", async function (this: ThreadbearWorld) {
  const dump = await this.app.dump();
  const row = dump.transcript.rows[0];
  assert.ok(row);
  const result = await this.app.do({ cmd: 'debug-info', row: row.id });
  assert.ok(String(result).includes(row.id), String(result));
});
