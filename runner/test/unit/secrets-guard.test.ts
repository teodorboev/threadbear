// The secrets guard (src/secrets-guard.ts) on spellings of the runner's data folder that name
// it without the words the guard refuses on sight. None of these commands contains the token
// file's name (the one test of its letter case builds it from parts), so each is held back, or
// let through, by the spelling of the folder alone. They use only forms that mean the same
// thing on macOS, Linux and Windows, so they run the same on all three.
import { after, test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { denySecrets } from '../../src/secrets-guard.ts';
import { threadEnv } from '../../src/restart.ts';

const home = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'tb-guard-')));
after(() => fs.rmSync(home, { recursive: true, force: true }));
const deny = (command: string) => denySecrets('Bash', { command }, home);

test('every variable a thread is given for the data folder names it, plain or in braces', () => {
  const given = Object.entries(threadEnv('th1', home))
    .filter(([, value]) => value === home)
    .map(([name]) => name);
  // If a new one is added to threadEnv (restart.ts), the guard has to know it too.
  assert.ok(given.includes('THREADBEAR_HOME') && given.includes('THREADBEAR_RUNNER_HOME'), given.join(' '));
  for (const name of given) {
    for (const command of [
      `cat $${name}/secrets/convex-deploy.env`,
      `cat "\${${name}}/secrets/convex-deploy.env"`,
      `cd $${name}/secrets`,
      // A wildcard after the folder reaches the same files without the word.
      `cat $${name}/secr*/*`,
      `cat "\${${name}}"/s?crets/*`,
    ]) assert.ok(deny(command), command);
  }
});

test('a path is the same path in other letters, as the file system sees it', () => {
  // APFS as it is usually made, and NTFS, treat these as the same folder. The
  // data folder here is a temp one, so the last three are held by the guard's
  // looser `Threadbear/secrets` form, which is compared in lower case now too.
  for (const command of [
    'cat $THREADBEAR_HOME/SECRETS/convex-deploy.env',
    'cat "${THREADBEAR_RUNNER_HOME}/Secrets/convex-deploy.env"',
    'cat ~/Library/Application\\ Support/THREADBEAR/Secrets/convex-deploy.env',
    'cat ~/library/application\\ support/threadbear/secrets/convex-deploy.env',
    'cat "$HOME/Library/Application Support/Threadbear/SECRETS/convex-deploy.env"',
  ]) assert.ok(deny(command), command);
});

test('the token file is the token file in other letters too', () => {
  // Built from parts so the file's other tests don't contain it.
  const token = ['Runner', 'Token'].join('-');
  assert.ok(deny(`cat ${token}`), token);
  assert.ok(deny(`cat ${token.toUpperCase()}`), token.toUpperCase());
});

test("a file tool's path is compared as the file system does, in any letters", () => {
  const upper = home.toUpperCase();
  assert.ok(denySecrets('Read', { file_path: path.join(upper, 'SECRETS', 'convex-deploy.env') }, home), 'Read');
  assert.ok(denySecrets('Edit', { file_path: path.join(upper, 'Secrets', 'convex-deploy.env') }, home), 'Edit');
  assert.ok(denySecrets('Grep', { path: upper, pattern: 'x' }, home), 'Grep from the data folder itself');
  assert.ok(denySecrets('Glob', { path: path.dirname(home), pattern: `${path.basename(home).toUpperCase()}/SECRETS/*` }, home), 'Glob that names it');
});

test('ordinary commands and paths that only look near it are left alone', () => {
  for (const command of [
    'ls secrets',
    'ls Secrets',
    'git status',
    'git show runner~1',
    'tb status',
    'echo $THREADBEAR_RUNNER_HOME',
    'ls "${THREADBEAR_RUNNER_HOME}/threads"',
    'cat $THREADBEAR_HOME/threads/th1.json',
    'echo threadbear and secrets',
  ]) assert.equal(deny(command), undefined, command);
  assert.equal(denySecrets('Read', { file_path: path.join(home, 'threads', 'a.txt') }, home), undefined);
  assert.equal(denySecrets('Read', { file_path: path.join(os.tmpdir(), 'other', 'SECRETS', 'x') }, home), undefined, "someone else's folder of that name");
});

// Codex tried to break this change before it landed (11 Nov 2026).
test('what the cross-lab review got past the guard', () => {
  // The Kelvin sign (U+212A) folds to a K, which took the token's word
  // boundary with it; the shell's ${x%K} then cuts it back to the file's name.
  // Nothing else in the command names the folder, so only the name can hold it.
  const token = ['runner', 'token'].join('-');
  const kelvin = '\u212A';
  assert.ok(deny(`cd "$THREADBEAR_HOME"; x=${token}${kelvin}; cat secrets/"\${x%${kelvin}}"`), 'Kelvin sign, the name alone');
  // Ways of writing the same folder that the shell reads as it.
  for (const command of [
    'cat "$THREADBEAR_HOME"/./secrets/convex-deploy.env',
    'cat $THREADBEAR_HOME//secrets/convex-deploy.env',
    'cat "${THREADBARE_RUNNER_HOME:-unused}/secrets/convex-deploy.env"',
    'cat ${THREADBEAR_HOME%/}/secrets/convex-deploy.env',
    // The fallback can hold the path itself (the second pass).
    'unset X; cat "${X:-$THREADBEAR_HOME/secrets/convex-deploy.env}"',
    // A fallback with a $ in it, which the variable never needs (the third pass).
    'cat "${THREADBEAR_HOME:-$HOME}/secrets/convex-deploy.env"',
  ]) assert.ok(deny(command), command);
  // A Grep from further up reads every file its glob picks, whatever its pattern.
  const up = path.dirname(home);
  assert.ok(denySecrets('Grep', { path: up, pattern: '.', glob: `${path.basename(home)}/secrets/*`, output_mode: 'content' }, home), 'Grep with a glob');
  assert.ok(denySecrets('Grep', { path: up, pattern: '.' }, home), 'Grep over everything under it');
  // An accent the shell cuts off, which NFC would join onto the s (the third pass).
  const accent = '\u0301';
  assert.ok(deny(`p="$THREADBEAR_HOME/secrets${accent}"; cat "\${p%${accent}}/convex-deploy.env"`), 'a combining accent');
  // From the top of the disk too, which already ends in a separator (the second pass).
  assert.ok(denySecrets('Grep', { path: path.parse(home).root, pattern: '.', glob: '**/secrets/*' }, home), 'Grep from /');
  // A Glob only lists names, so one that doesn't name the folder is fine.
  assert.equal(denySecrets('Glob', { path: up, pattern: '*.md' }, home), undefined);
  // A Grep further in, or beside it, is fine.
  assert.equal(denySecrets('Grep', { path: path.join(home, 'threads'), pattern: '.' }, home), undefined);
});

test('a data folder whose name has an accent, written either way', () => {
  const accented = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'tb-café-')));
  try {
    const decomposed = accented.normalize('NFD');
    assert.notEqual(decomposed, accented);
    assert.ok(denySecrets('Bash', { command: `cat "${decomposed}/secrets/convex-deploy.env"` }, accented), 'Bash');
    assert.ok(denySecrets('Read', { file_path: path.join(decomposed, 'secrets', 'x') }, accented), 'Read');
  } finally {
    fs.rmSync(accented, { recursive: true, force: true });
  }
});

test('a long command with an unclosed ${ is checked quickly', () => {
  // The second pass found the first rewrite of ${X...} quadratic on these.
  for (const command of [`printf %s '\${${'A'.repeat(64_000)}'`, `echo ${'${a'.repeat(20_000)}`]) {
    const start = performance.now();
    deny(command);
    const took = performance.now() - start;
    assert.ok(took < 500, `${command.length} characters took ${Math.round(took)} ms`);
  }
});
