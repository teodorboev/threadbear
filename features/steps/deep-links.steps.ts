// threadbear:// links handed to the app as macOS hands them over (the
// automation's `open-url`, through the same steps as a real one).
import { When } from '@cucumber/cucumber';
import type { ThreadbearWorld } from '../support/world.ts';

When('a threadbear link to {string} comes from outside the app', async function (this: ThreadbearWorld, title: string) {
  const t = await this.until(`a thread called "${title}"`, (d) => this.thread(d, title));
  await this.app.do({ cmd: 'open-url', url: `threadbear://thread/${t.id}` });
});

When('the link {string} comes from outside the app', async function (this: ThreadbearWorld, url: string) {
  await this.app.do({ cmd: 'open-url', url });
});
