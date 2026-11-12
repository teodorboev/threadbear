// Another lab's review on the website, worded as the Mac's row says it.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { findingDetail, reviewHeading, reviewSeq, type LabReviewLedger } from './lab-review.ts';

const ledger: LabReviewLedger = {
  reviewer: 'Codex (codex-1)',
  independent: true,
  rounds: 1,
  threadId: 'r1',
  passed: false,
  findings: [
    { id: 1, round: 1, severity: 'high', claim: 'Drops the session on refresh.', where: 'src/app.ts:3', outcome: 'fixed' },
    { id: 2, round: 1, severity: 'low', claim: 'Logs the token.', outcome: 'disputed', note: 'It logs a hash.' },
  ],
};

test('the heading says who reviewed and how it came out, without the login', () => {
  assert.equal(reviewHeading(ledger), 'Reviewed by Codex: all answered');
  assert.equal(reviewHeading({ ...ledger, rounds: 2, findings: [{ ...ledger.findings[0]!, outcome: undefined }] }), 'Reviewed by Codex, 2 rounds: 1 open');
  assert.equal(reviewHeading({ ...ledger, passed: true, findings: [] }), 'Reviewed by Codex: nothing found');
  assert.equal(reviewHeading({ ...ledger, independent: false, unstructured: true }), 'Reviewed by a fresh Codex session: see its reply');
});

test("a finding's line says what was done, the note, how serious and where", () => {
  assert.equal(findingDetail(ledger.findings[0]!), 'Fixed · serious · src/app.ts:3');
  assert.equal(findingDetail(ledger.findings[1]!), 'Disputed: It logs a hash.');
  assert.equal(findingDetail({ id: 3, round: 1, severity: 'medium', claim: 'x' }), 'No answer');
});

test("it goes over its own turn's reply, and nowhere without one", () => {
  const messages = [
    { seq: 1, role: 'user', turn: 'aaaa1111' },
    { seq: 3, role: 'assistant', turn: 'aaaa1111' },
    { seq: 5, role: 'assistant', turn: 'aaaa1111' },
    { seq: 7, role: 'user', turn: 'bbbb2222' },
  ];
  assert.equal(reviewSeq(messages, 'aaaa1111'), 5);
  assert.equal(reviewSeq(messages, 'bbbb2222'), undefined, 'its reply not here yet');
  assert.equal(reviewSeq(messages, undefined), undefined, "an older runner's row");
});
