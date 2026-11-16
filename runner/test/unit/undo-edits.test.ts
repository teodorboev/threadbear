import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { diffLines } from '../../src/diff.ts';
import { appliedChanges, undoChanges } from '../../src/undo-edits.ts';
import { cutPointBefore } from '../../src/transcripts.ts';
import type { PersistedEvent } from '../../src/protocol.ts';
import { tmpDir } from '../helpers/fakes.ts';

// An agent's edit of a file: writes the new text and returns the events
// the thread would have for it.
let seq = 0;
function edit(file: string, after: string, opts: { ok?: boolean; itemId?: string } = {}): PersistedEvent[] {
  const before = fs.existsSync(file) ? fs.readFileSync(file, 'utf8') : '';
  fs.writeFileSync(file, after);
  const itemId = opts.itemId ?? `item${++seq}`;
  const env = { ts: '', threadId: 't' };
  return [
    { ...env, seq: ++seq, type: 'edit.diff', itemId, path: file, hunks: diffLines(before, after) },
    { ...env, seq: ++seq, type: 'tool.result', itemId, ok: opts.ok ?? true, text: '', truncated: false, images: [] },
  ];
}

test('undoing edits puts files back the way they were, newest first', () => {
  const dir = tmpDir();
  const a = path.join(dir, 'a.txt');
  const lines = Array.from({ length: 20 }, (_, i) => `line ${i + 1}`);
  fs.writeFileSync(a, lines.join('\n') + '\n');
  const original = fs.readFileSync(a, 'utf8');
  const events = [
    ...edit(a, original.replace('line 2\n', 'line two\n')),
    ...edit(a, fs.readFileSync(a, 'utf8').replace('line 15\n', 'line 15\nnew line\n')),
    ...edit(a, fs.readFileSync(a, 'utf8').replace('line two\n', 'LINE TWO\n')),
  ];
  const r = undoChanges(appliedChanges(events), dir);
  assert.deepEqual(r, { undone: [a], failed: [] });
  assert.equal(fs.readFileSync(a, 'utf8'), original);
});

test('a file the agent made is deleted, and one it deleted comes back', () => {
  const dir = tmpDir();
  const made = path.join(dir, 'new.txt');
  const events = edit(made, 'hello\nworld\n');
  const gone = path.join(dir, 'gone.txt');
  fs.writeFileSync(gone, 'keep me\n');
  // Codex's shape for a deleted file: the old text as one all-minus hunk.
  events.push(
    { seq: ++seq, ts: '', threadId: 't', type: 'edit.diff', itemId: 'del', path: gone, hunks: [{ oldStart: 1, oldLines: 1, newStart: 0, newLines: 0, lines: ['-keep me'] }] },
    { seq: ++seq, ts: '', threadId: 't', type: 'tool.result', itemId: 'del', ok: true, text: '', truncated: false, images: [] },
  );
  fs.rmSync(gone);
  const r = undoChanges(appliedChanges(events), dir);
  assert.deepEqual(r.failed, []);
  assert.equal(fs.existsSync(made), false);
  assert.equal(fs.readFileSync(gone, 'utf8'), 'keep me\n');
});

test('a file changed since in a way the diff does not fit is left alone and reported', () => {
  const dir = tmpDir();
  const a = path.join(dir, 'a.txt');
  fs.writeFileSync(a, 'one\ntwo\nthree\n');
  const events = edit(a, 'one\nTWO\nthree\n');
  // Alex rewrote the same line afterwards.
  fs.writeFileSync(a, 'one\nzwei\nthree\n');
  const r = undoChanges(appliedChanges(events), dir);
  assert.deepEqual(r.undone, []);
  assert.equal(r.failed.length, 1);
  assert.match(r.failed[0].reason, /changed since/);
  assert.equal(fs.readFileSync(a, 'utf8'), 'one\nzwei\nthree\n');
});

test('an edit that moved down the file is still found', () => {
  const dir = tmpDir();
  const a = path.join(dir, 'a.txt');
  fs.writeFileSync(a, 'a\nb\nc\nd\ne\nf\ng\n');
  const events = edit(a, 'a\nb\nc\nd\nE\nf\ng\n');
  // Lines added above it later, by hand or a command.
  fs.writeFileSync(a, 'x\ny\na\nb\nc\nd\nE\nf\ng\n');
  const r = undoChanges(appliedChanges(events), dir);
  assert.deepEqual(r.failed, []);
  assert.equal(fs.readFileSync(a, 'utf8'), 'x\ny\na\nb\nc\nd\ne\nf\ng\n');
});

test('only changes that ran count, and a later diff of the same item replaces the earlier one', () => {
  const dir = tmpDir();
  const a = path.join(dir, 'a.txt');
  fs.writeFileSync(a, 'one\n');
  const failedEdit = edit(a, 'one\n', { ok: false });
  const events = [
    // Codex sends a patch's diff again as it changes; only the last counts.
    { seq: ++seq, ts: '', threadId: 't', type: 'edit.diff', itemId: 'p', path: a, hunks: diffLines('one\n', 'uno\n') },
    ...edit(a, 'ONE\n', { itemId: 'p' }),
    ...failedEdit,
  ] as PersistedEvent[];
  const changes = appliedChanges(events);
  assert.equal(changes.length, 1);
  const r = undoChanges(changes, dir);
  assert.deepEqual(r.failed, []);
  assert.equal(fs.readFileSync(a, 'utf8'), 'one\n');
});

test('cutPointBefore resumes at the entry before the message, or starts fresh for a session\'s first', () => {
  const lines = [
    { type: 'queue-operation' },
    { uuid: 'att', parentUuid: null, type: 'attachment' },
    { uuid: 'm1', parentUuid: 'att', type: 'user' },
    { uuid: 'a1', parentUuid: 'm1', type: 'assistant' },
    { uuid: 'r1', parentUuid: 'a1', type: 'user' },
    { uuid: 'a2', parentUuid: 'r1', type: 'assistant' },
    { uuid: 'm2', parentUuid: 'a2', type: 'user' },
    'not json',
  ].map((l) => (typeof l === 'string' ? l : JSON.stringify(l))).join('\n');
  assert.deepEqual(cutPointBefore(lines, 'm2'), { resumeAt: 'a2', first: false });
  assert.deepEqual(cutPointBefore(lines, 'm1'), { first: true });
  assert.equal(cutPointBefore(lines, 'nope'), null);
  // Only a user entry counts as the message.
  assert.equal(cutPointBefore(lines, 'a1'), null);
});
