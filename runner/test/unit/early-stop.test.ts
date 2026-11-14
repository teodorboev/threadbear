import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { FakeAccounts, makeHarness, tick, tmpDir, withCodex } from '../helpers/fakes.ts';
import { makePng } from '../helpers/png.ts';
import { phoneCommands } from '../../src/handover.ts';
import { placePictures } from '../../src/pictures.ts';
import { SessionNotFoundError } from '../../src/adapters/types.ts';

const zero = { inputTokens: 0, outputTokens: 0, cacheReadTokens: 0, cacheWriteTokens: 0 };

function image() {
  const file = path.join(tmpDir(), 'picture.png');
  fs.writeFileSync(file, makePng(2, 2));
  return { path: file, mediaType: 'image/png' };
}

function stop(h: ReturnType<typeof makeHarness>, id: string) {
  h.runner.interrupt(id, { byMike: true });
  h.adapter.last.result('interrupted', 0, undefined, { usage: zero, queuedTurns: 0 });
}

test('send, Stop during resume, switch model, send: the first message and picture reach the next turn', async () => {
  const h = makeHarness();
  const { thread } = h.runner.create({ text: 'hello', accountId: 'claude-3' });
  h.adapter.last.init();
  h.adapter.last.result();
  h.clock.advance(30 * 60_000);
  await tick();
  const image = path.join(tmpDir(), 'picture.png');
  fs.writeFileSync(image, makePng(2, 2));
  h.runner.send(thread.id, 'Please fix this [Picture 1]', [{ path: image, mediaType: 'image/png' }]);
  const resumed = h.adapter.last;
  assert.equal(resumed.opts.resume, true);
  resumed.out({ kind: 'messageState', id: resumed.sent[0].id!, state: 'queued' });
  h.runner.interrupt(thread.id, { byMike: true });
  h.runner.update({ threadId: thread.id, model: 'claude-haiku-5-5' });
  resumed.result('interrupted', 0, undefined, { usage: zero, queuedTurns: 0 });
  assert.equal(h.runner.get(thread.id).status, 'idle', 'Stop does not restart the work');
  h.runner.send(thread.id, 'Continuing on a smaller model.');
  const sent = h.adapter.last.sent.at(-1)!;
  assert.match(sent.text, /Please fix this/);
  assert.ok(sent.text.indexOf('Please fix this') < sent.text.indexOf('Continuing on a smaller model.'));
  assert.equal(sent.images.length, 1);
  assert.ok(fs.existsSync(sent.images[0].path));
  assert.ok(h.events(thread.id).some(e => e.type === 'system.note' && /next message/.test(e.text)), 'the saved message is visible');
  assert.equal(h.events(thread.id).filter(e => e.type === 'user.message' && e.text === 'Please fix this [Picture 1]').length, 1);
});

test('another early Stop retains every message once; a successful reply settles them', () => {
  const h = makeHarness();
  const { thread } = h.runner.create({ text: 'first request', accountId: 'claude-3' });
  stop(h, thread.id);
  h.runner.send(thread.id, 'second request');
  stop(h, thread.id);
  h.runner.send(thread.id, 'third request');
  const sent = h.adapter.last.sent.at(-1)!.text;
  for (const word of ['first request', 'second request', 'third request']) assert.equal(sent.split(word).length, 2);
  h.adapter.last.text('reply', 'Done.');
  h.adapter.last.result();
  h.runner.send(thread.id, 'fourth request');
  assert.equal(h.adapter.last.sent.at(-1)!.text, 'fourth request');
});

test('the next message can arrive while Stop is still finishing', () => {
  const h = makeHarness();
  const { thread } = h.runner.create({ text: 'unread request', accountId: 'claude-3' });
  h.runner.interrupt(thread.id);
  h.runner.send(thread.id, 'after Stop');
  h.adapter.last.result('interrupted', 0, undefined, { usage: zero, queuedTurns: 0 });
  assert.match(h.adapter.last.sent.at(-1)!.text, /unread request[\s\S]*after Stop/);
});

test('saved messages survive a restart and picture numbers stay attached to their own words', () => {
  const h = makeHarness();
  const first = image();
  const { thread } = h.runner.create({ text: 'earlier [Picture 1]', images: [first], accountId: 'claude-3' });
  stop(h, thread.id);
  const h2 = h.restart();
  h2.runner.send(thread.id, 'latest [Picture 1]', [image()]);
  const sent = h2.adapter.last.sent.at(-1)!;
  assert.match(sent.text, /earlier \[Picture 1\][\s\S]*latest \[Picture 2\]/);
  assert.equal(sent.images.length, 2);
  const parts = placePictures(sent.text, sent.images);
  assert.deepEqual(parts.map(p => 'image' in p ? p.image.path : p.text.includes('earlier [Picture 1]') ? 'earlier' : 'latest'), ['earlier', sent.images[0].path, 'latest', sent.images[1].path]);
});

test('messages steered into a turn before it started are saved individually', () => {
  const h = makeHarness();
  const { thread } = h.runner.create({ text: 'first request', accountId: 'claude-3' });
  h.adapter.last.init();
  h.runner.send(thread.id, 'second request');
  stop(h, thread.id);
  h.runner.send(thread.id, 'third request');
  const sent = h.adapter.last.sent.at(-1)!.text;
  for (const word of ['first request', 'second request', 'third request']) assert.equal(sent.split(word).length, 2);
});

test('phone and web commands use the same recovery and keep the phone origin', async () => {
  const h = makeHarness();
  const { thread } = h.runner.create({ text: 'hello', accountId: 'claude-3' });
  h.adapter.last.result();
  const commands = phoneCommands(h.runner);
  await commands.send!(thread.id, 'phone request', [], { origin: 'phone' });
  await commands.interrupt!(thread.id);
  h.adapter.last.result('interrupted', 0, undefined, { usage: zero });
  await commands.send!(thread.id, 'next from phone', [], { origin: 'phone' });
  assert.match(h.adapter.last.sent.at(-1)!.text, /Sent from the phone app[\s\S]*phone request[\s\S]*next from phone/);
});

for (const progress of ['text', 'thinking', 'tool', 'inputTokens', 'outputTokens', 'cacheReadTokens', 'cacheWriteTokens'] as const) {
  test(`Stop after ${progress} does not replay work`, () => {
    const h = makeHarness();
    const { thread } = h.runner.create({ text: 'do the work', accountId: 'claude-3' });
    const s = h.adapter.last;
    s.init();
    if (progress === 'text') s.delta('a', 'started');
    if (progress === 'thinking') s.out({ kind: 'delta', delta: { type: 'reasoning.delta', itemId: 'r', text: 'thinking' } });
    if (progress === 'tool') s.out({ kind: 'event', event: { type: 'tool.call', itemId: 't', name: 'Bash', kind: 'command', input: {}, title: 'do work' } });
    h.runner.interrupt(thread.id);
    s.result('interrupted', 0, undefined, { usage: { ...zero, ...(progress.endsWith('Tokens') ? { [progress]: 1 } : {}) } });
    h.runner.send(thread.id, 'new request');
    assert.equal(h.adapter.last.sent.at(-1)!.text, 'new request');
  });
}

test('Stop before the CLI is ready preserves the request even if interrupt fails', async () => {
  const h = makeHarness();
  const { thread } = h.runner.create({ text: 'unread request', accountId: 'claude-3' });
  h.adapter.last.interruptError = new Error('not ready');
  h.runner.interrupt(thread.id);
  await tick();
  h.runner.send(thread.id, 'new request');
  await tick();
  assert.match(h.adapter.last.sent.at(-1)!.text, /unread request[\s\S]*new request/);
});

test('slash commands and scheduled messages leave the saved request for the user', () => {
  const h = makeHarness();
  const { thread } = h.runner.create({ text: 'unread request', accountId: 'claude-3' });
  stop(h, thread.id);
  h.runner.send(thread.id, '/model');
  h.adapter.last.result();
  h.runner.send(thread.id, 'scheduled check', [], { origin: 'schedule' });
  assert.equal(h.adapter.last.sent.at(-1)!.text, 'scheduled check');
  h.adapter.last.result();
  h.runner.send(thread.id, 'new request');
  assert.match(h.adapter.last.sent.at(-1)!.text, /unread request[\s\S]*new request/);
});

test('editing the stopped message replaces it instead of recovering the old words', async () => {
  const h = makeHarness();
  h.adapter.rewind = async () => ({ fresh: true });
  const { thread } = h.runner.create({ text: 'old request', accountId: 'claude-3' });
  stop(h, thread.id);
  const seq = h.events(thread.id).find(e => e.type === 'user.message').seq;
  await h.runner.editMessage(thread.id, seq, 'corrected request');
  await tick();
  assert.equal(h.adapter.last.sent.at(-1)!.text, 'corrected request');
});

for (const accountId of ['claude-3', 'codex-main']) test(`/clear on ${accountId} discards the saved message with the rest of the conversation`, async () => {
  const h = makeHarness(undefined, {}, { accounts: new FakeAccounts(withCodex()) });
  const { thread } = h.runner.create({ text: 'old request', accountId });
  const adapter = accountId === 'codex-main' ? h.codex : h.adapter;
  h.runner.interrupt(thread.id);
  adapter.last.result('interrupted', 0, undefined, { usage: zero });
  h.runner.send(thread.id, '/clear');
  await tick();
  adapter.last.result();
  h.runner.send(thread.id, 'new request');
  await tick();
  assert.equal(adapter.last.sent.at(-1)!.text, 'new request');
});

test('an unplaced picture in the latest message stays after the earlier words', () => {
  const h = makeHarness();
  const { thread } = h.runner.create({ text: 'earlier words', accountId: 'claude-3' });
  stop(h, thread.id);
  h.runner.send(thread.id, 'latest words', [image()]);
  const sent = h.adapter.last.sent.at(-1)!;
  const parts = placePictures(sent.text, sent.images);
  assert.ok('text' in parts[0] && parts[0].text.includes('earlier words'));
  assert.ok('image' in parts[1]);
  assert.ok('text' in parts[2] && parts[2].text.includes('latest words'));
});

test('a provider switch carries the saved picture to the new adapter', async () => {
  const h = makeHarness(undefined, {}, { accounts: new FakeAccounts(withCodex()) });
  const { thread } = h.runner.create({ text: 'fix this [Picture 1]', images: [image()], accountId: 'claude-3' });
  stop(h, thread.id);
  h.runner.update({ threadId: thread.id, accountId: 'codex-main' });
  h.runner.send(thread.id, 'on the new provider');
  await tick();
  const sent = h.codex.last.sent[0];
  assert.match(sent.text, /fix this \[Picture 1\][\s\S]*on the new provider/);
  assert.equal(sent.images.length, 1);
});

test('nonzero usage prevents replay even when init has not arrived', () => {
  const h = makeHarness();
  const { thread } = h.runner.create({ text: 'old request', accountId: 'claude-3' });
  h.runner.interrupt(thread.id);
  h.adapter.last.result('interrupted', 0, undefined, { usage: { ...zero, inputTokens: 10 } });
  h.runner.send(thread.id, 'new request');
  assert.equal(h.adapter.last.sent.at(-1)!.text, 'new request');
});

test('a message the CLI started is a deliberate cancel even at zero tokens', () => {
  const h = makeHarness();
  const { thread, turnId } = h.runner.create({ text: 'deploy it', accountId: 'claude-3' });
  h.adapter.last.out({ kind: 'messageState', id: turnId, state: 'started' });
  stop(h, thread.id);
  h.runner.send(thread.id, 'actually run the tests first');
  assert.equal(h.adapter.last.sent.at(-1)!.text, 'actually run the tests first');
});

test('a queued input cancelled by the CLI is recovered after other work already streamed', () => {
  const h = makeHarness();
  const { thread, turnId } = h.runner.create({ text: 'first work', accountId: 'claude-3' });
  const s = h.adapter.last;
  s.out({ kind: 'messageState', id: turnId, state: 'started' });
  s.text('a', 'Working on the first request.');
  h.runner.send(thread.id, 'unread second request');
  const next = s.sent.at(-1)!.id!;
  s.out({ kind: 'messageState', id: next, state: 'queued' });
  s.out({ kind: 'messageState', id: next, state: 'cancelled' });
  stop(h, thread.id);
  h.runner.send(thread.id, 'latest request');
  const text = s.sent.at(-1)!.text;
  assert.match(text, /unread second request[\s\S]*latest request/);
  assert.ok(!text.includes('first work'));
});

test('a result receipt for a folded-in message prevents replay without its own started frame', () => {
  const h = makeHarness();
  const { thread, turnId } = h.runner.create({ text: 'first work', accountId: 'claude-3' });
  const s = h.adapter.last;
  s.out({ kind: 'messageState', id: turnId, state: 'started' });
  h.runner.send(thread.id, 'second work');
  const second = s.sent.at(-1)!.id!;
  s.out({ kind: 'messageState', id: second, state: 'queued' });
  h.runner.interrupt(thread.id);
  s.result('interrupted', 0, undefined, { answered: [turnId, second], usage: zero });
  h.runner.send(thread.id, 'latest');
  assert.equal(s.sent.at(-1)!.text, 'latest');
});

test('editing a follow-up restores the earlier saved request that its CLI input carried', async () => {
  const h = makeHarness();
  h.adapter.rewind = async () => ({ resumeAt: 'before-followup' });
  const { thread } = h.runner.create({ text: 'earlier request', accountId: 'claude-3' });
  stop(h, thread.id);
  const followup = h.runner.send(thread.id, 'follow-up');
  h.adapter.last.init();
  h.adapter.last.out({ kind: 'messageState', id: followup.turnId, state: 'started' });
  h.adapter.last.text('answer', 'Done.');
  h.adapter.last.result();
  const seq = h.events(thread.id).find(e => e.type === 'user.message' && e.text === 'follow-up').seq;
  await h.runner.editMessage(thread.id, seq, 'edited follow-up');
  await tick();
  assert.match(h.adapter.last.sent.at(-1)!.text, /earlier request[\s\S]*edited follow-up/);
});

test('a missing or outside-media saved picture cannot block the new message or read another file', () => {
  for (const outside of [false, true]) {
    const h = makeHarness();
    const { thread } = h.runner.create({ text: 'earlier [Picture 1]', images: [image()], accountId: 'claude-3' });
    stop(h, thread.id);
    const saved = h.store.log(thread.id).read(1, 1)[0];
    assert.equal(saved.type, 'user.message');
    if (saved.type !== 'user.message') continue;
    const file = saved.images[0].path;
    fs.unlinkSync(file);
    if (outside) fs.symlinkSync(image().path, file);
    h.runner.send(thread.id, 'new request');
    const sent = h.adapter.last.sent.at(-1)!;
    assert.match(sent.text, /earlier \[Picture 1 unavailable\][\s\S]*new request/);
    assert.deepEqual(sent.images, []);
  }
});

test('a session-not-found retry keeps the recovered words and picture', async () => {
  const h = makeHarness();
  const { thread } = h.runner.create({ text: 'earlier [Picture 1]', images: [image()], accountId: 'claude-3' });
  h.adapter.last.init();
  stop(h, thread.id);
  h.clock.advance(30 * 60_000);
  await tick();
  h.runner.send(thread.id, 'follow-up');
  const missing = h.adapter.last;
  missing.sink.exited(new SessionNotFoundError());
  const sent = h.adapter.last.sent[0];
  assert.notEqual(h.adapter.last, missing);
  assert.match(sent.text, /earlier \[Picture 1\][\s\S]*follow-up/);
  assert.equal(sent.images.length, 1);
});

test('background output before the carrying input starts does not settle saved messages', () => {
  const h = makeHarness();
  const { thread } = h.runner.create({ text: 'earlier request', accountId: 'claude-3' });
  stop(h, thread.id);
  const followup = h.runner.send(thread.id, 'follow-up');
  const s = h.adapter.last;
  s.out({ kind: 'messageState', id: followup.turnId, state: 'queued' });
  s.out({ kind: 'cliTurn' });
  s.text('background', 'A background task finished.');
  h.runner.interrupt(thread.id);
  s.result('completed', 0, undefined, { own: true, usage: { inputTokens: 15, outputTokens: 5 } });
  h.runner.send(thread.id, 'latest');
  assert.match(s.sent.at(-1)!.text, /earlier request[\s\S]*follow-up[\s\S]*latest/);
});

test('a user message steered into scheduled work carries saved messages immediately', () => {
  const h = makeHarness();
  const { thread } = h.runner.create({ text: 'earlier request', accountId: 'claude-3' });
  stop(h, thread.id);
  h.runner.send(thread.id, 'scheduled work', [], { origin: 'schedule' });
  h.runner.send(thread.id, 'latest request');
  assert.match(h.adapter.last.sent.at(-1)!.text, /earlier request[\s\S]*latest request/);
});

test('a slash command stopped while the old CLI is closing is never recovered as words', async () => {
  const h = makeHarness();
  h.adapter.slowExit = true;
  const { thread } = h.runner.create({ text: 'hello', accountId: 'claude-3' });
  const s = h.adapter.last;
  s.init();
  s.result();
  h.clock.advance(30 * 60_000);
  h.runner.send(thread.id, '/compact');
  h.runner.interrupt(thread.id);
  s.exit();
  await tick();
  h.runner.send(thread.id, 'latest request');
  await tick();
  assert.equal(h.adapter.last.sent.at(-1)!.text, 'latest request');
});

test('a message cancelled during a skill turn is saved without replaying the skill', () => {
  const h = makeHarness();
  const { thread, turnId } = h.runner.create({ text: '/ship', accountId: 'claude-3' });
  const s = h.adapter.last;
  s.out({ kind: 'messageState', id: turnId, state: 'started' });
  s.text('skill', 'Starting the skill.');
  h.runner.send(thread.id, 'also bump the version');
  const id = s.sent.at(-1)!.id!;
  s.out({ kind: 'messageState', id, state: 'cancelled' });
  stop(h, thread.id);
  h.runner.send(thread.id, 'latest');
  assert.match(s.sent.at(-1)!.text, /also bump the version[\s\S]*latest/);
  assert.ok(!s.sent.at(-1)!.text.includes('/ship'));
});

test('adapter-reported unreceived inputs override CLI readiness without replaying accepted work', () => {
  const h = makeHarness();
  const { thread } = h.runner.create({ text: 'accepted work', accountId: 'claude-3' });
  const s = h.adapter.last;
  s.out({ kind: 'cliVersion', version: '1.0' });
  s.text('work', 'Working.');
  h.runner.send(thread.id, 'held unread request');
  const id = s.sent.at(-1)!.id!;
  h.runner.interrupt(thread.id);
  s.result('interrupted', 0, undefined, { unreceived: [id], answered: [id], usage: { inputTokens: 50 } });
  h.runner.send(thread.id, 'latest');
  assert.match(s.sent.at(-1)!.text, /held unread request[\s\S]*latest/);
  assert.ok(!s.sent.at(-1)!.text.includes('accepted work'));
});

test('a missing media folder cannot block recovery of the words', () => {
  const h = makeHarness();
  const { thread } = h.runner.create({ text: 'earlier [Picture 1]', images: [image()], accountId: 'claude-3' });
  stop(h, thread.id);
  fs.rmSync(h.store.mediaDir(thread.id), { recursive: true });
  h.runner.send(thread.id, 'latest');
  assert.match(h.adapter.last.sent.at(-1)!.text, /earlier \[Picture 1 unavailable\][\s\S]*latest/);
});

test('an adapter dropping the carrying input does not settle its saved content on another input\'s usage', () => {
  const h = makeHarness();
  const { thread } = h.runner.create({ text: 'earlier request', accountId: 'claude-3' });
  stop(h, thread.id);
  h.runner.send(thread.id, 'scheduled work', [], { origin: 'schedule' });
  const s = h.adapter.last;
  s.text('work', 'The scheduled work is running.');
  h.runner.send(thread.id, 'carrying input');
  const id = s.sent.at(-1)!.id!;
  h.runner.interrupt(thread.id);
  s.result('interrupted', 0, undefined, { unreceived: [id], answered: [id], usage: { inputTokens: 50 } });
  h.runner.send(thread.id, 'latest');
  assert.match(s.sent.at(-1)!.text, /earlier request[\s\S]*carrying input[\s\S]*latest/);
});

test('a failed edit does not save the wording it interrupted to replace', async () => {
  const h = makeHarness();
  h.adapter.rewind = async () => { throw new Error('rewind failed'); };
  const { thread } = h.runner.create({ text: 'old wording', accountId: 'claude-3' });
  h.adapter.last.init();
  const edit = h.runner.editMessage(thread.id, 1, 'new wording');
  const failed = assert.rejects(edit, /rewind failed/);
  await tick();
  h.adapter.last.result('interrupted', 0, undefined, { usage: zero });
  await failed;
  h.runner.send(thread.id, 'latest');
  await tick();
  assert.equal(h.adapter.last.sent.at(-1)!.text, 'latest');
});

test('a session-not-found retry keeps recovery on a steered carrier and leaves slash commands bare', async () => {
  for (const command of [false, '/compact', '/clear'] as const) {
    const h = makeHarness();
    const { thread } = h.runner.create({ text: 'earlier [Picture 1]', images: [image()], accountId: 'claude-3' });
    h.adapter.last.init();
    stop(h, thread.id);
    h.clock.advance(30 * 60_000);
    await tick();
    if (command) h.runner.send(thread.id, command);
    else {
      h.runner.send(thread.id, 'scheduled work', [], { origin: 'schedule' });
      h.runner.send(thread.id, 'steered carrier');
    }
    const missing = h.adapter.last;
    missing.sink.exited(new SessionNotFoundError());
    const sent = h.adapter.last.sent;
    if (command) {
      assert.equal(sent[0].text, command);
      h.adapter.last.init();
      h.adapter.last.result();
      h.runner.send(thread.id, 'ordinary send after the command');
      await tick();
      const following = h.adapter.last.sent.at(-1)!;
      if (command === '/clear') {
        assert.equal(following.text, 'ordinary send after the command');
        assert.equal(following.images.length, 0);
      } else {
        assert.match(following.text, /\[Threadbear handoff\][\s\S]*earlier[\s\S]*ordinary send after the command/);
        assert.equal(following.images.length, 1);
      }
    }
    else {
      assert.match(sent[1].text, /earlier \[Picture 1\][\s\S]*steered carrier/);
      assert.equal(sent[1].images.length, 1);
    }
  }
});
