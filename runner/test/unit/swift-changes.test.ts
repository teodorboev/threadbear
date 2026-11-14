// A made-up fork with a couple of its own Swift app commits, and what the
// runner makes of them (swift-changes.ts): the first look's thread, the
// Updates panel's list, Port this, Dismiss and the Ports: trailer.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { tmpDir } from '../helpers/fakes.ts';
import { FakeClock } from '../../src/clock.ts';
import { nullLogger } from '../../src/log.ts';
import { GPUI_SHARED, scanSwiftChanges, SwiftChanges } from '../../src/swift-changes.ts';

const REPO = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../../..');
const ENV = { ...process.env, GIT_AUTHOR_NAME: 'Them', GIT_AUTHOR_EMAIL: 'them@example.com', GIT_COMMITTER_NAME: 'Them', GIT_COMMITTER_EMAIL: 'them@example.com', GIT_CONFIG_GLOBAL: '/dev/null' };

function git(cwd: string, ...args: string[]): string {
  const r = spawnSync('git', ['-C', cwd, '-c', 'commit.gpgsign=false', ...args], { encoding: 'utf8', env: ENV });
  if (r.status !== 0) throw new Error(`git ${args.join(' ')}: ${r.stderr}`);
  return r.stdout.trim();
}

function commit(dir: string, files: string[], message: string): string {
  for (const f of files) {
    fs.mkdirSync(path.dirname(path.join(dir, f)), { recursive: true });
    fs.appendFileSync(path.join(dir, f), `${message}\n`);
  }
  git(dir, 'add', '-A');
  git(dir, 'commit', '-q', '-m', message);
  return git(dir, 'rev-parse', 'HEAD');
}

// Mike's repo, and someone's fork of it with Mike's release kept as
// upstream.ts keeps it, two Swift commits of their own and the things that
// mustn't count: a change outside the app, one only to what the GPUI app
// still uses, a cherry-pick of Mike's, and a merge of his main.
function world() {
  const root = tmpDir('tb-swift-changes-');
  const mike = path.join(root, 'mike');
  fs.mkdirSync(mike);
  git(mike, 'init', '-q', '-b', 'main');
  commit(mike, ['app/Sources/Threadbear/App.swift', 'desktop/gpui/src/app.rs'], 'Start');
  const theirs = path.join(root, 'theirs');
  git(root, 'clone', '-q', mike, theirs);
  git(theirs, 'remote', 'rename', 'origin', 'upstream');
  git(theirs, 'remote', 'add', 'origin', 'git@github.com:someone/threadbear.git');
  git(theirs, 'update-ref', 'refs/threadbear/upstream/2026-11-01', git(theirs, 'rev-parse', 'HEAD'));

  const sidebar = commit(theirs, ['app/Sources/Threadbear/Views/Sidebar.swift'], 'Bigger sidebar font');
  commit(theirs, ['README.md'], 'My own readme');
  const badge = commit(theirs, ['dock/Sources/Badge.swift', 'app/Tests/Fixtures/badge.json'], 'Orange dock badge');
  commit(theirs, ['app/Tests/Fixtures/only.json'], 'A fixture');
  // Mike fixes something in the Swift app; they cherry-pick it, then merge
  // his main.
  const fix = commit(mike, ['app/Sources/Threadbear/Crash.swift'], 'Fix a crash');
  git(theirs, 'fetch', '-q', 'upstream');
  git(theirs, 'cherry-pick', fix);
  git(theirs, 'merge', '-q', '--no-edit', 'upstream/main');
  return { root, mike, theirs, sidebar, badge };
}

function rig(checkout: string) {
  const home = tmpDir('tb-swift-changes-home-');
  const clock = new FakeClock();
  const started: { text: string; title: string; why: string }[] = [];
  const archived = new Set<string>();
  let changes = 0;
  const s = new SwiftChanges({
    home,
    checkout,
    clock,
    log: nullLogger,
    start: (r) => {
      started.push(r);
      return `t${started.length}`;
    },
    open: (id) => !archived.has(id),
    onChange: () => void changes++,
  });
  return { s, home, clock, started, archived };
}

test("finds the fork's own Swift commits and nothing of Mike's", async () => {
  const w = world();
  const st = await scanSwiftChanges(w.theirs);
  assert.equal(st.state, 'ready');
  assert.deepEqual(st.changes.map((c) => c.subject), ['Orange dock badge', 'Bigger sidebar font']);
  assert.deepEqual(st.changes[0].files, ['dock/Sources/Badge.swift'], "the fixture the GPUI app shares isn't one to rebuild");
  assert.equal(st.changes[1].commit, w.sidebar);
  assert.match(st.changes[1].date, /^\d{4}-\d{2}-\d{2}T/);
});

test("in Mike's own copy it's off, and with nothing of upstream's to compare with too", async () => {
  const w = world();
  git(w.theirs, 'remote', 'set-url', 'origin', 'https://github.com/mikecann/threadbear');
  assert.equal((await scanSwiftChanges(w.theirs)).state, 'off');
  git(w.theirs, 'remote', 'set-url', 'origin', 'git@github.com:someone/threadbear.git');
  git(w.theirs, 'update-ref', '-d', 'refs/threadbear/upstream/2026-11-01');
  git(w.theirs, 'remote', 'remove', 'upstream');
  const st = await scanSwiftChanges(w.theirs);
  assert.equal(st.state, 'off');
  assert.deepEqual(st.changes, []);
});

test('the first hello from the GPUI app offers one thread, once', async () => {
  const w = world();
  const r = rig(w.theirs);
  assert.equal(await r.s.desktopHello(), 'started');
  assert.equal(r.started.length, 1);
  assert.equal(r.started[0].title, 'Bring your changes to the new app');
  assert.match(r.started[0].text, /You changed the old app in 2 places:/);
  assert.match(r.started[0].text, /Bigger sidebar font/);
  assert.match(r.started[0].text, /Orange dock badge/);
  assert.match(r.started[0].text, /Change nothing until I say yes/);
  assert.match(r.started[0].text, /Ports: <the Swift commit's full id>/);
  assert.doesNotMatch(r.started[0].text, /Fix a crash|My own readme|A fixture/);
  // Both show that thread as rebuilding them.
  assert.deepEqual(r.s.status().changes.map((c) => c.threadId), ['t1', 't1']);

  r.clock.advance(5 * 60_000);
  assert.equal(await r.s.desktopHello(), 'again');
  assert.equal(r.started.length, 1);
  // Archived without porting: back to Port this.
  r.archived.add('t1');
  await r.s.scan();
  assert.deepEqual(r.s.status().changes.map((c) => c.threadId), [undefined, undefined]);
});

test("a copy with nothing to bring across starts nothing, and doesn't look for a first time again", async () => {
  const w = world();
  git(w.theirs, 'remote', 'set-url', 'origin', 'https://github.com/mikecann/threadbear.git');
  const r = rig(w.theirs);
  assert.equal(await r.s.desktopHello(), 'off');
  assert.equal(r.started.length, 0);
  assert.ok(JSON.parse(fs.readFileSync(path.join(r.home, 'runner', 'swift-changes.json'), 'utf8')).firstLook);
});

test('Port this starts a thread for that change once; a Ports trailer takes it off the list', async () => {
  const w = world();
  const r = rig(w.theirs);
  await r.s.scan();
  const a = await r.s.port(w.sidebar.slice(0, 9));
  assert.equal(a.threadId, 't1');
  assert.match(r.started[0].title, /Bigger sidebar font/);
  assert.match(r.started[0].text, new RegExp(w.sidebar));
  assert.equal((await r.s.port(w.sidebar)).threadId, 't1', 'pressed again: the same thread');
  assert.equal(r.started.length, 1);

  fs.mkdirSync(path.join(w.theirs, 'desktop'), { recursive: true });
  fs.writeFileSync(path.join(w.theirs, 'desktop', 'sidebar.rs'), 'bigger\n');
  git(w.theirs, 'add', '-A');
  git(w.theirs, 'commit', '-q', '-m', `Bigger sidebar font in the new app\n\nPorts: ${w.sidebar.slice(0, 10)}`);
  const st = await r.s.scan();
  assert.deepEqual(st.changes.map((c) => c.subject), ['Orange dock badge']);
});

test('Dismiss takes one off the list, and undo brings it back', async () => {
  const w = world();
  const r = rig(w.theirs);
  await r.s.scan();
  assert.deepEqual((await r.s.dismiss(w.badge)).swiftChanges.changes.map((c) => c.subject), ['Bigger sidebar font']);
  assert.deepEqual((await r.s.dismiss(w.badge, true)).swiftChanges.changes.map((c) => c.subject), ['Orange dock badge', 'Bigger sidebar font']);
  await assert.rejects(r.s.dismiss('0000000'), /isn't on the list/);
});

test('what the GPUI app still uses from app/ matches swift-freeze.sh', () => {
  const r = spawnSync('bash', [path.join(REPO, 'scripts', 'swift-freeze.sh'), '--gpui-shared'], { encoding: 'utf8' });
  assert.equal(r.status, 0);
  assert.equal(GPUI_SHARED.source.replace(/\\\//g, '/'), r.stdout.trim());
});
