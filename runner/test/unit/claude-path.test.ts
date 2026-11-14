// Where the runner finds Claude Code (config.ts, claudePath): the native
// install in ~/.local/bin first, then the runner's PATH, so an npm or
// Homebrew install works too. Until 13 Nov 2026 only ~/.local/bin/claude
// counted, and a fork with Claude from npm saw "the claude CLI isn't at
// ~/.local/bin/claude" though INSTALL.md said the PATH's would do
// (jayehzzz's fork).
import { test, type TestContext } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { loadConfig } from '../../src/config.ts';
import { tmpDir } from '../helpers/fakes.ts';

function executable(file: string, mode = 0o755): string {
  fs.mkdirSync(path.dirname(file), { recursive: true });
  fs.writeFileSync(file, '#!/bin/sh\nexit 0\n', { mode });
  return file;
}

// A home of its own, so this Mac's real ~/.local/bin/claude never counts.
function fakeHome(t: TestContext): string {
  const home = fs.realpathSync(tmpDir());
  t.mock.method(os, 'homedir', () => home);
  return home;
}

const load = (home: string, env: Record<string, string>) => loadConfig({ THREADBEAR_HOME: path.join(home, 'data'), ...env });

test('no native install: a claude on the PATH, kept as the link npm or Homebrew made', (t) => {
  const home = fakeHome(t);
  const bin = path.join(home, 'homebrew', 'bin');
  const target = executable(path.join(home, 'homebrew', 'lib', 'node_modules', '@anthropic-ai', 'claude-code', 'cli.js'));
  fs.mkdirSync(bin, { recursive: true });
  fs.symlinkSync(path.relative(bin, target), path.join(bin, 'claude'));
  // The first PATH folder has no claude; the second does.
  assert.equal(load(home, { PATH: `${path.join(home, 'empty')}:${bin}` }).claudePath, path.join(bin, 'claude'));
});

test('the native install comes before the PATH, and claudePath or THREADBEAR_CLAUDE_PATH before both', (t) => {
  const home = fakeHome(t);
  const native = executable(path.join(home, '.local', 'bin', 'claude'));
  const bin = path.dirname(executable(path.join(home, 'npm', 'bin', 'claude')));
  assert.equal(load(home, { PATH: bin }).claudePath, native);
  const own = executable(path.join(home, 'own', 'claude'));
  assert.equal(load(home, { PATH: bin, THREADBEAR_CLAUDE_PATH: own }).claudePath, own);
  fs.mkdirSync(path.join(home, 'data'), { recursive: true });
  fs.writeFileSync(path.join(home, 'data', 'runner.json'), JSON.stringify({ claudePath: own }));
  assert.equal(load(home, { PATH: bin }).claudePath, own);
});

test("a relative PATH folder, a file that won't run and a folder named claude don't count; with none, the native path", (t) => {
  const home = fakeHome(t);
  // A thread's folder must never supply the runner's CLI.
  executable(path.join(home, 'rel', 'claude'));
  const was = process.cwd();
  process.chdir(home);
  t.after(() => process.chdir(was));
  const notRunnable = path.dirname(executable(path.join(home, 'plain', 'claude'), 0o644));
  const dir = path.join(home, 'dir');
  fs.mkdirSync(path.join(dir, 'claude'), { recursive: true });
  const broken = path.join(home, 'broken');
  fs.mkdirSync(broken);
  fs.symlinkSync(path.join(home, 'gone'), path.join(broken, 'claude'));
  const cfg = load(home, { PATH: ['rel', notRunnable, dir, broken].join(':') });
  // Said missing as before: the notice names where it looked first.
  assert.equal(cfg.claudePath, path.join(home, '.local', 'bin', 'claude'));
});
