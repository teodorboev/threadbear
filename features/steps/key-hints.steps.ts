// The keys shown beside the buttons while ⌘ is held. A script can't hold a
// key down in the app, so both apps' `hints` command sets them as ⌘ would.
import { Then, When } from '@cucumber/cucumber';
import type { ThreadbearWorld } from '../support/world.ts';

When('I hold ⌘', async function (this: ThreadbearWorld) {
  // The activity bar's views come once the runner has said it has them.
  await this.until('the activity bar filled in', (d) => (d.rails?.activity.length ?? 0) >= 4);
  await this.app.do({ cmd: 'hints', on: true });
});

When('I let go of ⌘', async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'hints', on: false });
});

Then('the keys show beside the buttons: {string}', async function (this: ThreadbearWorld, list: string) {
  const wanted = list.split('|').map((s) => s.trim()).filter(Boolean);
  await this.until(`the keys ${wanted.join(', ')}`, (d) => wanted.every((w) => d.keyHints?.includes(w)));
});

Then('no keys show beside the buttons', async function (this: ThreadbearWorld) {
  await this.until('no keys showing', (d) => (d.keyHints ?? []).length === 0);
});
