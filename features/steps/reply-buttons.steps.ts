import assert from 'node:assert/strict';
import { Then, When } from '@cucumber/cucumber';
import type { ThreadbearWorld } from '../support/world.ts';
import type { Dump } from '../support/driver.ts';

/** Without text-style marks (U+FE0E), which a feature file may or may not keep. */
const plain = (s: string) => s.replace(/[\uFE0E\uFE0F]/g, '');
/** Every pill the open thread's replies show, in order. */
const pills = (d: Dump) => d.transcript.rows.flatMap((r) => r.buttons ?? []).map(plain);

Then('the reply\'s buttons read {string}, {string}, {string} and {string}', async function (this: ThreadbearWorld, a: string, b: string, c: string, e: string) {
  for (const want of [a, b, c, e].map(plain)) await this.until(`a pill "${want}"`, (d) => pills(d).includes(want));
});

Then('the reply has an unknown button', async function (this: ThreadbearWorld) {
  await this.until('an unknown button', (d) => pills(d).some((p) => p.startsWith('unknown ')));
});

When('I press the reply button {string}', async function (this: ThreadbearWorld, title: string) {
  await this.until(`the pill "${title}"`, (d) => pills(d).some((p) => p.includes(` ${title} [`)));
  const r = await this.app.do({ cmd: 'press-button', title });
  assert.ok(!r.includes('error'), r);
});

Then('the reply button {string} reads {string}', async function (this: ThreadbearWorld, title: string, want: string) {
  await this.until(`"${title}" reading ${want}`, (d) => pills(d).includes(plain(want)), 15_000);
});
