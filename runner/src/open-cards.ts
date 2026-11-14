// The cards (approvals, questions, secrets) a thread's log still has open.
// Its own file so the draining runner (runner/handovers.ts, drain.openCards)
// and the primary (handover.ts, Peers.openCards) share it without importing
// each other.
import type { PersistedEvent } from './protocol.ts';

// The requests in a thread's events that nothing has answered yet.
export function openCardEvents(events: PersistedEvent[]): PersistedEvent[] {
  const answered = new Set(events.flatMap((e) => (e.type === 'approval.resolved' ? [e.approvalId] : [])));
  return events.filter((e) => (e.type === 'approval.requested' || e.type === 'question.requested' || e.type === 'secret.requested') && !answered.has(e.approvalId));
}
