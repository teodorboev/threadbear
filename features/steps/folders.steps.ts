import { Given, Then, When } from '@cucumber/cucumber';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { tempFolder } from '../support/temp.ts';
import type { ThreadbearWorld } from '../support/world.ts';
import { okValue } from '../support/driver.ts';
import { pane, previewLine } from '../support/pane.ts';

// `folder`'s answer ends with the names; its summary reads "(list, 5 rows,
// sort name up, rows drawn 5): docs, logs, ...".
const summary = (d: Parameters<typeof pane>[0]) => pane(d)?.folder ?? '';
const names = (d: Parameters<typeof pane>[0]) => summary(d).split('): ').slice(1).join('): ').split(', ').filter(Boolean);

async function showFiles(world: ThreadbearWorld) {
  await world.app.do({ cmd: 'panel', id: 'threadbear.files' });
  await world.until('the folder browser to be showing', (d) => pane(d)?.section === 'threadbear.files' && names(d).length > 0);
}

// The demo thread's own folder sits in the mock runner's temp folder, which
// is deep on a Mac (/private/var/folders/...) but /tmp/tb-spec-... on Linux,
// short enough for the path bar to show whole, so nothing was tucked behind
// its … there (11 Nov). A folder made deep, in a temp folder of its own.
Given('the thread {string} works in a folder too deep for the path bar to show whole', async function (this: ThreadbearWorld, title: string) {
  const thread = await this.until(`a thread called "${title}"`, (d) => this.thread(d, title));
  const top = tempFolder('tb-folders-');
  const folder = path.join(top, 'a-folder-of-its-own', 'with-more-folders', 'above-the-one', 'it-works-in');
  fs.mkdirSync(folder, { recursive: true });
  fs.writeFileSync(path.join(folder, 'notes.md'), 'made by the scenario\n');
  await this.app.do({ cmd: 'mock', method: 'terminal', params: { threadId: thread.id, cwd: folder } });
  // Open, so its composer says when the app has the new folder.
  await this.open(title);
  await this.until(`the thread working in ${folder}`, (d) => d.composer.cwd?.replace(/\/$/, '') === folder || undefined);
});

Given('the thread\'s folder is showing in the pane for {string}', async function (this: ThreadbearWorld, title: string) {
  await this.open(title);
  await showFiles(this);
});

Then('the folder lists {string} first', async function (this: ThreadbearWorld, list: string) {
  const wanted = list.split(', ');
  await this.until(`the folder listing ${list}`, (d) => wanted.every((name, i) => names(d)[i] === name));
});

Then('the folder says it\'s sorted by {string}', async function (this: ThreadbearWorld, how: string) {
  await this.until(`the folder sorted by ${how}`, (d) => summary(d).includes(`sort ${how}`));
});

When('I sort the folder by {string} descending', async function (this: ThreadbearWorld, key: string) {
  await this.app.do({ cmd: 'folder', sort: key, ascending: false });
});

When('I expand the folder row {string}', async function (this: ThreadbearWorld, name: string) {
  await this.app.do({ cmd: 'folder', expand: name });
});

Then('the folder shows {int} rows in all', async function (this: ThreadbearWorld, rows: number) {
  await this.until(`${rows} rows drawn`, (d) => summary(d).includes(`rows drawn ${rows}`));
});

When('I double-click the folder row {string}', async function (this: ThreadbearWorld, name: string) {
  await this.until(`a row called ${name}`, (d) => names(d).includes(name));
  await this.app.do({ cmd: 'folder', open: name });
});

Then('the pane is browsing the folder {string}', async function (this: ThreadbearWorld, name: string) {
  await this.until(`the pane browsing ${name}`, (d) => previewLine(d)?.startsWith('folder ') && previewLine(d)?.includes(`/${name}`));
});

Then('the pane shows the thread\'s folder again', async function (this: ThreadbearWorld) {
  await this.until('the thread\'s folder to be showing again', (d) => names(d)[0] === 'docs' && !previewLine(d)?.includes('/logs'));
});

Then('the path bar ends with {string}', async function (this: ThreadbearWorld, words: string) {
  const answer = await this.app.do({ cmd: 'folder', pathBar: true });
  assert.ok(answer.endsWith(words), `path bar reads: ${answer}`);
});

When('I right-click the folder row {string}', async function (this: ThreadbearWorld, name: string) {
  const answer = await this.app.do({ cmd: 'folder', menu: name });
  this.menuTitles = answer.replace(/^(\w+ )?ok /, '').split(' | ');
});

Then('the folder menu offers {string}', function (this: ThreadbearWorld, list: string) {
  const items = this.menuTitles ?? [];
  for (const item of list.split(' | ')) {
    // A rule the Mac app draws, and Windows' or Linux's word for Finder.
    const found = items.some((i) => i === item || (item === 'Reveal in Finder' && /^(Reveal in Finder|Show in folder)$/.test(i)) || (item === 'Open in Finder' && /^(Open in Finder|Open in its app)$/.test(i)));
    assert.ok(found, `no "${item}" in: ${items.join(' | ')}`);
  }
});

// The thread's folder on disk: the mock runner's own, under its temp data
// folder, so a scenario may make and take away files there.
async function threadFolder(world: ThreadbearWorld): Promise<string> {
  const d = await world.until('the composer to know its folder', (x) => (x.composer as unknown as { cwd?: string }).cwd && x);
  return (d.composer as unknown as { cwd: string }).cwd.replace(/\/$/, '');
}

When('a file {string} is made in the thread\'s folder', async function (this: ThreadbearWorld, name: string) {
  fs.writeFileSync(path.join(await threadFolder(this), name), 'made by the scenario\n');
});

When('the file {string} is taken out of the thread\'s folder', async function (this: ThreadbearWorld, name: string) {
  fs.rmSync(path.join(await threadFolder(this), name), { force: true });
});

Given('a hidden file {string} is in the folder of {string}', async function (this: ThreadbearWorld, name: string, title: string) {
  await this.open(title);
  fs.writeFileSync(path.join(await threadFolder(this), name), 'SECRET=not-really\n');
});

Then('the folder lists {string}', async function (this: ThreadbearWorld, name: string) {
  await this.until(`the folder listing ${name}`, (d) => names(d).includes(name), 20_000);
});

Then('the folder no longer lists {string}', async function (this: ThreadbearWorld, name: string) {
  await this.until(`the folder without ${name}`, (d) => names(d).length > 0 && !names(d).includes(name), 20_000);
});

Then('the folder doesn\'t list {string}', async function (this: ThreadbearWorld, name: string) {
  await this.until(`the folder listed, without ${name}`, (d) => names(d).length > 0 && !names(d).includes(name));
});

When('I pick the folder row {string}', async function (this: ThreadbearWorld, name: string) {
  await this.until(`a row called ${name}`, (d) => names(d).includes(name));
  await this.app.do({ cmd: 'folder', select: name });
});

When('I add the folder row {string} to the picked rows', async function (this: ThreadbearWorld, name: string) {
  await this.app.do({ cmd: 'folder', selectAlso: name });
});

Then('the folder has {string} picked', async function (this: ThreadbearWorld, picked: string) {
  await this.until(`the folder with ${picked} picked`, (d) => summary(d).includes(`selected ${picked})`) || summary(d).includes(`selected ${picked},`));
});

// The Mac's key codes, which both apps' `folder` command takes.
const keys: Record<string, { key: number; characters: string; command?: boolean; shift?: boolean }> = {
  return: { key: 36, characters: '\r' },
  'cmd-up': { key: 126, characters: '\u{F700}', command: true },
  // A real keyboard's ⇧. types ">": with "." a made-up key matched the
  // composer's ⌘. (Stop) in the Swift app first.
  'cmd-shift-.': { key: 47, characters: '>', command: true, shift: true },
};
const letterCodes: Record<string, number> = { a: 0, s: 1, d: 2, f: 3, h: 4, g: 5, z: 6, x: 7, c: 8, v: 9, b: 11, q: 12, w: 13, e: 14, r: 15, y: 16, t: 17, o: 31, u: 32, i: 34, p: 35, l: 37, j: 38, k: 40, n: 45, m: 46 };

When('I press {string} in the folder', async function (this: ThreadbearWorld, name: string) {
  const press = keys[name];
  assert.ok(press, `no key called ${name}`);
  await this.app.do({ cmd: 'folder', ...press });
});

When('I type {string} in the folder', async function (this: ThreadbearWorld, letters: string) {
  await this.until('the folder\'s rows', (d) => names(d).length > 0);
  for (const letter of letters) {
    await this.app.do({ cmd: 'folder', key: letterCodes[letter] ?? 0, characters: letter });
  }
});

Then('Space would look at {string}', async function (this: ThreadbearWorld, name: string) {
  const answer = await this.app.do({ cmd: 'folder', quickLook: true });
  assert.ok(answer.includes(`/${name}"`), `Space would look at: ${answer}`);
});

When('I choose {string} from the folder\'s menu', async function (this: ThreadbearWorld, title: string) {
  await this.until('the folder\'s rows', (d) => names(d).length > 0);
  await this.app.do({ cmd: 'folder', choose: title });
});

Then('the folder is showing as {string}', async function (this: ThreadbearWorld, mode: string) {
  await this.until(`the folder as ${mode}`, (d) => summary(d).startsWith(`(${mode},`));
});

Then('the path bar\'s … menu starts with the folder above the first one it shows', async function (this: ThreadbearWorld) {
  const folder = await threadFolder(this);
  const bar = okValue(await this.app.do({ cmd: 'folder', pathBar: true }));
  const menu = okValue(await this.app.do({ cmd: 'folder', pathMenu: true })).split(' | ').filter(Boolean);
  assert.ok(bar.startsWith('… > '), `the path bar has nothing tucked: ${bar}`);
  const firstShown = bar.split(' > ')[1];
  const parts = folder.replaceAll('\\', '/').split('/');
  const above = parts[parts.lastIndexOf(firstShown) - 1];
  assert.equal(menu[0], above, `the … menu: ${menu.join(' | ')}, path bar: ${bar}`);
  this.lastAnswer = menu[menu.length - 1];
});

When('I pick the last folder in the path bar\'s … menu', async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'folder', crumb: this.lastAnswer });
});

Then('the pane is browsing that folder', async function (this: ThreadbearWorld) {
  const name = this.lastAnswer ?? '';
  // The top of the disk is "/" itself, which no other path ends with.
  const at = (d: Parameters<typeof previewLine>[0]) => previewLine(d)?.split(' ')[1] ?? '';
  await this.until(`the pane browsing ${name}`, (d) => previewLine(d)?.startsWith('folder ') && (name === '/' ? at(d) === '/' : at(d).endsWith(`/${name}`)));
});
