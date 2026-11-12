// Settings › Keyboard (features/settings-keyboard.feature). Each command is a
// line as both apps' dumps give it: "Archive: ⌘K (changed)", "Hibernate: none".
// Commands go by their names in shortcuts.json ("archive", "newThread").
import { Then, When } from '@cucumber/cucumber';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { okValue } from '../support/driver.ts';
import type { ThreadbearWorld } from '../support/world.ts';

type KeyboardSection = { rows: string[]; listening?: string | null; question?: string | null; canTake: boolean; problem?: string | null; canResetAll: boolean };
const section = (d: unknown) => (d as { settingsWindow?: { keyboard?: KeyboardSection | null } | null }).settingsWindow?.keyboard;

Then('Settings lists {string} under Keyboard', async function (this: ThreadbearWorld, line: string) {
  await this.until(`"${line}" on the Keyboard page`, (d) => section(d)?.rows.includes(line));
});

Then("Settings doesn't list {string} under Keyboard", async function (this: ThreadbearWorld, line: string) {
  await this.until(`"${line}" gone from the Keyboard page`, (d) => section(d) && !section(d)?.rows.includes(line));
});

// A click on its key, which then listens for the new one (the `shortcuts`
// command): the GPUI app clicks it, and the Swift app, whose form a script
// can't click, starts its recorder as the click does.
When('I click the key for {string} in Settings', async function (this: ThreadbearWorld, command: string) {
  await this.app.do({ cmd: 'shortcuts', record: command });
});

Then('Settings is listening for a new key for {string}', async function (this: ThreadbearWorld, title: string) {
  await this.until(`${title} listening`, (d) => section(d)?.listening === title);
});

Then("Settings isn't listening for a key", async function (this: ThreadbearWorld) {
  await this.until('nothing listening', (d) => section(d) && !section(d)?.listening);
});

// Keys as shortcuts.json writes them ("cmd+k"). Esc and Delete on their own
// go as the keyboard sends them, by their key codes, as only a real key press
// reaches the recorder's own handling of them.
const keyCodes: Record<string, { keyCode: number; characters: string }> = {
  escape: { keyCode: 53, characters: '\u001b' },
  delete: { keyCode: 51, characters: '\u007f' },
};
When('I press the keys {string} in Settings', async function (this: ThreadbearWorld, keys: string) {
  const post = keyCodes[keys];
  await this.app.do(post ? { cmd: 'shortcuts', post: { ...post, modifiers: [] } } : { cmd: 'shortcuts', press: keys });
});

Then('Settings asks {string}', async function (this: ThreadbearWorld, question: string) {
  await this.until(`"${question}"`, (d) => section(d)?.question === question);
});

When('I use the key for this one instead in Settings', async function (this: ThreadbearWorld) {
  await this.until('a key it can take', (d) => section(d)?.canTake === true);
  await this.app.do({ cmd: 'shortcuts', answer: 'use' });
});

When('I reset the key for {string} in Settings', async function (this: ThreadbearWorld, command: string) {
  await this.app.do({ cmd: 'shortcuts', reset: command });
});

When('I reset all the keys in Settings', async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'shortcuts', reset: 'all' });
});

Then('Settings has nothing to reset', async function (this: ThreadbearWorld) {
  await this.until('Reset All with nothing to do', (d) => section(d)?.canResetAll === false);
});

// As an agent part way through editing it would leave it.
When('shortcuts.json holds {string}', async function (this: ThreadbearWorld, text: string) {
  const dir = okValue(await this.app.do({ cmd: 'data-dir' }));
  assert.ok(fs.existsSync(dir), `no data folder: ${dir}`);
  fs.writeFileSync(path.join(dir, 'shortcuts.json'), text);
});

Then('Keyboard says {string}', async function (this: ThreadbearWorld, problem: string) {
  await this.until(`"${problem}" under the keys`, (d) => section(d)?.problem === problem);
});

When('I search the keys for {string}', async function (this: ThreadbearWorld, words: string) {
  await this.app.do({ cmd: 'shortcuts', search: words });
});
