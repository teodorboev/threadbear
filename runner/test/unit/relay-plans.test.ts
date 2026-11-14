// Another runner's thread on this Mac (relay.ts) draws a Plan first plan as
// its plan card, another lab's review on its turn's end, and the done card's
// loose ends, as that runner's own Mac does. All three were missing in the
// nightly parity run's pictures (12 Nov): the plan folded into the turn's
// work, and the other two never left that runner's row.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { relayEvents, type Card, type RelayRow, type RelayThreadRow } from '../../src/relay.ts';
import type { LabReviewLedger } from '../../src/protocol.ts';

const T = 'thread-on-bruce';
const at = (s: number) => new Date(Date.parse('2026-11-12T10:00:00Z') + s * 1000).toISOString();
const PLAN: RelayRow = { seq: 4, role: 'assistant', text: '# Bots\n\n1. Add a bots table', plan: true, turn: 'aaaa1111', at: at(1) };
const start: RelayRow = { seq: 2, role: 'user', text: 'Plan the bots', turn: 'aaaa1111', at: at(0) };
const idle: RelayThreadRow = { runnerId: 'bruce', status: 'idle', needs: 'you' };

const outcome = (plan: RelayRow, after: RelayRow[] = []) =>
  relayEvents(T, [start, plan, ...after], idle, [], false, 'bruce').find((e) => e.type === 'plan.resolved' && e.itemId === `relay-${plan.seq}`)?.decision;

test('a plan row is a plan card, not a reply folded into the work', () => {
  const ev = relayEvents(T, [start, PLAN], idle, [], false, 'bruce');
  const plan = ev.find((e) => e.type === 'plan.proposed');
  assert.ok(plan, 'a plan.proposed');
  assert.equal(plan.itemId, 'relay-4');
  assert.equal(plan.text, PLAN.text);
  assert.equal(ev.some((e) => e.type === 'assistant.text' && e.text === PLAN.text), false);
  assert.equal(ev.some((e) => e.type === 'plan.resolved'), false, 'nothing after it: still waiting');
});

test('a plan row from an older backend, with no flag, is a reply as before', () => {
  const { plan: _plan, ...asText } = PLAN;
  const ev = relayEvents(T, [start, asText], idle, [], false, 'bruce');
  assert.equal(ev.some((e) => e.type === 'plan.proposed'), false);
  assert.ok(ev.some((e) => e.type === 'assistant.text' && e.text === PLAN.text));
});

test('what became of a plan is what its runner says, never a guess from what came after', () => {
  const turn = 'aaaa1111';
  assert.equal(outcome({ ...PLAN, planDecision: 'go' }), 'go');
  assert.equal(outcome({ ...PLAN, planDecision: 'revise' }), 'revise');
  // Work after it, Mike writing, a schedule's message: none of them say
  // which (a denied plan's revision reads files too, Codex's review).
  assert.equal(outcome(PLAN, [{ seq: 6, role: 'tool', text: 'Read ~/x.ts', toolKind: 'read', turn, at: at(2) }]), undefined);
  assert.equal(outcome(PLAN, [{ seq: 6, role: 'user', text: 'Use Postgres instead', turn: 'bbbb2222', at: at(5) }]), undefined);
  assert.equal(outcome(PLAN, [{ seq: 6, role: 'user', text: 'Nightly check', origin: 'schedule', turn: 'bbbb2222', at: at(5) }]), undefined);
  // A newer plan sends it back, as the runner does.
  assert.equal(outcome(PLAN, [{ ...PLAN, seq: 8, text: '# Bots, take two' }]), 'revise');
});

test('a resolution that comes later goes in its own seq, after the plan', () => {
  const waiting = relayEvents(T, [start, PLAN], idle, [], false, 'bruce');
  const resolved = relayEvents(T, [start, { ...PLAN, planDecision: 'go' }], idle, [], false, 'bruce');
  const proposed = resolved.find((e) => e.type === 'plan.proposed')!;
  const went = resolved.find((e) => e.type === 'plan.resolved')!;
  assert.equal(went.decision, 'go');
  assert.ok(went.seq > proposed.seq);
  assert.equal(waiting.some((e) => e.seq === went.seq), false, 'a seq the app hasn\'t been given yet');
});

test("Claude's plan card goes with the plan's card, only once the plan is here as one", () => {
  const pending = { approvalId: 'a1', kind: 'approval' as const, seq: 5, name: 'ExitPlanMode', title: 'Finish the plan', options: ['deny'], planSeq: 4 };
  const row: RelayThreadRow = { ...idle, status: 'waiting', pendingApproval: pending };
  const card: Card = { pending, slot: 5 * 8 + 1 };
  const withPlan = relayEvents(T, [start, PLAN], row, [card], false, 'bruce').find((e) => e.type === 'approval.requested');
  assert.equal(withPlan?.itemId, 'relay-4');
  // Without the plan row (an older backend's, or not read yet) the card
  // stands on its own: one naming a plan that isn't drawn would show nothing.
  const { plan: _plan, ...asText } = PLAN;
  const without = relayEvents(T, [start, asText], row, [card], false, 'bruce').find((e) => e.type === 'approval.requested');
  assert.equal(without?.itemId, undefined);
});

test("another lab's review goes on its own turn's end, and no other", () => {
  const review: LabReviewLedger = { reviewer: 'Codex (codex-1)', independent: true, rounds: 1, threadId: 'r1', passed: false, findings: [{ id: 1, round: 1, severity: 'high', claim: 'Drops the session on refresh.', outcome: 'fixed' }] };
  const reply: RelayRow = { seq: 6, role: 'assistant', text: 'Fixed it.', turn: 'aaaa1111', at: at(3) };
  const end = (cardTurn?: string) => relayEvents(T, [start, reply], { ...idle, needs: 'nothing', review, ...(cardTurn ? { cardTurn } : {}) }, [], false, 'bruce').filter((e) => e.type === 'turn.completed').at(-1);
  assert.deepEqual(end('aaaa1111')?.review, review);
  // The row is a newer turn's whose rows haven't come yet (Codex's review).
  assert.equal(end('cccc3333')?.review, undefined);
  // An older runner's row says no turn: not shown, rather than on the wrong card.
  assert.equal(end()?.review, undefined);
});

// Daybreak 12 Nov (finding 105): each plan copied every row after it to see
// whether a newer plan came, quadratic in a run of plans. The newest plan is
// found once now; relay.test.ts times the real path, where a watch holds at
// most MAX_HELD rows.
test('in a run of plans each but the newest was sent back', () => {
  const plans: RelayRow[] = Array.from({ length: 2_000 }, (_, i) => ({ seq: 10 + i, role: 'assistant', text: 'p', plan: true, turn: 'aaaa1111', at: at(1) }));
  const ev = relayEvents(T, [start, ...plans, { ...plans[0]!, seq: 5000, planDecision: 'go' }, { seq: 5001, role: 'tool', text: 'Read x', toolKind: 'read', turn: 'aaaa1111', at: at(2) }], idle, [], false, 'bruce');
  const resolved = ev.filter((e) => e.type === 'plan.resolved');
  assert.equal(resolved.length, plans.length + 1);
  assert.ok(resolved.slice(0, -1).every((e) => e.decision === 'revise'));
  // The newest says what became of it itself.
  assert.deepEqual([resolved.at(-1)!.itemId, resolved.at(-1)!.decision], ['relay-5000', 'go']);
  // With no decision and none after it, the newest still waits.
  const waiting = relayEvents(T, [start, ...plans], idle, [], false, 'bruce').filter((e) => e.type === 'plan.resolved');
  assert.equal(waiting.some((e) => e.itemId === `relay-${plans.at(-1)!.seq}`), false);
});
