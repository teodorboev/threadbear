// The Librarian view and its page (features/librarian.feature).
import { Then, When } from '@cucumber/cucumber';
import type { ThreadbearWorld } from '../support/world.ts';

/** The librarian's state in words, and the page in place of a thread, from either app's dump. */
type Extra = { librarian?: string | null; homePage?: { title: string } | null };

When('I scroll to the Librarian settings', async function (this: ThreadbearWorld) {
  const geometry = await this.app.do({ cmd: 'window' });
  const size = / ([0-9]+)x([0-9]+)$/.exec(String(geometry));
  if (!size) throw new Error(`No window size: ${geometry}`);
  await this.app.do({ cmd: 'point', x: Number(size[1]) / 2, y: Number(size[2]) / 2, scrollY: -100 });
  await this.app.dump();
});

When('I follow a project in the librarian log', async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'click', tip: 'Open the project' });
});

Then("the librarian's project page is open", async function (this: ThreadbearWorld) {
  await this.until('the project page', (d) => Boolean((d as unknown as { project?: { title: string } }).project?.title));
});

Then('the librarian says {string}', async function (this: ThreadbearWorld, words: string) {
  await this.until(`the librarian saying "${words}"`, (d) => (d as unknown as Extra).librarian === words);
});

Then('the Librarian page is in place of the thread', async function (this: ThreadbearWorld) {
  await this.until('the Librarian page', (d) => (d as unknown as Extra).homePage?.title === 'Librarian');
});

Then("the activity bar's {string} button counts {int}", async function (this: ThreadbearWorld, title: string, count: number) {
  await this.until(`${title} counting ${count} in the activity bar`, (d) => d.rails?.activity.find((b) => b.title === title)?.badge === count);
});
