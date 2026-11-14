import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { tmpDir } from '../helpers/fakes.ts';
import { FolderCache } from '../../src/folder-cache.ts';
import { loadPatchSet, projectPanels } from '../../src/patches.ts';

test('a listing answers what is in the folder, a linked folder counting as a folder', () => {
  const dir = tmpDir();
  fs.mkdirSync(path.join(dir, 'convex'));
  fs.writeFileSync(path.join(dir, 'package.json'), '{}');
  fs.symlinkSync(path.join(dir, 'convex'), path.join(dir, 'linked'));
  fs.symlinkSync(path.join(dir, 'gone'), path.join(dir, 'broken'));
  const c = new FolderCache();
  assert.deepEqual(c.names(dir).sort(), ['convex', 'linked', 'package.json']);
  assert.equal(c.isDir(path.join(dir, 'convex')), true);
  assert.equal(c.isDir(path.join(dir, 'linked')), true);
  assert.equal(c.isDir(path.join(dir, 'package.json')), false);
  assert.equal(c.exists(path.join(dir, 'package.json')), true);
  assert.equal(c.exists(path.join(dir, 'broken')), false);
  assert.deepEqual(c.names(path.join(dir, 'nope')), []);
});

test('what was read is kept until the ttl runs out, then read again; clear forgets it', () => {
  const dir = tmpDir();
  let now = 0;
  const c = new FolderCache(10_000, () => now);
  assert.deepEqual(c.names(dir), []);
  fs.writeFileSync(path.join(dir, 'a.ts'), '');
  now = 9_999;
  assert.deepEqual(c.names(dir), [], 'still the listing from before');
  now = 10_000;
  assert.deepEqual(c.names(dir), ['a.ts']);
  fs.writeFileSync(path.join(dir, 'b.ts'), '');
  c.clear();
  assert.deepEqual(c.names(dir).sort(), ['a.ts', 'b.ts']);
});

test('warming reads off the event loop and the synchronous reads after it are hits', async () => {
  const dir = tmpDir();
  fs.mkdirSync(path.join(dir, 'sub'));
  fs.writeFileSync(path.join(dir, '.env'), 'X=1\n');
  const c = new FolderCache(10_000, () => 0);
  await Promise.all([c.warmFolders([dir, dir]), c.warmFolders([dir]), c.warmTexts([path.join(dir, '.env')]), c.warmMtimes([path.join(dir, '.env')])]);
  // Gone from the disk, still answered: these came from the warm reads.
  fs.rmSync(dir, { recursive: true });
  assert.equal(c.isDir(path.join(dir, 'sub')), true);
  assert.equal(c.text(path.join(dir, '.env')), 'X=1\n');
  assert.equal(typeof c.mtimeMs(path.join(dir, '.env')), 'number');
});

test('many threads in a few folders: one shared cache lists each folder once and gives what a fresh read gives', async () => {
  const home = tmpDir();
  const app = path.join(home, 'dev/app');
  fs.mkdirSync(path.join(app, 'convex'), { recursive: true });
  fs.writeFileSync(path.join(app, 'package.json'), '{}');
  fs.writeFileSync(path.join(app, '.env.local'), 'CONVEX_DEPLOYMENT=dev:happy-otter-123\n');
  fs.mkdirSync(path.join(app, '.threadbear/patches'), { recursive: true });
  fs.writeFileSync(path.join(app, '.threadbear/patches/go.mjs'), 'export default { actions: [{ title: "Go", run: "true" }] };');
  const shared = new FolderCache(10_000, () => 0);
  const listed: string[] = [];
  const realReaddir = fs.promises.readdir;
  const realReaddirSync = fs.readdirSync;
  // Counts the listings, async (warm) and sync (a miss) alike.
  fs.promises.readdir = (async (p: fs.PathLike, o: never) => {
    listed.push(String(p));
    return realReaddir(p, o);
  }) as typeof fs.promises.readdir;
  fs.readdirSync = ((p: fs.PathLike, o: never) => {
    listed.push(`sync ${String(p)}`);
    return realReaddirSync(p, o);
  }) as typeof fs.readdirSync;
  let sets: Awaited<ReturnType<typeof loadPatchSet>>[];
  try {
    sets = await Promise.all(Array.from({ length: 20 }, () => loadPatchSet(app, home, new Map(), [], [], shared)));
  } finally {
    fs.promises.readdir = realReaddir;
    fs.readdirSync = realReaddirSync;
  }
  assert.deepEqual(listed.filter((l) => l.startsWith('sync ')), [], 'warm read everything the walk asked for');
  assert.equal(new Set(listed).size, listed.length, `each folder once: ${listed.join(', ')}`);
  const fresh = await loadPatchSet(app, home);
  assert.deepEqual(fresh.actions.map((a) => a.id), ['go.go']);
  for (const set of sets) assert.deepEqual(set, fresh);
  assert.deepEqual(projectPanels(app, home, [], shared), fresh.panels);
});

// A listing read when an agent was about to write a patch file, landing
// after clear() (the runner's "something changed, read again"), put the
// old listing back, so the reads after it missed the new file until the
// cache expired (Codex, reviewing the second-Trust fix, 13 Nov 2026).
test('a read still going when the cache is cleared is not kept', async () => {
  const dir = tmpDir();
  const c = new FolderCache();
  const warming = c.warmFolders([dir]);
  c.clear();
  await warming;
  fs.writeFileSync(path.join(dir, 'new.ts'), '');
  assert.deepEqual(c.names(dir), ['new.ts']);
  // One started after a clear is kept as usual.
  c.clear();
  await c.warmFolders([dir]);
  fs.writeFileSync(path.join(dir, 'later.ts'), '');
  assert.deepEqual(c.names(dir), ['new.ts']);
});
