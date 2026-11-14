// Which check hosts ./check.sh sends a run to (THREADBEAR_CHECK_HOST, runner.json's
// checkHosts, and a grouped landing's THREADBEAR_CHECK_PREFER), with the real
// check.sh and a stand-in scripts/check-remote.sh that only says what it got.
// Codex's review (14 Nov): a group's check took the holder's own
// THREADBEAR_CHECK_HOST, so a holder asking only for the cloud box left out
// every Mac a group needing one could run on, and an empty one ran everyone's
// check on this Mac.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { tmpDir } from '../helpers/fakes.ts';

const REPO = path.join(import.meta.dirname, '..', '..', '..');

function hostsFor(env: Record<string, string | undefined>, configured?: string[]): string {
  const root = tmpDir('check-host-');
  fs.mkdirSync(path.join(root, 'scripts'));
  fs.mkdirSync(path.join(root, 'runner', 'scripts'), { recursive: true });
  fs.copyFileSync(path.join(REPO, 'check.sh'), path.join(root, 'check.sh'));
  fs.copyFileSync(path.join(REPO, 'runner', 'scripts', 'find-node.sh'), path.join(root, 'runner', 'scripts', 'find-node.sh'));
  fs.writeFileSync(path.join(root, 'scripts', 'check-remote.sh'), '#!/bin/bash\necho "hosts=$1 prefer=${THREADBEAR_CHECK_PREFER-}"\n', { mode: 0o755 });
  const home = path.join(root, 'home');
  fs.mkdirSync(home);
  if (configured) fs.writeFileSync(path.join(home, 'runner.json'), JSON.stringify({ checkHosts: configured }));
  const base: NodeJS.ProcessEnv = { ...process.env, THREADBEAR_HOME: home, THREADBEAR_NODE: process.execPath };
  delete base.THREADBEAR_CHECK_HOST;
  delete base.THREADBEAR_CHECK_PREFER;
  delete base.THREADBEAR_CHECK_GROUP;
  for (const [k, v] of Object.entries(env)) {
    if (v === undefined) delete base[k];
    else base[k] = v;
  }
  const r = spawnSync('bash', [path.join(root, 'check.sh'), '--quick'], { encoding: 'utf8', env: base });
  return (r.stdout.match(/^hosts=.*$/m) ?? [`none: ${r.stdout}${r.stderr}`])[0];
}

test('by itself, a landing goes where its own THREADBEAR_CHECK_HOST says, else runner.json', () => {
  assert.equal(hostsFor({ THREADBEAR_CHECK_HOST: 'cloud' }, ['tinker-desk', 'cloud']), 'hosts=cloud prefer=');
  assert.equal(hostsFor({}, ['tinker-desk', 'cloud']), 'hosts=tinker-desk,cloud prefer=');
});

test("a group's check goes to the configured hosts with those asked for first, whatever the holder set", () => {
  // The holder asked only for cloud: the configured Mac stays in the list.
  assert.equal(hostsFor({ THREADBEAR_CHECK_HOST: 'cloud', THREADBEAR_CHECK_PREFER: 'cloud,gone' }, ['tinker-desk', 'cloud']),
    'hosts=tinker-desk,cloud prefer=cloud,gone');
  // The holder asked for this Mac (empty): the group still goes to a host.
  assert.equal(hostsFor({ THREADBEAR_CHECK_HOST: '', THREADBEAR_CHECK_PREFER: 'cloud' }, ['tinker-desk', 'cloud']),
    'hosts=tinker-desk,cloud prefer=cloud');
  // The holder asked for this Mac and nobody asked for anything.
  assert.equal(hostsFor({ THREADBEAR_CHECK_HOST: '', THREADBEAR_CHECK_GROUP: '1' }, ['tinker-desk', 'cloud']),
    'hosts=tinker-desk,cloud prefer=');
  // Nothing configured or asked for: the holder's, as before.
  assert.equal(hostsFor({ THREADBEAR_CHECK_HOST: 'cloud', THREADBEAR_CHECK_GROUP: '1' }), 'hosts=cloud prefer=');
  // Nothing configured: the hosts asked for are the list.
  assert.equal(hostsFor({ THREADBEAR_CHECK_PREFER: 'cloud' }), 'hosts=cloud prefer=cloud');
});
