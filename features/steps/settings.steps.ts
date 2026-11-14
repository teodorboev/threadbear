// The Settings window (features/settings.feature).
import { Then, When } from '@cucumber/cucumber';
import type { ThreadbearWorld } from '../support/world.ts';

/** The Settings window while it's open, from either app's dump. */
type SettingsInfo = { tab: string; keepAwake?: boolean | null; fullSync?: boolean | null; privateFolders?: string[] | null };
const shown = (d: unknown) => (d as { settingsWindow?: SettingsInfo | null }).settingsWindow;

When('I open Settings', async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'settings' });
  await this.until('the Settings window', (d) => shown(d));
});

When('I open Settings on {string}', async function (this: ThreadbearWorld, tab: string) {
  await this.app.do({ cmd: 'settings', tab });
  await this.until(`Settings on ${tab}`, (d) => shown(d)?.tab === tab);
});

Then('Settings shows {string}', async function (this: ThreadbearWorld, tab: string) {
  await this.until(`Settings on ${tab}`, (d) => shown(d)?.tab === tab);
});

When('I press {string} in Settings', async function (this: ThreadbearWorld, tip: string) {
  // Tall enough for every section of General: a switch scrolled out of sight
  // can't be clicked, by a script any more than by a person (Sleep went below
  // the window's 620 points as Quitting, Notifications and Instructions came in).
  await this.app.do({ cmd: 'resize', settings: true, w: 880, h: 1300 });
  await this.app.do({ cmd: 'click', tipPrefix: tip, settings: true });
});

Then('Settings says the Mac is kept awake', async function (this: ThreadbearWorld) {
  await this.until('Keep the Mac awake on', (d) => shown(d)?.keepAwake === true);
});

Then("Settings says the Mac isn't kept awake", async function (this: ThreadbearWorld) {
  await this.until('Keep the Mac awake off', (d) => shown(d)?.keepAwake === false);
});

Then('Settings says full sync is off', async function (this: ThreadbearWorld) {
  await this.until('Full sync off', (d) => shown(d)?.fullSync === false);
});

Then('Settings says full sync is on', async function (this: ThreadbearWorld) {
  await this.until('Full sync on', (d) => shown(d)?.fullSync === true);
});

Then('Settings lists the private folder {string}', async function (this: ThreadbearWorld, folder: string) {
  await this.until(`${folder} kept private`, (d) => shown(d)?.privateFolders?.includes(folder));
});

Then('Settings lists no private folders', async function (this: ThreadbearWorld) {
  await this.until('no private folders', (d) => shown(d)?.privateFolders?.length === 0);
});

// Where the keyboard is in the Settings window: its list of sections, as it opens.
When('I press the {word} arrow in Settings', async function (this: ThreadbearWorld, dir: string) {
  await this.app.do({ cmd: 'arrow', dir, settings: true });
});
