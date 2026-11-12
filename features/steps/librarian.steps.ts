// The Librarian view and its page (features/librarian.feature).
import { Then } from '@cucumber/cucumber';
import type { ThreadbearWorld } from '../support/world.ts';

/** The librarian's state in words, and the page in place of a thread, from either app's dump. */
type Extra = { librarian?: string | null; homePage?: { title: string } | null };

Then('the librarian says {string}', async function (this: ThreadbearWorld, words: string) {
  await this.until(`the librarian saying "${words}"`, (d) => (d as unknown as Extra).librarian === words);
});

Then('the Librarian page is in place of the thread', async function (this: ThreadbearWorld) {
  await this.until('the Librarian page', (d) => (d as unknown as Extra).homePage?.title === 'Librarian');
});

Then("the activity bar's {string} button counts {int}", async function (this: ThreadbearWorld, title: string, count: number) {
  await this.until(`${title} counting ${count} in the activity bar`, (d) => d.rails?.activity.find((b) => b.title === title)?.badge === count);
});
