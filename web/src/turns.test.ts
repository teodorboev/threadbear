// node --test web/src/turns.test.ts
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { fold, splitTurns, workedTitle, type TurnMessage } from './turns.ts';

let seq = 0;
const at = (s: number) => new Date(Date.UTC(2026, 10, 3, 9, 0, s)).toISOString();
const user = (text: string, s: number, origin?: string): TurnMessage => ({ seq: ++seq, role: 'user', text, at: at(s), ...(origin ? { origin } : {}) });
const said = (text: string, s: number, extra: Partial<TurnMessage> = {}): TurnMessage => ({ seq: ++seq, role: 'assistant', text, at: at(s), ...extra });
const tool = (text: string, s: number): TurnMessage => ({ seq: ++seq, role: 'tool', text, at: at(s) });

const kinds = (items: ReturnType<typeof fold>) => items.map((i) => (i.kind === 'message' ? `${i.m.role}:${i.m.text}` : i.kind));

test('a finished turn folds its work and keeps the reply', () => {
  const items = fold([user('add dark mode', 0), said('Looking.', 1), tool('Read settings.tsx', 2), tool('Edit settings.tsx', 30), said('Done.', 72)], false);
  assert.deepEqual(kinds(items), ['user:add dark mode', 'worked', 'assistant:Done.']);
  const worked = items[1]!;
  assert.equal(worked.kind === 'worked' && workedTitle(worked.ms), 'Worked for 1m 12s');
});

test('the turn still running shows its work', () => {
  const items = fold([user('go', 0), tool('Read a', 1), said('So far', 2)], true);
  assert.deepEqual(kinds(items), ['user:go', 'tools', 'assistant:So far']);
});

test('a quick answer has nothing to fold', () => {
  assert.deepEqual(kinds(fold([user('hi', 0), said('Hello', 1)], false)), ['user:hi', 'assistant:Hello']);
});

test('the summary is the reply when nothing longer comes after the last step', () => {
  const items = fold([user('go', 0), tool('Bash npm test', 1), said('- tests pass', 5, { summary: true }), tool('threadbear: end_turn', 6)], false);
  assert.deepEqual(kinds(items), ['user:go', 'worked', 'assistant:- tests pass']);
});

// The phone's PlansTests.aFinishedTurnKeepsItsPlanInView: a plan stays in
// view, not folded into the turn's working with the steps around it.
test('a finished turn keeps its plan in view', () => {
  const items = fold([user('Plan the bots', 0), tool('Read the schema', 5), said('# Bots\n\n1. Add a table', 10, { plan: true }), tool('Finish the plan', 11), said('Waiting on your go.', 12)], false);
  assert.deepEqual(kinds(items), ['user:Plan the bots', 'worked', 'assistant:# Bots\n\n1. Add a table', 'assistant:Waiting on your go.']);
});

test('a runner message joins the turn before it', () => {
  const items = fold([user('go', 0), said('One', 1), user('Resumed after a restart', 2, 'runner'), tool('Read b', 3), said('Two', 4)], false);
  assert.equal(items[0]!.kind === 'message' && items[0]!.m.text, 'go');
  assert.equal(items.filter((i) => i.kind === 'message' && i.m.role === 'user').length, 1);
});

test('splitTurns times each turn from its ask to its last message', () => {
  const turns = splitTurns([user('a', 0), said('x', 90), user('b', 100), tool('t', 101), said('y', 130)]);
  assert.equal(turns.length, 2);
  assert.equal(turns[0]!.ms, 90_000);
  assert.equal(turns[1]!.starter?.text, 'b');
});
