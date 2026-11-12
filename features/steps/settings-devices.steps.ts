// Settings › Phone & browser (features/settings-devices.feature). Each paired
// device is a line as both apps' dumps give it, `browser "Chrome": Paired 22
// Oct, until 20 Jan 2027`; the dump never has a token in it.
import { Given, Then, When } from '@cucumber/cucumber';
import { sleep } from '../support/driver.ts';
import type { ThreadbearWorld } from '../support/world.ts';

type DevicesSection = { rows: string[]; heading?: string | null; note?: string | null; site?: string | null; loaded: boolean; pairing: boolean; error?: string | null };
const section = (d: unknown) => (d as { settingsWindow?: { devices?: DevicesSection | null } | null }).settingsWindow?.devices;
const named = (rows: string[] | undefined, name: string) => (rows ?? []).filter((r) => r.includes(` "${name}": `));

Then('Settings offers to add a phone or browser', async function (this: ThreadbearWorld) {
  await this.until('"Add a phone or browser"', (d) => section(d)?.heading === 'Add a phone or browser');
});

Then('Settings lists {string} as paired', async function (this: ThreadbearWorld, name: string) {
  await this.until(`${name} paired`, (d) => named(section(d)?.rows, name).length > 0);
});

Then("Settings doesn't list {string} as paired", async function (this: ThreadbearWorld, name: string) {
  await this.until(`${name} gone from the paired list`, (d) => section(d)?.loaded && named(section(d)?.rows, name).length === 0);
});

// The dates are the mock's, so only their shape is checked: "Paired 21 Oct,
// until 19 Jan 2027" this year, with the year once it isn't.
Then('Settings says when {string} was paired and until when', async function (this: ThreadbearWorld, name: string) {
  const line = /: Paired \d{1,2} [A-Z][a-z]{2}( \d{4})?, until \d{1,2} [A-Z][a-z]{2}( \d{4})?$/;
  await this.until(`when ${name} was paired`, (d) => named(section(d)?.rows, name).some((r) => line.test(r)));
});

Then('Settings says {string} has run out', async function (this: ThreadbearWorld, name: string) {
  const line = /: Expired \d{1,2} [A-Z][a-z]{2}( \d{4})?: pair it again$/;
  await this.until(`${name} run out`, (d) => named(section(d)?.rows, name).some((r) => line.test(r)));
});

Then('Settings lists {int} paired iPhones', async function (this: ThreadbearWorld, count: number) {
  await this.until(`${count} iPhones paired`, (d) => (section(d)?.rows ?? []).filter((r) => r.startsWith('phone "iPhone": ')).length === count);
});

// Revoke, Show the code… and Done, as their buttons do. A scripted click on
// the Swift app's AppKit form never answers, so both apps take these as the
// `settings` command's (the GPUI app clicks the button itself).
When('I revoke {string} in Settings', async function (this: ThreadbearWorld, name: string) {
  await this.app.do({ cmd: 'settings', revoke: name });
});

When('I ask Settings for the code to pair a phone', async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'settings', tab: 'devices', pair: true });
});

Then('Settings shows the pairing code', async function (this: ThreadbearWorld) {
  await this.until('the pairing code on show', (d) => section(d)?.pairing === true);
});

When('I close the pairing code', async function (this: ThreadbearWorld) {
  // Once it has come all the way in: both apps' sheets slide in, and a click
  // while one's still moving can land beside Done.
  await sleep(700);
  await this.app.do({ cmd: 'settings', pairDone: true });
});

Then("Settings doesn't show the pairing code", async function (this: ThreadbearWorld) {
  await this.until('the pairing code gone', (d) => section(d)?.pairing === false);
});

Then('Phone & browser says {string}', async function (this: ThreadbearWorld, words: string) {
  await this.until(`"${words}"`, (d) => [section(d)?.note, section(d)?.error].some((t) => t?.includes(words)));
});

Given("the backend won't answer the next pairing change", async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'mock', method: 'devices', params: { failNext: true } });
});

Given("the runner isn't mirroring threads because {string}", async function (this: ThreadbearWorld, reason: string) {
  await this.app.do({ cmd: 'mock', method: 'devices', params: { reason } });
});
