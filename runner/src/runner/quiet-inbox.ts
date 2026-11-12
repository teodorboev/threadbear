// A quiet inbox (experiment quietInbox; docs/protocol.md, "A quiet
// inbox"): a finished thread with nothing for Mike keeps out of it.
//
// Mike, 12 Nov 2026, of three threads other threads had started (a bun fix
// split off a Let's Build It thread, two GPUI fixes from the port's
// thread): "there's nothing for me to do... it should just auto archive."
// And of a done card listing four threads its thread had started that
// were still building: "If that thread is waiting on another thread, then
// it shouldn't be in the inbox." Before this a thread only archived itself
// when asked (end_turn's archive), and only waited on others when its
// agent said so (waitFor).
//
// Each method's `this` is the Runner; runner.ts puts them on its prototype.
import type { Runner, ThreadState } from '../runner.ts';
import type { RunnerPart } from './part.ts';
import type { LooseEnd } from '../protocol.ts';
import { sentByAgent } from '../protocol.ts';
import { experimentOn } from '../experiments.ts';

export const QUIET_INBOX = 'quietInbox';
export const HELPER_ARCHIVED_NOTE = 'Archived: another thread started this one, and it finished with nothing left for you.';

export const quietInboxPart = {
  // After a turn that finished for good (completeTurn's `finished`, with no
  // review of it to come). `next`: whether it suggested follow-ups, which
  // are Mike's to tick or leave.
  async settleQuietly(this: Runner, st: ThreadState, next: boolean): Promise<void> {
    if (!experimentOn(this.cfg.home, QUIET_INBOX)) return;
    const { rec, rt } = st;
    const seq = this.looseEndsCard(st);
    if (seq === undefined) return;
    let ends: LooseEnd[];
    try {
      // The done card's own check of this card, if it's going (looseEnds).
      ends = await (rt.looseEndsGather ?? this.gatherLooseEnds(st));
    } catch (e) {
      // Not known to be clean, so it stays where Mike can see it.
      this.log.warn("couldn't check a finished thread's loose ends", { threadId: rec.id, error: String(e) });
      return;
    }
    // Git took a moment. Anything since (a message, a new turn, Mike or a
    // schedule putting it away, the runner stopping) decides instead.
    if (rt.deleted || this.stopping || rt.turn || rec.queue.length || this.looseEndsCard(st) !== seq || this.bucket(rec) !== 'inbox') return;
    // The threads it started, as they stand now rather than when git began.
    const open = [...ends.filter((e) => e.kind !== 'thread' && e.kind !== 'scheduled'), ...this.childEnds(st)];
    const waits = open.filter((e) => e.kind === 'thread' && e.threadId).map((e) => ({ id: e.threadId! }));
    if (waits.length && waits.length === open.length) {
      // All that's left is theirs to finish: it waits on them, as end_turn's
      // waitFor would have, and wakes with what they said. No longer
      // finished itself, for anything waiting on it in turn.
      delete rec.finished;
      this.waitOn(st, waits, undefined);
      // So its wake says the runner put it away, not the agent (deliverWaits).
      for (const w of rec.waitingOn ?? []) w.quiet = true;
      this.markDirty(st);
      this.log.info('quiet inbox: waiting on the threads it started', { threadId: rec.id, on: waits.map((w) => w.id) });
      return;
    }
    // A check-in it booked brings it back by itself; it goes after that run.
    if (open.length || ends.some((e) => e.kind === 'scheduled')) return;
    if (!this.helperNobodyTouched(st) || next) return;
    this.emit(st, { type: 'system.note', text: HELPER_ARCHIVED_NOTE });
    this.update({ threadId: rec.id, archived: true, archivedBy: 'threadbear' });
    this.log.info('quiet inbox: archived a finished helper thread', { threadId: rec.id, startedBy: rec.startedBy?.threadId });
  },

  // One another thread's agent started that Mike has had nothing to do
  // with: never written in (answering its card counts), starred, or taken
  // back out of the archive. A tool's, a schedule's or the librarian's is
  // often the thing he wants to read, and has no thread holding its result.
  helperNobodyTouched(this: Runner, st: ThreadState): boolean {
    const { rec } = st;
    if (rec.startedBy?.kind !== 'thread' || rec.starredAt || rec.keptOut || rec.reviewOf) return false;
    // Its first message is its starter's brief, whatever it looks like (`tb
    // new` from an agent's shell has no "Sent by the agent" line, so
    // rec.asked can't be trusted for it). Anything of Mike's after that
    // makes it his.
    const log = this.store.log(rec.id);
    const said = log.read(1, log.lastSeq).filter((e) => e.type === 'user.message');
    return !said.slice(1).some((m) => (!m.origin || m.origin === 'phone') && !sentByAgent(m.text));
  },
};

export type QuietInboxPart = RunnerPart<typeof quietInboxPart>;
