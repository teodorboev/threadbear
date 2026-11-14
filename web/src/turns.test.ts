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

// A thread whose login ran out and moved to another: the runner's line
// about it (role "note", backend API 79) shows where it happened and never
// folds into the work, as on the Mac (13 Nov).
test("a login's note stays out of the work, between what came before it and the reply after", () => {
  const note = (text: string, s: number): TurnMessage => ({ seq: ++seq, role: 'note', text, at: at(s) });
  const moved = 'Moved to mike@example.org: mike@example.net hit its 5-hour limit, back at 11:30 am.';
  const rows = [
    user('land it', 0),
    tool('Ran tb land', 5),
    said("You've hit your 5-hour limit", 60, { turn: 'aaaa1111' }),
    note(moved, 61),
    user('Carry on from where you left off.', 62, 'runner'),
    tool('Ran git push', 70),
    said('Landed.', 90, { turn: 'bbbb2222' }),
  ];
  const shown = (items: ReturnType<typeof fold>) => items.map((i) => (i.kind === 'message' || i.kind === 'note' ? `${i.m.role}:${i.m.text}` : i.kind));
  // Folded exactly as the same thread without the note, plus the note.
  const without = shown(fold(rows.filter((m) => m.role !== 'note'), false));
  const withNote = shown(fold(rows, false));
  assert.deepEqual(withNote.filter((k) => !k.startsWith('note:')), without);
  assert.equal(withNote.at(-1), 'assistant:Landed.');
  assert.equal(withNote.at(-2), `note:${moved}`);
  // The login ran out before the turn did anything, so the folded work is
  // all from after the move: the note still goes under the limit's words.
  const early = [user('fix it', 0), said("You've hit your limit", 1, { turn: 'aaaa1111' }), note(moved, 2), user('Carry on.', 3, 'runner'), tool('Edited a file', 4), said('Fixed.', 9, { turn: 'bbbb2222' })];
  assert.deepEqual(shown(fold(early, false)), ['user:fix it', 'worked', "assistant:You've hit your limit", `note:${moved}`, 'assistant:Fixed.']);
  // While the thread waits for room it's the last thing there, under the work.
  const waiting = shown(fold([user('land it', 0), tool('Ran tb land', 5), said("You've hit your 5-hour limit", 60), note('Every login in Claude is out of room. Carrying on at 11:30 am.', 61)], false));
  assert.equal(waiting.at(-1), 'note:Every login in Claude is out of room. Carrying on at 11:30 am.');
  // And a turn still running shows it in place.
  const running = shown(fold([user('go', 0), tool('Read a', 1), note(moved, 2), said('So far', 3)], true));
  assert.deepEqual(running, ['user:go', 'tools', `note:${moved}`, 'assistant:So far']);
  // The time a turn took isn't run on to a note after its reply.
  const turn = splitTurns([user('go', 0), said('Done.', 10), note(moved, 500)]).at(-1)!;
  assert.equal(turn.ms, 10_000);
});
