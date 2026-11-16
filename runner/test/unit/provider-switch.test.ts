// Moving a thread to another login while a turn runs (as in T3 Code): the
// turn carries on where it started and says so, and the next message from
// Alex stops it and starts its own turn on the new login, with a handoff
// that has the work between messages, not only the chat.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { FakeAccounts, makeHarness, tick, withCodex } from '../helpers/fakes.ts';
import { historyItems, selectHistory } from '../../src/handoff-history.ts';
import type { PersistedEvent } from '../../src/protocol.ts';

function harness() {
  return makeHarness(undefined, {}, { accounts: new FakeAccounts(withCodex()) });
}

const notes = (h: { events(id: string): any[] }, id: string): string[] => h.events(id).filter((e) => e.type === 'system.note').map((e) => e.text);

// A Codex turn that has run a command and said something, still going.
function codexMidTurn(h: ReturnType<typeof harness>) {
  const { thread } = h.runner.create({ text: 'build the sign-in button', accountId: 'codex-main' });
  const c = h.codex.last;
  c.init('019a-1', 'gpt-6.1-sol');
  c.out({ kind: 'event', event: { type: 'tool.call', itemId: 't1', name: 'shell', kind: 'command', title: 'swift test --filter SignIn', input: {} } });
  c.out({ kind: 'event', event: { type: 'tool.result', itemId: 't1', ok: false, text: 'error: no such module Foo', truncated: false, images: [] } });
  c.text('m1', 'Working on the button');
  return { thread, c };
}

test('a switch mid-turn says the turn carries on, and the next message stops it and hands over', async () => {
  const h = harness();
  const { thread, c } = codexMidTurn(h);
  h.runner.update({ threadId: thread.id, accountId: 'claude-3', model: 'opus' });
  assert.equal(c.ended, false, 'the running turn is left alone');
  assert.ok(notes(h, thread.id).some((n) => /takes over from your next message, which stops this turn\.$/.test(n)), notes(h, thread.id).join(' | '));

  h.runner.send(thread.id, 'explain it simply');
  await tick();
  assert.equal(c.interrupts, 1, 'the Codex turn is stopped');
  assert.equal(c.sent.length, 1, 'the message never went to Codex');
  assert.ok(notes(h, thread.id).some((n) => /^Stopped here so .+ can take over\.$/.test(n)));

  c.result('interrupted');
  await tick();
  const cl = h.adapter.last;
  assert.equal(cl.opts.resume, false);
  const first = cl.sent[0].text;
  assert.match(first, /^\[Threadbear handoff\] This conversation moved from the .+ login to .+, so this is a fresh session\. The thread so far:/);
  assert.match(first, /User: build the sign-in button/);
  assert.match(first, /Tool: swift test --filter SignIn\n  failed: error: no such module Foo/);
  assert.match(first, /Assistant: Working on the button/);
  assert.match(first, /\[End of handoff\. The new message follows\.\]\n\nexplain it simply$/);
  // The message is in the transcript, on the new turn.
  const msgs = h.events(thread.id).filter((e) => e.type === 'user.message').map((e) => e.text);
  assert.deepEqual(msgs, ['build the sign-in button', 'explain it simply']);
});

test('a switch mid-turn with no message after lets the turn finish; the next one starts on the new login', async () => {
  const h = harness();
  const { thread, c } = codexMidTurn(h);
  h.runner.update({ threadId: thread.id, accountId: 'claude-3' });
  c.result();
  await tick();
  assert.equal(c.interrupts, 0);
  h.runner.send(thread.id, 'next');
  await tick();
  assert.match(h.adapter.last.sent[0].text, /^\[Threadbear handoff\][\s\S]*next$/);
});

test("a queued message (a schedule's) waits for the turn rather than stopping it", async () => {
  const h = harness();
  const { thread, c } = codexMidTurn(h);
  h.runner.update({ threadId: thread.id, accountId: 'claude-3' });
  h.runner.send(thread.id, 'scheduled check', [], { origin: 'schedule', queue: true });
  await tick();
  assert.equal(c.interrupts, 0);
});

test('a message mid-turn with no switch waiting still steers into the turn', async () => {
  const h = harness();
  const { thread, c } = codexMidTurn(h);
  h.runner.send(thread.id, 'also make it blue');
  await tick();
  assert.equal(c.interrupts, 0);
  assert.equal(c.sent.at(-1)!.text, 'also make it blue');
});

// ------------------------------------------------------------ the history

let seq = 0;
const ev = (body: Record<string, unknown>, turnId = 't'): PersistedEvent => ({ seq: ++seq, ts: '', threadId: 'x', turnId, ...body }) as unknown as PersistedEvent;

test('history keeps the first message, the latest exchange, and marks the gap', () => {
  const events = [ev({ type: 'user.message', text: 'first ask', images: [] })];
  for (let i = 0; i < 200; i++) events.push(ev({ type: 'assistant.text', itemId: `a${i}`, text: `filler ${i} `.repeat(20) }));
  events.push(ev({ type: 'user.message', text: 'latest ask', images: [] }));
  events.push(ev({ type: 'assistant.text', itemId: 'z', text: 'latest reply' }));
  events.push(ev({ type: 'user.message', text: 'the new message', images: [] }, 'now'));
  const { lines, omitted } = selectHistory(historyItems(events, 'now'), 4000);
  assert.equal(lines[0], 'User: first ask');
  assert.equal(lines[1], '[…]');
  assert.deepEqual(lines.slice(-2), ['User: latest ask', 'Assistant: latest reply']);
  assert.ok(omitted > 150);
  assert.ok(!lines.some((l) => l.includes('the new message')), "the turn's own message follows the note instead");
  assert.ok(lines.join('\n\n').length <= 4000);
});

test('history folds results into their tool calls and lists edited files once per edit', () => {
  const events = [
    ev({ type: 'tool.call', itemId: 'c', name: 'shell', kind: 'command', title: 'npm test', input: {} }),
    ev({ type: 'tool.result', itemId: 'c', ok: true, text: 'x'.repeat(1000), truncated: false, images: [] }),
    ev({ type: 'edit.diff', itemId: 'e', path: 'a.ts', hunks: [] }),
    ev({ type: 'edit.diff', itemId: 'e', path: 'a.ts', hunks: [] }),
    ev({ type: 'error', message: 'boom' }),
  ];
  const { lines } = selectHistory(historyItems(events));
  assert.equal(lines.length, 3);
  assert.match(lines[0], /^Tool: npm test\n  → x{300}…$/);
  assert.equal(lines[1], 'Edited: a.ts');
  assert.equal(lines[2], 'Error: boom');
});
