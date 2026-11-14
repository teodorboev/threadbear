import { Then } from '@cucumber/cucumber';
import type { ThreadbearWorld } from '../support/world.ts';

// The GPUI app's dump says which row a click last slid off and which card
// last arrived in the box while its thread was on screen (`juice`,
// row_swipe.rs and composer_card.rs).

Then('the row {string} slid off', async function (this: ThreadbearWorld, title: string) {
  const t = await this.until(`a thread called "${title}"`, (d) => this.thread(d, title));
  await this.until(`"${title}" sliding off`, (d) => d.juice?.slidOff === t.id);
});

Then('the card that came settled in', async function (this: ThreadbearWorld) {
  await this.until('a card arriving in the box', (d) => typeof d.juice?.cardArrived === 'number', 20_000);
});
