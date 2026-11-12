// A dev runner on a copy of the real data dir (dev-runner.sh --clone-real)
// never runs the copied threads: on 11 Nov 2026 one on a plain copy
// carried on a thread mid-turn in Mike's real session and folder.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { makeHarness, tick, tmpDir } from '../helpers/fakes.ts';
import { REAL_COPY_FILE, cloneRealHome } from '../../src/real-copy.ts';
import { diffLines } from '../../src/diff.ts';

test('a copied thread that was mid-turn is not carried on, resumed or run', async () => {
  const real = tmpDir();
  const h = makeHarness(real);
  const realFolder = tmpDir();
  const { thread } = h.runner.create({ text: 'build the thing', cwd: realFolder });
  h.adapter.last.init();
  const sessionId = h.runner.get(thread.id).nativeSessionId;
  assert.ok(sessionId);
  fs.writeFileSync(path.join(real, 'schedules.json'), '[]');
  fs.writeFileSync(path.join(real, 'backend.json'), '{}');
  // The real runner dies mid-turn; its data dir is cloned.
  const copy = tmpDir();
  const work = path.join(copy, 'work');
  assert.equal(cloneRealHome(real, copy, work).threads, 1);
  assert.ok(fs.existsSync(path.join(copy, REAL_COPY_FILE)));
  assert.ok(!fs.existsSync(path.join(copy, 'schedules.json')), 'schedules stay behind');
  assert.ok(!fs.existsSync(path.join(copy, 'backend.json')), "the backend's keys stay behind");

  const d = makeHarness(copy);
  d.runner.carryOnStopped();
  await tick();
  assert.equal(d.adapter.sessions.length, 0, 'nothing started');
  const t = d.runner.get(thread.id);
  assert.equal(t.status, 'idle');
  assert.equal(t.nativeSessionId, undefined);
  assert.ok(t.cwd.startsWith(work), `its folder is under the copy's work folder, not ${t.cwd}`);
  // Sent to by hand: the turn fails and says why, with no CLI.
  try { d.runner.send(thread.id, 'go on then'); } catch (e) { assert.match(String(e), /copy of one of your real threads/); }
  await tick();
  assert.equal(d.adapter.sessions.length, 0);
  assert.match(JSON.stringify(d.runner.events(thread.id).events), /copy of one of your real threads/);
  // A thread made on the dev runner itself runs.
  d.runner.create({ text: 'new one', cwd: work });
  assert.equal(d.adapter.sessions.length, 1);
});

test('a copy made by hand with the marker is neutralised as it loads', async () => {
  const real = tmpDir();
  const h = makeHarness(real);
  const { thread } = h.runner.create({ text: 'x', cwd: tmpDir() });
  h.adapter.last.init();
  const copy = tmpDir();
  fs.cpSync(real, copy, { recursive: true });
  fs.writeFileSync(path.join(copy, REAL_COPY_FILE), '{}');
  const d = makeHarness(copy);
  d.runner.carryOnStopped();
  await tick();
  assert.equal(d.adapter.sessions.length, 0);
  assert.equal(d.runner.get(thread.id).status, 'idle');
  const onDisk = JSON.parse(fs.readFileSync(path.join(copy, 'threads', thread.id, 'thread.json'), 'utf8'));
  assert.equal(onDisk.copied, true);
  assert.equal(onDisk.nativeSessionId, undefined);
});

test("going back to an earlier message in a copied thread doesn't touch the real folder's files", async () => {
  const real = tmpDir();
  const h = makeHarness(real);
  const folder = tmpDir();
  const { thread } = h.runner.create({ text: 'make a plan', cwd: folder, accountId: 'claude-3' });
  const s = h.adapter.last;
  s.init();
  s.result();
  const file = path.join(folder, 'notes.txt');
  h.runner.send(thread.id, 'write notes.txt');
  fs.writeFileSync(file, 'the plan\n');
  s.out({ kind: 'event', event: { type: 'tool.call', itemId: 'w1', name: 'Write', kind: 'write', title: 'Write notes.txt', input: { file_path: file } } });
  s.out({ kind: 'event', event: { type: 'edit.diff', itemId: 'w1', path: file, hunks: diffLines('', 'the plan\n') } });
  s.out({ kind: 'event', event: { type: 'tool.result', itemId: 'w1', ok: true, text: '', truncated: false, images: [] } });
  s.result();
  const seq = h.store.log(thread.id).read(1, h.store.log(thread.id).lastSeq).filter((e) => e.type === 'user.message').at(-1)!.seq;
  const copy = tmpDir();
  cloneRealHome(real, copy, path.join(copy, 'work'));
  const d = makeHarness(copy);
  d.adapter.rewind = async () => ({ resumeAt: 'entry-1' });
  await assert.rejects(d.runner.editMessage(thread.id, seq, 'something else'), /copy of one of your real threads/);
  assert.equal(fs.readFileSync(file, 'utf8'), 'the plan\n', 'the real file is left alone');
});

test('a clone copies through links rather than writing through them to the real data dir', () => {
  const real = tmpDir();
  const h = makeHarness(real);
  const { thread } = h.runner.create({ text: 'x', cwd: tmpDir() });
  h.adapter.last.init();
  // A data dir whose threads folder is a link to the real one.
  const linked = tmpDir();
  fs.symlinkSync(path.join(real, 'threads'), path.join(linked, 'threads'));
  const copy = tmpDir();
  cloneRealHome(linked, copy, path.join(copy, 'work'));
  const realRec = JSON.parse(fs.readFileSync(path.join(real, 'threads', thread.id, 'thread.json'), 'utf8'));
  assert.ok(realRec.nativeSessionId, 'the real record keeps its session');
  assert.equal(realRec.copied, undefined);
  assert.ok(!fs.lstatSync(path.join(copy, 'threads')).isSymbolicLink());
});
