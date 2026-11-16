// A quick ask from the phone (docs/protocol.md, Sync, "quickAsk"): the phone
// already said the answer out loud, so the runner keeps the exchange as a
// thread without running a turn, and the first message sent to it starts a
// fresh session told how the thread began.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { FakeAccounts, makeHarness, tick, withCodex } from '../helpers/fakes.ts';

const ask = {
  question: 'What did I call the backup drive on Mac mini?',
  answer: "It's MediaDrive, the 8 TB one.",
  sources: [{ title: 'Mac mini migration', url: 'https://mac-mini.example/?thread=t1&item=4' }],
  askedAt: Date.parse('2026-10-29T03:00:00Z'),
};

test('a quick ask becomes a seen, finished thread with no CLI started', () => {
  const h = makeHarness();
  const { thread } = h.runner.quickAsk(ask);
  assert.equal(h.adapter.sessions.length, 0, 'no turn runs');
  const evs = h.events(thread.id);
  assert.deepEqual(
    evs.map((e) => e.type),
    ['user.message', 'turn.started', 'assistant.text', 'system.note', 'turn.completed'],
  );
  const turnId = evs[0].turnId;
  assert.ok(turnId);
  assert.ok(evs.every((e) => e.turnId === turnId), 'all under one turn');
  assert.equal(evs[0].text, ask.question);
  assert.equal(evs[0].origin, 'phone');
  assert.match(evs[2].text, /^It's MediaDrive, the 8 TB one\./);
  assert.match(evs[2].text, /\[Mac mini migration\]\(https:\/\/mac-mini\.example\/\?thread=t1&item=4\)/);
  assert.equal(evs[3].text, 'Answered by Ask from Agent Archive');
  assert.equal(evs[4].outcome, 'completed');
  assert.equal(evs[4].needs, 'nothing');
  const summary = h.runner.get(thread.id);
  assert.equal(summary.status, 'idle');
  assert.equal(summary.lastSeenSeq, summary.lastSeq, 'Alex has heard it');
  assert.equal(summary.cwd, h.cfg.defaults.cwd);
  assert.equal(summary.title, ask.question);
  assert.match(summary.preview, /MediaDrive/);
});

test('a quick ask with no sources has just the answer', () => {
  const h = makeHarness();
  const { thread } = h.runner.quickAsk({ ...ask, sources: [] });
  const text = h.events(thread.id).find((e) => e.type === 'assistant.text').text;
  assert.equal(text, ask.answer);
});

test('a follow-up runs on Haiku, even with a Codex login too', async () => {
  const h = makeHarness(undefined, {}, { accounts: new FakeAccounts(withCodex()) });
  const { thread } = h.runner.quickAsk(ask);
  assert.equal(thread.provider, 'claude');
  assert.equal(thread.model, 'claude-haiku-5-5');
  h.runner.send(thread.id, 'How big is the other one?');
  await tick();
  assert.equal(h.adapter.last.opts.model, 'claude-haiku-5-5');
  assert.equal(h.events(thread.id).some((e) => e.type === 'system.note' && /isn't a/.test(e.text)), false);
});

test('with only a Codex login, a follow-up runs on Luna at low effort', () => {
  const h = makeHarness(undefined, {}, { accounts: new FakeAccounts(withCodex().filter((a) => a.provider === 'codex')) });
  const { thread } = h.runner.quickAsk(ask);
  assert.equal(thread.provider, 'codex');
  assert.equal(thread.model, 'gpt-6-luna');
  assert.equal(thread.effort, 'low');
  assert.equal(h.events(thread.id).some((e) => e.type === 'system.note' && /isn't a|doesn't offer/.test(e.text)), false);
});

test("a follow-up starts a fresh session told the thread began as a quick ask", async () => {
  const h = makeHarness();
  const { thread } = h.runner.quickAsk(ask);
  h.runner.send(thread.id, 'How big is the other one?');
  await tick();
  const s = h.adapter.last;
  assert.equal(s.opts.resume, false);
  const sent = s.sent[0].text;
  assert.match(sent, /^\[Threadbear handoff\] This thread began as a quick ask on the user's phone/);
  assert.match(sent, /User: What did I call the backup drive on Mac mini\?/);
  assert.match(sent, /Assistant: It's MediaDrive/);
  assert.ok(sent.endsWith('How big is the other one?'));
  // No "Started a new session" note: there was no session before.
  assert.ok(!h.events(thread.id).some((e) => e.type === 'system.note' && /Started a new session/.test(e.text)));
  // The second follow-up resumes that session, with no note.
  s.init();
  s.text('m1:0', 'The 5 TB one is ExternalBak.');
  s.result('completed', 0.01);
  await tick();
  h.runner.send(thread.id, 'thanks');
  await tick();
  const last = h.adapter.last.sent.at(-1)!.text;
  assert.equal(last, 'thanks');
});
