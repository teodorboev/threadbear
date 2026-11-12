// The Usage panel and the activity bar's honey pot (features/usage.feature).
// Both apps' dumps say what each part of the panel draws, under `usage`
// (app/README.md, the `usage` command).
import { Then, When } from '@cucumber/cucumber';
import assert from 'node:assert/strict';
import type { Dump } from '../support/driver.ts';
import type { ThreadbearWorld } from '../support/world.ts';

type Login = { name: string; provider?: string | null; status?: string | null; meters: { title: string; left: string; verdict?: string | null }[]; more: string[] };
type Usage = {
  shown: boolean;
  icon: { percent: number; login?: string | null; tip: string };
  tiles: { title: string; value: string; detail?: string | null }[];
  chart?: { title: string; range: string; key: string[]; empty?: string | null } | null;
  cards: { title: string; today?: string | null; logins: Login[] }[];
  settings?: { switch: string; on: boolean } | null;
};

const usage = (d: Dump) => (d as unknown as { usage?: Usage }).usage;
const login = (d: Dump, card: string, name: string) => usage(d)?.cards.find((c) => c.title === card)?.logins.find((l) => l.name === name);
const list = (words: string) => words.split(',').map((w) => w.trim());

/** A mock call's answer, past whatever the app puts before it ("ok", or nothing). */
const answer = (out: string) => JSON.parse(out.slice(out.indexOf('{')));

When('I click the honey pot in the activity bar', async function (this: ThreadbearWorld) {
  // Its tooltip is "Usage", then a line for each pool once the logins have
  // come with the runner's hello.
  await this.until('the honey pot saying each pool\'s room', (d) => usage(d)?.icon.tip.includes('\n'));
  await this.app.do({ cmd: 'click', tipPrefix: 'Usage\n' });
});

// "the Usage panel is open" is _common's `the {word} panel is open`, which
// reads the dump's `usage.shown` as it does the other panels'.

Then("the Usage panel's tiles are {string}, each with a number", async function (this: ThreadbearWorld, titles: string) {
  // The numbers come with `usage.stats`, which the panel asks for as it opens.
  const tiles = await this.until('the Usage panel\'s tiles', (d) => (usage(d)?.tiles.length ? usage(d)?.tiles : undefined));
  assert.deepEqual(tiles.map((t) => t.title), list(titles));
  for (const t of tiles) assert.match(t.value, /^\d+(\.\d)?[kMB]?$/, `${t.title} says ${t.value}`);
});

Then('the Usage chart is {string}', async function (this: ThreadbearWorld, title: string) {
  await this.until(`the Usage chart "${title}"`, (d) => usage(d)?.chart?.title === title);
});

When('I show the last 7 days on the Usage chart', async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'usage', range: 'week' });
});

Then("the Usage chart's key names {string}", async function (this: ThreadbearWorld, names: string) {
  const key = await this.until('the Usage chart\'s key', (d) => usage(d)?.chart?.key);
  assert.deepEqual(key, list(names));
});

Then('the Usage panel has a card for each pool: {string}', async function (this: ThreadbearWorld, titles: string) {
  const cards = await this.until('the Usage panel\'s cards', (d) => (usage(d)?.cards.length ? usage(d)?.cards : undefined));
  assert.deepEqual(cards.map((c) => c.title), list(titles));
});

Then('in the {string} card, {string} has {string}', async function (this: ThreadbearWorld, card: string, name: string, meters: string) {
  const want = list(meters).join(', ');
  const got = (d: Dump) => login(d, card, name)?.meters.map((m) => `${m.title} ${m.left}`).join(', ');
  await this.until(`${name} in ${card} with ${want}`, (d) => got(d) === want).catch(async (e) => {
    throw new Error(`${(e as Error).message.split('\n')[0]}: it has ${got(await this.app.dump()) ?? 'no row there'}`);
  });
});

Then('in the {string} card, {string} says {string}', async function (this: ThreadbearWorld, card: string, name: string, status: string) {
  await this.until(`${name} in ${card} saying ${status}`, (d) => login(d, card, name)?.status === status);
});

Then("the {string} card names each login's provider: {string}", async function (this: ThreadbearWorld, card: string, logins: string) {
  const got = await this.until(`the ${card} card`, (d) => usage(d)?.cards.find((c) => c.title === card)?.logins);
  assert.deepEqual(got.map((l) => `${l.provider}: ${l.name}`), list(logins));
});

Then("the Usage panel's {string} switch is {word}", async function (this: ThreadbearWorld, words: string, state: string) {
  await this.until(`"${words}" ${state}`, (d) => usage(d)?.settings?.switch === words && usage(d)?.settings?.on === (state === 'on'));
});

When('I turn off {string} in the Usage panel', async function (this: ThreadbearWorld, words: string) {
  await this.until(`a "${words}" switch`, (d) => usage(d)?.settings?.switch === words);
  await this.app.do({ cmd: 'usage', showModelWeeks: false });
});

Then('the honey pot is {int}% full, for {string}', async function (this: ThreadbearWorld, percent: number, account: string) {
  await this.until(`the honey pot ${percent}% full for ${account}`, (d) => usage(d)?.icon.percent === percent && usage(d)?.icon.login === account);
});

Then("the honey pot's tooltip says {string}", async function (this: ThreadbearWorld, line: string) {
  await this.until(`"${line}" in the honey pot's tooltip`, (d) => usage(d)?.icon.tip.split('\n').includes(line));
});

/** Sets what's left of a login's week on the mock, which tells the app as the runner does (`accounts.updated`). */
async function setWeek(world: ThreadbearWorld, account: string, left: number) {
  const { accounts } = answer(await world.app.do({ cmd: 'mock', method: 'accounts', params: {} })) as { accounts: { id: string; usage?: { windows: { kind: string; remainingPercent: number }[] } }[] };
  const windows = accounts.find((a) => a.id === account)?.usage?.windows;
  const week = windows?.find((w) => w.kind === 'weekly');
  assert.ok(week, `${account} has no week on the mock`);
  week.remainingPercent = left;
  await world.app.do({ cmd: 'mock', method: 'accounts', params: { accounts } });
}

When('{string} is down to {int}% of its week', async function (this: ThreadbearWorld, account: string, left: number) {
  await setWeek(this, account, left);
});

When("{string}'s week resets", async function (this: ThreadbearWorld, account: string) {
  await setWeek(this, account, 100);
});
