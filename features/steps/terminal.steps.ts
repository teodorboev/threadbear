// The Terminal panel (terminal.feature). Both apps take the `terminal` command
// and say what the panel shows in the dump's `terminal`; the GPUI app's `keys`
// and `press` go through its keyboard and input method, the Swift app's to the
// shell as SwiftTerm's `send` does.
import { Given, Then, When } from '@cucumber/cucumber';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { okValue, type Dump } from '../support/driver.ts';
import { tempFolder } from '../support/temp.ts';
import type { ThreadbearWorld } from '../support/world.ts';

/** Each scenario's folders, by thread id, the threads' ids by title, and what it noted. */
const scenario = new WeakMap<ThreadbearWorld, { folders: Map<string, string>; ids: Map<string, string>; width?: number; cols?: number }>();

function state(world: ThreadbearWorld) {
  let s = scenario.get(world);
  if (!s) {
    s = { folders: new Map(), ids: new Map() };
    scenario.set(world, s);
  }
  return s;
}

/** The folder the open thread was given. */
function openFolder(world: ThreadbearWorld, d: Dump): string {
  const folder = d.selection ? state(world).folders.get(d.selection.id) : undefined;
  assert.ok(folder, 'the open thread has no folder of its own');
  return folder;
}

function screen(d: Dump): string[] {
  return d.terminal?.screen ?? [];
}

/** `until`, saying what the Terminal's screen had when it never came. */
async function untilScreen(world: ThreadbearWorld, what: string, check: (lines: string[], d: Dump) => boolean): Promise<void> {
  try {
    await world.until(what, (d) => check(screen(d), d));
  } catch (e) {
    const d = await world.app.dump();
    throw new Error(`${(e as Error).message.split('\n')[0]}\n\nThe Terminal (${d.terminal?.cols}x${d.terminal?.rows}, ${d.terminal?.shell} in ${d.terminal?.cwd}) showed:\n${screen(d).join('\n')}`);
  }
}

// A real folder, as the demo threads' are Mike's and on no other machine. A
// temp one, made for the scenario, so the shell never starts in anyone's.
Given('the thread {string} works in a folder of its own', async function (this: ThreadbearWorld, title: string) {
  const thread = await this.until(`a thread called "${title}"`, (d) => this.thread(d, title));
  const folder = tempFolder('tb-terminal-');
  state(this).folders.set(thread.id, folder);
  state(this).ids.set(title, thread.id);
  await this.app.do({ cmd: 'mock', method: 'terminal', params: { threadId: thread.id, cwd: folder } });
});

When('I press ⌘J', async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'menu-key', key: 'j', modifiers: ['cmd'] });
});

When('I follow the Terminal link {string}', async function (this: ThreadbearWorld, words: string) {
  await this.app.do({ cmd: 'terminal', follow: words });
});

// The empty panel and New Shell, by their tooltips' start, as each app's
// tip names the folder.
When('I start a shell from the empty Terminal', async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'click', tipPrefix: 'Start your login shell in ' });
});

When('I press New Shell in the Terminal', async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'click', tipPrefix: 'Start a new shell in ' });
});

When('I bring up the Terminal without starting a shell', async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'terminal', show: true });
  await this.until('the Terminal on show', (d) => d.terminal?.shown === true);
});

When('I type {string} and press Return in the Terminal', async function (this: ThreadbearWorld, text: string) {
  // As the feature file has it: `\033` and `\n` stay as the shell's printf reads them.
  await this.until('a shell running', (d) => d.terminal?.shell === 'running');
  // PowerShell formats `pwd` as a table, shortening a long path to `...`.
  if (process.platform === 'win32' && text === 'pwd') text = '(Get-Location).Path';
  await this.app.do({ cmd: 'terminal', keys: text });
  await this.app.do({ cmd: 'terminal', press: 'enter' });
});

When('I scroll the Terminal back {int} lines', async function (this: ThreadbearWorld, lines: number) {
  await this.app.do({ cmd: 'terminal', scroll: -lines });
});

When('I select everything in the Terminal and copy it', async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'terminal', select: 'all' });
  await this.app.do({ cmd: 'terminal', copy: true });
});

When('I paste {string} into the Terminal and press Return', async function (this: ThreadbearWorld, text: string) {
  await this.app.do({ cmd: 'terminal', paste: text });
  await this.app.do({ cmd: 'terminal', press: 'enter' });
});

// Two lines, each ending in a line break, as a block copied from a reply is.
When('I paste the lines {string} and {string} into the Terminal', async function (this: ThreadbearWorld, first: string, second: string) {
  await this.app.do({ cmd: 'terminal', paste: `${first}\n${second}\n` });
});

When('I note how many columns the Terminal has', async function (this: ThreadbearWorld) {
  // From a known width, so "wider" is the same step on both apps.
  state(this).width = 1000;
  await this.app.do({ cmd: 'resize', w: 1000, h: 800 });
  const cols = await this.until('the Terminal drawn at 1000 wide', (d) => d.terminal?.cols ?? undefined);
  // A frame or two for the grid to follow the window.
  await new Promise((done) => setTimeout(done, 400));
  state(this).cols = (await this.app.dump()).terminal?.cols ?? cols;
});

When('the window is made {int} points wider', async function (this: ThreadbearWorld, more: number) {
  const width = (state(this).width ?? 1000) + more;
  await this.app.do({ cmd: 'resize', w: width, h: 800 });
});

Then('the Terminal has more columns than before', async function (this: ThreadbearWorld) {
  const before = state(this).cols ?? 0;
  await this.until(`more than ${before} columns`, (d) => (d.terminal?.cols ?? 0) > before);
});

Then('the Terminal shows its own number of columns', async function (this: ThreadbearWorld) {
  // `tput cols` asks the pty, so this is the shell hearing the new size.
  await untilScreen(this, "tput cols answering the grid's width", (lines, d) => d.terminal?.cols != null && lines.includes(String(d.terminal.cols)));
});

Then("the Terminal is showing a shell in the thread's folder", async function (this: ThreadbearWorld) {
  const d0 = await this.app.dump();
  const folder = openFolder(this, d0);
  await this.until(`a shell running in ${folder}`, (d) => d.terminal?.shown && d.terminal.shell === 'running' && d.terminal.cwd === folder && !d.terminal.note);
});

Then('the Terminal has the keyboard', async function (this: ThreadbearWorld) {
  await this.until('the Terminal focused', (d) => d.terminal?.focused === true);
});

Then("the Terminal shows the thread's folder", async function (this: ThreadbearWorld) {
  const folder = openFolder(this, await this.app.dump());
  // PowerShell expands a short temp-directory alias such as RUNNER~1.
  const realFolder = fs.realpathSync.native(folder);
  // A long folder wraps onto the next line, so the screen is read whole.
  await untilScreen(this, `the folder "${realFolder}"`, (lines) => lines.join('').includes(realFolder));
});

Then('the Terminal shows the line {string}', async function (this: ThreadbearWorld, line: string) {
  await untilScreen(this, `the line "${line}"`, (lines) => lines.includes(line));
});

Then('the Terminal shows earlier lines and not the line {string}', async function (this: ThreadbearWorld, line: string) {
  await untilScreen(this, `earlier output, without "${line}"`, (lines) => !lines.includes(line) && lines.some((l) => /^\d+$/.test(l) && Number(l) < 350));
});

Then('the clipboard holds {string} among other lines', async function (this: ThreadbearWorld, words: string) {
  // The copy lands a moment after the key, on both apps.
  let text = '';
  for (let i = 0; i < 30; i++) {
    text = JSON.parse(okValue(await this.app.do({ cmd: 'clipboard' }))) as string;
    if (text.split('\n').some((l) => l.trim() === words)) return;
    await new Promise((done) => setTimeout(done, 100));
  }
  assert.fail(`the clipboard had ${JSON.stringify(text)}`);
});

Then('the Terminal is put away', async function (this: ThreadbearWorld) {
  await this.until('the Terminal hidden', (d) => d.terminal?.shown === false);
});

Then("the thread's shell is still running", async function (this: ThreadbearWorld) {
  const d0 = await this.app.dump();
  const id = d0.selection?.id;
  await this.until("the thread's shell running", (d) => d.terminal?.shells.some((s) => s.startsWith(`thread:${id} running`)));
});

Then('the shell of {string} has stopped', async function (this: ThreadbearWorld, title: string) {
  const id = state(this).ids.get(title);
  assert.ok(id, `"${title}" wasn't given a folder`);
  await this.until(`no shell for "${title}"`, (d) => !d.terminal?.shells.some((s) => s.startsWith(`thread:${id} `)));
});

// The process itself, by the pid the shell wrote in its folder, not just
// the panel's row for it.
Then('the shell that wrote {string} in the folder of {string} has ended', async function (this: ThreadbearWorld, file: string, title: string) {
  const id = state(this).ids.get(title);
  const folder = id ? state(this).folders.get(id) : undefined;
  assert.ok(folder, `"${title}" wasn't given a folder`);
  const pid = Number(fs.readFileSync(path.join(folder, file), 'utf8').trim());
  assert.ok(pid > 1, `no pid in ${file}`);
  // A shell ignoring SIGHUP takes the SIGTERM and SIGKILL that follow it.
  const deadline = Date.now() + 15_000;
  while (Date.now() < deadline) {
    try {
      process.kill(pid, 0);
    } catch {
      return;
    }
    await new Promise((done) => setTimeout(done, 200));
  }
  // Not left spinning on the test machine.
  process.kill(pid, 'SIGKILL');
  assert.fail(`the shell (pid ${pid}) was still running`);
});

// zsh, fish and modern bash ask for bracketed paste while they edit a line.
Then('the shell in the Terminal is at its prompt', async function (this: ThreadbearWorld) {
  await untilScreen(this, 'the shell at its prompt, pastes marked', (_lines, d) => d.terminal?.shell === 'running' && d.terminal.pastesMarked === true);
});

Then("the shell in the Terminal doesn't mark pastes", async function (this: ThreadbearWorld) {
  await untilScreen(this, 'a shell with pastes not marked', (_lines, d) => d.terminal?.shell === 'running' && d.terminal.pastesMarked === false);
});

When('an input method puts {string} and a line break in the Terminal at once',async function (this: ThreadbearWorld, text: string) {
  await this.app.do({ cmd: 'terminal', commit: `${text}\n` });
});

Then("the Terminal's prompt has {string} on it", async function (this: ThreadbearWorld, text: string) {
  await untilScreen(this, `"${text}" at the prompt`, (_lines, d) => promptLine(d).endsWith(text));
});

// As the app menu's Quit does once it's said yes, with the quit's own work.
When('the app quits', async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'quit' });
  await new Promise((done) => setTimeout(done, 1500));
});

Then('the Terminal says {string}', async function (this: ThreadbearWorld, note: string) {
  await this.until(`the note "${note}"`, (d) => d.terminal?.note === note);
});

Then('the Terminal offers to start a shell', async function (this: ThreadbearWorld) {
  await this.until('no shell yet, with Start a shell', (d) => d.terminal?.shown === true && d.terminal.shell === 'none');
});

Then('no shell is running', async function (this: ThreadbearWorld) {
  const d = await this.app.dump();
  assert.deepEqual(d.terminal?.shells ?? [], []);
});

Then('{int} shell(s) is/are running', async function (this: ThreadbearWorld, count: number) {
  await this.until(`${count} shells`, (d) => d.terminal?.shells.filter((s) => s.includes(' running in ')).length === count);
});

Then('the Terminal says the shell has exited', async function (this: ThreadbearWorld) {
  await this.until('the shell exited', (d) => d.terminal?.shell === 'exited');
});

// ---- Putting a reply's command in the Terminal (`terminal.putcommand`) ----

Given("the agent's last reply in {string} holds a bash block {string}", async function (this: ThreadbearWorld, title: string, command: string) {
  const thread = await this.until(`a thread called "${title}"`, (d) => this.thread(d, title));
  await this.app.do({ cmd: 'mock', method: 'terminal', params: { threadId: thread.id, block: command } });
});

Then('the reply offers to put {string} in the Terminal', async function (this: ThreadbearWorld, command: string) {
  await this.until(`a block offering to put "${command}"`, (d) => d.transcript.rows.some((r) => r.shellBlocks?.includes(`[put, copy] ${command}`)));
});

Then('the reply only offers to copy {string}', async function (this: ThreadbearWorld, command: string) {
  await this.until(`a block only offering to copy "${command}"`, (d) => d.transcript.rows.some((r) => r.shellBlocks?.includes(`[copy] ${command}`)));
});

// What the block's play icon does: both apps' `put` takes its path.
When('I put {string} in the Terminal from its block', async function (this: ThreadbearWorld, command: string) {
  await this.app.do({ cmd: 'terminal', put: command });
});

When('I press Return in the Terminal', async function (this: ThreadbearWorld) {
  await this.app.do({ cmd: 'terminal', press: 'enter' });
});

/**
 * The prompt's line: the last line on the screen with anything on it, as a
 * line of text rather than a row. A command after a long prompt wraps in a
 * narrow panel, as after GitHub's Linux machines'
 * `runner@runnervm7aw5e:/tmp/tb-terminal-SFc6Wj$ ` (11 Nov), and the last
 * row then has only its end. The GPUI app's dump says which rows are one
 * line (`lines`). The Swift app's doesn't, so there it's the last row with
 * any full rows above it (one a character short ended in a space the rows
 * have trimmed): a guess, but the Swift app only runs on Tinker Desk's short
 * prompt.
 */
function promptLine(d: Dump): string {
  const last = (lines: string[]) => {
    let end = lines.length - 1;
    while (end >= 0 && lines[end].trim() === '') end--;
    return end;
  };
  if (d.terminal?.lines) return d.terminal.lines[last(d.terminal.lines)] ?? '';
  const lines = screen(d);
  const cols = d.terminal?.cols;
  const end = last(lines);
  if (end < 0) return '';
  let line = lines[end];
  for (let i = end - 1; cols && i >= 0; i--) {
    if (lines[i].length === cols) line = lines[i] + line;
    else if (lines[i].length === cols - 1) line = `${lines[i]} ${line}`;
    else break;
  }
  return line;
}

Then("the Terminal's prompt has {string} waiting for Return", async function (this: ThreadbearWorld, command: string) {
  await untilScreen(this, `"${command}" at the prompt, Return to run it`, (_lines, d) => d.terminal?.waitingForReturn === true && promptLine(d).endsWith(command));
});

Then("the Terminal's prompt doesn't have {string}", async function (this: ThreadbearWorld, command: string) {
  // A moment for anything that was going to land to land.
  await new Promise((done) => setTimeout(done, 600));
  const d = await this.app.dump();
  assert.ok(!promptLine(d).includes(command), `the prompt has it: ${promptLine(d)}\n${screen(d).join('\n')}`);
});

Then("the Terminal doesn't show the line {string}", async function (this: ThreadbearWorld, line: string) {
  await new Promise((done) => setTimeout(done, 600));
  const d = await this.app.dump();
  assert.ok(!screen(d).includes(line), `it ran:\n${screen(d).join('\n')}`);
});

Then('{string} is copied instead', async function (this: ThreadbearWorld, command: string) {
  // A shell only just started gets ten seconds to reach its prompt before
  // it counts as busy, so this waits that long.
  const deadline = Date.now() + 20_000;
  let last = '';
  while (Date.now() < deadline) {
    last = okValue(await this.app.do({ cmd: 'clipboard' }));
    if (last.includes(command)) return;
    await new Promise((done) => setTimeout(done, 200));
  }
  assert.fail(`the clipboard has ${last}`);
});

When('the Terminal prints a link to a program in its folder', async function (this: ThreadbearWorld) {
  const folder = openFolder(this, await this.app.dump());
  const program = path.join(folder, 'terminal-link.command');
  fs.writeFileSync(program, 'touch "' + path.join(folder, 'link-ran') + '"\n', { mode: 0o700 });
  const uri = new URL('file://' + program).href;
  await this.app.do({ cmd: 'terminal', keys: "printf '\\033]8;;" + uri + "\\033\\\\Run the program\\033]8;;\\033\\\\\\n'" });
  await this.app.do({ cmd: 'terminal', press: 'enter' });
  await untilScreen(this, 'the labelled link', (lines) => lines.includes('Run the program'));
});

Then("the Terminal link's program did not run", async function (this: ThreadbearWorld) {
  assert.ok(!fs.existsSync(path.join(openFolder(this, await this.app.dump()), 'link-ran')));
});
