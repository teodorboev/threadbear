// Another lab's review of a turn's change (docs/protocol.md, "Review by
// another lab"): the thread row's `review` (backend API 76), for the turn its
// `cardTurn` names. Worded as the Mac's row says it (desktop/core
// LabReview::heading and rows/lab_review.rs finding_detail), so the three
// read the same.

export type ReviewFinding = { id: number; round: number; severity: string; claim: string; where?: string; case?: string; outcome?: string; note?: string };
export type LabReviewLedger = { reviewer: string; independent: boolean; rounds: number; threadId: string; passed: boolean; unstructured?: boolean; unchecked?: boolean; findings: ReviewFinding[] };

/** "Reviewed by Codex: all answered". The login ("Codex (codex-1)") is left off. */
export function reviewHeading(r: LabReviewLedger): string {
  const name = r.reviewer.split(' (')[0] || r.reviewer;
  const who = r.independent ? `Reviewed by ${name}` : `Reviewed by a fresh ${name} session`;
  const rounds = r.rounds > 1 ? `, ${r.rounds} rounds` : '';
  const open = r.findings.filter((f) => !f.outcome || f.outcome === 'left').length;
  const result = r.unstructured
    ? 'see its reply'
    : r.unchecked
      ? 'last fixes not re-reviewed'
      : r.passed
        ? r.findings.length
          ? 'passed'
          : 'nothing found'
        : open > 0
          ? `${open} open`
          : 'all answered';
  return `${who}${rounds}: ${result}`;
}

export type Outcome = 'fixed' | 'disputed' | 'left' | 'none';

/** What was done about a finding, as its mark and word. */
export function outcomeOf(f: ReviewFinding): { kind: Outcome; word: string } {
  switch (f.outcome) {
    case 'fixed':
      return { kind: 'fixed', word: 'Fixed' };
    case 'disputed':
      return { kind: 'disputed', word: 'Disputed' };
    case 'left':
      return { kind: 'left', word: 'Left for you' };
    default:
      return { kind: 'none', word: 'No answer' };
  }
}

/** The line under a finding: what was done, the agent's note, how serious, and where. */
export function findingDetail(f: ReviewFinding): string {
  let detail = outcomeOf(f).word;
  if (f.note) detail += `: ${f.note}`;
  if (f.severity === 'high') detail += ' · serious';
  if (f.where) detail += ` · ${f.where}`;
  return detail;
}

/**
 * The seq of the reply the review goes over: the last assistant message of the
 * turn the row says it's from. None when that turn's reply isn't loaded, or
 * the row is an older runner's with no turn: not shown, rather than on the
 * wrong reply.
 */
export function reviewSeq(messages: { seq: number; role: string; turn?: string; plan?: boolean }[], cardTurn: string | undefined): number | undefined {
  if (!cardTurn) return undefined;
  for (let i = messages.length - 1; i >= 0; i--) {
    const m = messages[i]!;
    if (m.role === 'assistant' && !m.plan && m.turn === cardTurn) return m.seq;
  }
  return undefined;
}
