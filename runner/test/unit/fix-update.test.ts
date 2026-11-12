import { test } from 'node:test';
import assert from 'node:assert/strict';
import { tmpDir } from '../helpers/fakes.ts';
import { nullLogger } from '../../src/log.ts';
import { UpdateFixer, type UpdateFailure } from '../../src/fix-update.ts';

function rig(follows = false) {
  const started: { text: string; title: string; why: string }[] = [];
  const sent: { threadId: string; text: string; turnId: string }[] = [];
  // What's still waiting in each thread's queue, by turn id.
  const queued = new Set<string>();
  const archived = new Set<string>();
  const home = tmpDir('tb-fix-update-');
  const f = new UpdateFixer({
    home,
    log: nullLogger,
    start: (r) => {
      started.push(r);
      return `t${started.length}`;
    },
    open: (id) => !archived.has(id),
    send: (threadId, text) => {
      const turnId = `turn${sent.length + 1}`;
      sent.push({ threadId, text, turnId });
      queued.add(turnId);
      return turnId;
    },
    unqueue: (_threadId, turnId) => queued.delete(turnId),
    followsOrigin: () => follows,
    checkout: '/home/mike/dev/me/threadbear',
  });
  return { f, home, started, sent, queued, archived };
}

function failure(commit: string, kind: UpdateFailure['kind'] = 'build'): UpdateFailure {
  return { kind, commit: `${commit}full`, short: commit, subject: 'Steering', error: 'test/unit/rpc.test.ts failed', logFile: '/tmp/update.log' };
}

test('a failed update starts one thread that fixes it, lands it and archives itself', () => {
  const { f, home, started } = rig();
  assert.equal(f.failed(failure('c2')), 'started');
  assert.equal(started.length, 1);
  assert.equal(started[0].title, 'Fix the runner update to main c2');
  assert.match(started[0].text, /couldn't update to main at c2 \(Steering\): test\/unit\/rpc\.test\.ts failed/);
  assert.match(started[0].text, /\/tmp\/update\.log/);
  assert.match(started[0].text, /archive: true/);
  // A flake with nothing to land: clearing the failed record retries it.
  assert.ok(started[0].text.includes(`${home}/runner/update.json`));
});

test('the same failure again says nothing; a newer one goes to the open thread', () => {
  const { f, started, sent } = rig();
  f.failed(failure('c2'));
  assert.equal(f.failed(failure('c2')), 'already');
  assert.equal(f.failed(failure('c3')), 'sent');
  assert.equal(started.length, 1);
  assert.equal(sent.length, 1);
  assert.equal(sent[0].threadId, 't1');
  assert.match(sent[0].text, /failed again at a newer commit.*c3/s);
  // The backend failing at the same commit is news too.
  assert.equal(f.failed(failure('c3', 'backend')), 'sent');
});

test('once the thread is archived, the next failure starts a fresh one', () => {
  const { f, started, archived } = rig();
  f.failed(failure('c2'));
  archived.add('t1');
  assert.equal(f.failed(failure('c2')), 'started');
  assert.equal(started.length, 2);
});

test('a backend failure says the next commit is what retries it', () => {
  const { f, started } = rig();
  f.failed(failure('c4', 'backend'));
  assert.equal(started[0].title, 'Fix the backend deploy at main c4');
  assert.match(started[0].text, /Convex backend couldn't be deployed at main c4/);
  assert.doesNotMatch(started[0].text, /update\.json/);
});

// bruce and the Pixel each sat days behind GitHub after a thread like this
// committed on their main (a07a9f01, 9d6506d0; 9 Nov).
test('on a machine that follows GitHub the thread is told not to land, and to hand the fix over', () => {
  const { f, home, started } = rig(true);
  f.failed(failure('c2'));
  const text = started[0].text;
  assert.match(text, /Never commit on main/);
  assert.match(text, /\/home\/mike\/dev\/me\/threadbear/);
  assert.match(text, /tb land refuses/);
  assert.match(text, /runner: "main"/);
  // The branch name isn't ours to pick: Mike's cloud box may push only
  // cloud/** (AGENTS.md says so), and a fork has no cloud box.
  assert.match(text, /HEAD:<branch>/);
  assert.doesNotMatch(text, /cloud\//);
  assert.match(text, /ask it to land/);
  assert.doesNotMatch(text, /Land the fix on main/);
  // A flake still needs nothing more than a rerun.
  assert.ok(text.includes(`${home}/runner/update.json`));
});

test('a follow-up on such a machine repeats it', () => {
  const { f, sent } = rig(true);
  f.failed(failure('c2'));
  f.failed(failure('c3'));
  assert.match(sent[0].text, /Never commit on main/);
  assert.doesNotMatch(sent[0].text, /once main goes through/);
});

test('a machine that lands here keeps the usual instructions', () => {
  const { f, started } = rig(false);
  f.failed(failure('c2'));
  assert.match(started[0].text, /Land the fix on main/);
  assert.doesNotMatch(started[0].text, /Never commit on main/);
});

// The backend fix thread (2af6de5c, 11 Nov) was woken twice after its fix
// had gone through, by follow-ups for 37fcdace and 4491e93a queued behind
// its turn: both commits came before the fix.
test('a follow-up still queued is dropped once main gets through, or a newer one replaces it', () => {
  const { f, sent, queued } = rig();
  f.failed(failure('c2'));
  f.failed(failure('c3'));
  f.failed(failure('c4'));
  assert.equal(sent.length, 2);
  assert.deepEqual([...queued], ['turn2'], 'only the newest waits');
  f.through('c5');
  assert.equal(queued.size, 0);
  // Already delivered: nothing to take back, and nothing breaks.
  f.through('c6');
  // A failure after that still goes to the thread.
  assert.equal(f.failed(failure('c7')), 'sent');
  assert.deepEqual([...queued], ['turn3']);
});

test('a follow-up says to archive when main has already got past it', () => {
  const { f, sent } = rig();
  f.failed(failure('c2'));
  f.failed(failure('c3'));
  assert.match(sent[0].text, /tb status/);
});
