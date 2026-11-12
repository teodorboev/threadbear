// Bringing the Swift app's settings across (features/swift-import.feature):
// THREADBEAR_SWIFT_DEFAULTS names a property list for the GPUI app to read in
// place of the Swift app's own defaults, and the Swift app's drafts go in the
// data folder, as it keeps them (desktop/gpui/src/swift_import.rs).
import { After, Given, Then, When } from '@cucumber/cucumber';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import type { ThreadbearWorld } from '../support/world.ts';

After({ tags: '@system.swiftimport' }, () => {
  delete process.env.THREADBEAR_SWIFT_DEFAULTS;
});

Given('the Swift app had its zoom at {int}%, the Inbox newest first, and {string} in the box of {string}', async function (this: ThreadbearWorld, zoom: number, words: string, title: string) {
  const d = await this.until(`a thread called "${title}"`, (x) => this.thread(x, title) && x);
  const id = this.thread(d, title)!.id;
  const home = (await this.app.do({ cmd: 'data-dir' })).replace(/^.*?ok /, '').trim();
  const plist = `<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>inboxOrder</key>
	<string>newest</string>
	<key>textZoomPercent</key>
	<integer>${zoom}</integer>
</dict>
</plist>
`;
  const file = path.join(fs.mkdtempSync(path.join(fs.realpathSync(os.tmpdir()), 'tb-swift-defaults-')), 'com.mikerosoft.threadbear.plist');
  fs.writeFileSync(file, plist);
  process.env.THREADBEAR_SWIFT_DEFAULTS = file;
  fs.writeFileSync(path.join(home, 'app-drafts.json'), JSON.stringify({ [id]: { text: words, images: [] } }));
});

When('the app opens again', async function (this: ThreadbearWorld) {
  await this.app.relaunch();
  await this.until('the app connected, with its threads listed', (d) => d.connection === 'connected' && d.sidebar.sections.some((s) => s.threads.length > 0), 30_000);
});

Then("the app's zoom is {int}%", async function (this: ThreadbearWorld, zoom: number) {
  await this.until(`the zoom at ${zoom}%`, (d) => d.zoom === zoom);
});

Then('the Inbox is sorted newest first', async function (this: ThreadbearWorld) {
  await this.until('the Inbox newest first', (d) => d.sidebar.sort === 'newest');
});

Then('a note says {string}', async function (this: ThreadbearWorld, words: string) {
  await this.until(`the note "${words}"`, (d) => (d as unknown as { importNote?: string | null }).importNote === words);
});

Then('no note says what came across from the Swift app', async function (this: ThreadbearWorld) {
  const d = await this.app.dump();
  assert.equal((d as unknown as { importNote?: string | null }).importNote ?? null, null);
});
