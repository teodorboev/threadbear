// site/scripts/build.mjs --check (check.sh's "site build" step) builds into a folder mkdtemp made
// and keeps that one folder throughout. It used to remove it and make it again, so on a shared
// /tmp another account could put its own folder at that name in between, with a symlink at a
// fingerprinted file's name, and the build's write went through it into a file of the checking
// account's (Daybreak, 13 Nov, finding 114). The test plays that account winning the race every
// time: a preload puts the planted folder in place the moment the build removes its check folder
// before it has finished.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { createHash } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { tmpDir } from '../helpers/fakes.ts';

const SITE = path.join(import.meta.dirname, '..', '..', '..', 'site');

test('a check build never removes its own temp folder before it ends, so a planted one is never written through', () => {
  const tmp = tmpDir('site-check-');
  // The build's TMPDIR, standing in for a shared /tmp.
  const shared = path.join(tmp, 'shared');
  fs.mkdirSync(shared);
  const victim = path.join(tmp, 'victim.txt');
  fs.writeFileSync(victim, 'ORIGINAL', { mode: 0o600 });
  const body = fs.readFileSync(path.join(SITE, 'src', 'styles.css'));
  const hashed = `styles.${createHash('sha256').update(body).digest('hex').slice(0, 8)}.css`;
  const preload = path.join(tmp, 'attacker.mjs');
  fs.writeFileSync(
    preload,
    `import fs from 'node:fs';
import { syncBuiltinESMExports } from 'node:module';
let exiting = false;
process.on('exit', () => { exiting = true; });
const rm = fs.rmSync;
const made = fs.mkdtempSync;
fs.mkdtempSync = (...a) => { const d = made(...a); fs.writeFileSync(${JSON.stringify(path.join(tmp, 'made'))}, String(fs.statSync(d).ino)); return d; };
fs.rmSync = (p, o) => {
  rm(p, o);
  if (!exiting && String(p).startsWith(${JSON.stringify(shared)})) {
    fs.writeFileSync(${JSON.stringify(path.join(tmp, 'removed-early'))}, String(p));
    fs.mkdirSync(p, { mode: 0o777 });
    fs.symlinkSync(${JSON.stringify(victim)}, p + '/' + ${JSON.stringify(hashed)});
  }
};
syncBuiltinESMExports();
`,
  );
  const r = spawnSync(process.execPath, ['--import', preload, 'scripts/build.mjs', '--check'], {
    cwd: SITE,
    env: { ...process.env, TMPDIR: shared },
    encoding: 'utf8',
  });
  assert.equal(r.status, 0, r.stderr);
  assert.match(r.stdout, /they build/);
  assert.ok(fs.existsSync(path.join(tmp, 'made')), 'the check made its folder with mkdtemp');
  assert.equal(fs.readFileSync(victim, 'utf8'), 'ORIGINAL');
  assert.ok(!fs.existsSync(path.join(tmp, 'removed-early')), 'the check removed its own folder before it had finished');
  // And it cleaned up after itself.
  assert.deepEqual(fs.readdirSync(shared), []);
});
