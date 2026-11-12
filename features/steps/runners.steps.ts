// The Runners panel (features/runners.feature). Its lines are what it draws:
// the summary first, then a line a runner ("bruce: Online, up 1d, 51ae514",
// with " *" after the name where new threads start), and under each, indented,
// its figures while its details are open and the threads running on it.
import { Then, When } from '@cucumber/cucumber';
import assert from 'node:assert/strict';
import type { Dump } from '../support/driver.ts';
import type { ThreadbearWorld } from '../support/world.ts';

/** The runner's own line and the indented ones under it. */
function under(d: Dump, name: string): string[] | undefined {
  const lines = d.runners?.lines ?? [];
  const at = lines.findIndex((l) => l.startsWith(`${name}: `) || l.startsWith(`${name} *: `));
  if (at < 0) return undefined;
  const rest = lines.slice(at + 1);
  const end = rest.findIndex((l) => !l.startsWith('  '));
  return rest.slice(0, end < 0 ? rest.length : end).map((l) => l.slice(2));
}

Then('the Runners panel says {string}', async function (this: ThreadbearWorld, summary: string) {
  await this.until(`the Runners panel saying "${summary}"`, (d) => d.runners?.lines[0] === summary);
});

Then('the Runners panel shows {string} online on {string}', async function (this: ThreadbearWorld, name: string, build: string) {
  // How long it's been up moves on as the scenario runs: a number of
  // seconds, minutes, hours or days.
  const line = new RegExp(`^${name}( \\*)?: Online, up \\d+[smhd], ${build}$`);
  await this.until(`${name} online on ${build}`, (d) => d.runners?.lines.some((l) => line.test(l)));
});

When("I open bruce's details in the Runners panel", async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'click', tipPrefix: "Show bruce's CPU, memory and disk" });
});

When("I close bruce's details in the Runners panel", async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'click', tipPrefix: "Hide bruce's details" });
});

// Asked for now, as the panel does every 10 s while its window can be seen,
// which a scripted window often can't be.
When('the runners send their figures', async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'runners', stats: true });
});

Then('the Runners panel shows {string} under {string}', async function (this: ThreadbearWorld, line: string, name: string) {
  await this.until(`"${line}" under ${name}`, (d) => under(d, name)?.includes(line));
});

Then('the Runners panel shows no figures under {string}', async function (this: ThreadbearWorld, name: string) {
  await this.until(`no figures under ${name}`, (d) => {
    const lines = under(d, name);
    return lines !== undefined && !lines.some((l) => l.startsWith('CPU ') || l.startsWith('Memory ') || l.startsWith('Disk '));
  });
});

When('I drag {string} onto {string} in the Runners panel', async function (this: ThreadbearWorld, name: string, onto: string) {
  await this.app.do({ cmd: 'runners', drag: name, onto });
});

Then('the Runners panel lists {string} before {string}', async function (this: ThreadbearWorld, first: string, second: string) {
  const at = (d: Dump, name: string) => d.runners?.lines.findIndex((l) => l.startsWith(`${name}: `) || l.startsWith(`${name} *: `)) ?? -1;
  await this.until(`${first} before ${second}`, (d) => at(d, first) >= 0 && at(d, second) >= 0 && at(d, first) < at(d, second));
  // And it stays there once the runner has saved it.
  await new Promise((done) => setTimeout(done, 1500));
  const d = await this.app.dump();
  assert.ok(at(d, first) < at(d, second), `the order went back: ${d.runners?.lines.join(' | ')}`);
});
