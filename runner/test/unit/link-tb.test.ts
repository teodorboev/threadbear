// scripts/link-tb.sh: tb in ~/.local/bin next to threadbear, running the
// release this Mac's runner runs (issue #39: setup printed tb commands that
// were "command not found").
import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { tmpDir } from '../helpers/fakes.ts';

const SCRIPT = path.resolve(import.meta.dirname, '..', '..', 'scripts', 'link-tb.sh');

function setup() {
  const root = tmpDir();
  const bin = path.join(root, 'bin');
  // A data dir with a space in it, like the real one.
  const data = path.join(root, 'Application Support', 'Threadbear');
  const checkout = path.join(root, 'checkout', 'runner');
  // A stand-in "node" that prints the script it was handed and its arguments.
  const node = path.join(root, 'node');
  fs.writeFileSync(node, '#!/bin/sh\necho "ran $*"\n', { mode: 0o755 });
  const link = (...args: string[]) => spawnSync('bash', [SCRIPT, ...args], { env: { ...process.env, THREADBEAR_BIN_DIR: bin }, encoding: 'utf8' });
  const tb = (...args: string[]) => spawnSync(path.join(bin, 'tb'), args, { encoding: 'utf8' });
  return { root, bin, data, checkout, node, link, tb };
}

test("tb runs the current release's tb under the runner's Node, or the checkout's before there's one", () => {
  const s = setup();
  assert.equal(s.link(s.data, s.node, s.checkout).status, 0);
  assert.equal(s.tb('status', 'a b').stdout.trim(), `ran ${path.join(s.checkout, 'bin', 'tb')} status a b`);
  const rel = path.join(s.data, 'runner', 'current', 'bin');
  fs.mkdirSync(rel, { recursive: true });
  fs.writeFileSync(path.join(rel, 'tb'), '');
  assert.equal(s.tb('restart').stdout.trim(), `ran ${path.join(rel, 'tb')} restart`);
  // Again is fine, and --remove takes it out.
  assert.equal(s.link(s.data, s.node, s.checkout).status, 0);
  assert.equal(s.link('--remove').status, 0);
  assert.ok(!fs.existsSync(path.join(s.bin, 'tb')));
});

test("a tb that isn't Threadbear's is left alone, by install and by --remove", () => {
  const s = setup();
  fs.mkdirSync(s.bin);
  fs.writeFileSync(path.join(s.bin, 'tb'), '#!/bin/sh\necho mine\n', { mode: 0o755 });
  const r = s.link(s.data, s.node, s.checkout);
  assert.equal(r.status, 0);
  assert.match(r.stderr, /isn't Threadbear's/);
  s.link('--remove');
  assert.equal(s.tb().stdout.trim(), 'mine');
  // A link someone made by hand counts as theirs too.
  fs.rmSync(path.join(s.bin, 'tb'));
  fs.symlinkSync(path.join(s.checkout, 'bin', 'tb'), path.join(s.bin, 'tb'));
  s.link(s.data, s.node, s.checkout);
  assert.ok(fs.lstatSync(path.join(s.bin, 'tb')).isSymbolicLink());
});

test('--from-source runs the checkout\'s tb even with an older release lying about', () => {
  const s = setup();
  const rel = path.join(s.data, 'runner', 'current', 'bin');
  fs.mkdirSync(rel, { recursive: true });
  fs.writeFileSync(path.join(rel, 'tb'), '');
  assert.equal(s.link(s.data, s.node, s.checkout, '--from-source').status, 0);
  assert.equal(s.tb('status').stdout.trim(), `ran ${path.join(s.checkout, 'bin', 'tb')} status`);
  // And back to releases on the next install without it.
  assert.equal(s.link(s.data, s.node, s.checkout).status, 0);
  assert.equal(s.tb('status').stdout.trim(), `ran ${path.join(rel, 'tb')} status`);
  assert.equal(s.link(s.data, s.node, s.checkout, '--bogus').status, 2);
});
