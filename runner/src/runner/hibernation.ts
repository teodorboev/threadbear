// Snoozing and hibernating: when a thread wakes, by time, by topic or by restart.
// Part of the Runner (runner.ts), split out on 4 Nov 2026 as a plain move so a
// fork's change to one area doesn't clash with another's. Each method's `this`
// is the Runner; runner.ts puts them on its prototype.
import type { Runner } from '../runner.ts';
import type { RunnerPart } from './part.ts';
import type { ThreadSummary } from '../protocol.ts';
import { ErrorCode, RpcError } from '../protocol.ts';
import { SAID_MAX, WAKE_SYSTEM, awayFor, hibernateNote, localWhen, readWakeAnswer, wakePrompt } from '../wake-when.ts';
import type { ThreadState } from '../runner.ts';
import { NO_WAKE_TIME } from '../runner.ts';
import { parseWhen } from '../when.ts';
import type { WaitFor } from '../needs.ts';
import path from 'node:path';
import { providerName } from '../providers.ts';
import { LAB_REVIEW_REPLY_CHARS, addVerdict, findingLine } from './lab-review.ts';
import { cutSafely, safeEnd } from '../safe-cut.ts';
import { above, raises, type Authority } from '../threadbear-mcp.ts';

// A thread that isn't here (on another runner of this Mac's) counts as the
// least access, so what it said is never passed up on a guess.
const LEAST: Authority = { mode: 'ask', confined: true };

// The less of two: the lower mode, and in a sandbox if either is.
export function leastAuthority(a: Authority | undefined, b: Authority): Authority {
  if (!a) return b;
  return { mode: raises(a.mode, b.mode) ? b.mode : a.mode, confined: a.confined || b.confined };
}

// How long a thread waiting on others sleeps at most when end_turn gave no
// snooze, so one that never hears still comes back.
export const WAIT_FOR_DEFAULT_MS = 86_400_000;
// How much of each finished thread's reply the waiting one is handed.
export const WAIT_FOR_REPLY_CHARS = 2000;

export const hibernationPart = {
  // Hibernates a thread until when Mike said, in his own words
  // (wake-when.ts), as Opus reads them: a time, a topic for the librarian
  // to listen for, or both. The thread goes away at once, as a preset
  // does, and the time follows once the model answers; if it can't say,
  // the thread comes back and the call fails with why.
  // `by: 'agent'`: its own agent asked (snoozeAfterTurn), waiting for
  // something to happen (WakeWhen.by).
  async hibernateWhen(this: Runner, params: { threadId: string; said: string; by?: 'agent' }): Promise<{ thread: ThreadSummary; until?: string; topic?: string; why: string }> {
    const st = this.mustGet(params.threadId);
    const said = params.said.trim();
    if (!said) throw new RpcError(ErrorCode.badParams, 'Say when it should wake.');
    if (said.length > SAID_MAX) throw new RpcError(ErrorCode.badParams, `That's more than ${SAID_MAX} characters.`);
    const models = this.models;
    if (!models) throw new RpcError(ErrorCode.other, 'Model calls are off in this runner.');
    const timeZone = Intl.DateTimeFormat().resolvedOptions().timeZone;
    const gist = this.threadGist(st.rec.id, { recent: 0 });
    const by = params.by === 'agent' ? ('agent' as const) : undefined;
    const input = { said, now: this.clock.now(), timeZone, title: st.rec.title, first: gist.first, brief: st.rec.brief?.text, ...(by ? { by } : {}) };
    this.update({ threadId: st.rec.id, snoozedUntil: NO_WAKE_TIME });
    st.rec.wakeWhen = { said, pending: true };
    this.markDirty(st);
    const ask = (withThread: boolean) =>
      models.generate({
        feature: 'wake-when',
        ...(withThread ? { cwd: st.rec.cwd } : {}),
        system: WAKE_SYSTEM,
        prompt: wakePrompt(withThread ? input : { said, now: input.now, timeZone, title: '(private)', ...(by ? { by } : {}) }),
        maxTokens: 300,
        strong: true,
      });
    let res = await ask(true);
    // A private folder's thread isn't sent: his words alone are usually enough.
    if (!res.ok && res.reason === 'private') res = await ask(false);
    // Woken, hibernated another way or deleted meanwhile: that wins.
    if (st.rt.deleted || st.rec.wakeWhen?.pending !== true || st.rec.wakeWhen.said !== said) {
      throw new RpcError(ErrorCode.other, 'The thread was woken or put away again while this was worked out.');
    }
    const answer = res.ok ? readWakeAnswer(res.value, this.clock.now()) : { error: res.message || `No answer from the model (${res.reason}).` };
    if ('error' in answer) {
      delete st.rec.snoozedUntil;
      delete st.rec.wakeWhen;
      st.rec.updatedAt = this.isoNow();
      this.markDirty(st);
      this.armSnoozeTimer();
      if (this.bucket(st.rec) === 'inbox') this.kickQueue(st);
      throw new RpcError(ErrorCode.other, `Couldn't work out when to wake it: ${answer.error}`);
    }
    st.rec.snoozedUntil = answer.until ?? NO_WAKE_TIME;
    st.rec.wakeWhen = { said, ...(answer.topic ? { topic: answer.topic } : {}), ...(by ? { by } : {}) };
    // What it already checked counts only for the same words, from its agent.
    if (st.rec.wakeChecked && (!by || st.rec.wakeChecked.said !== said)) delete st.rec.wakeChecked;
    this.emit(st, { type: 'system.note', text: hibernateNote(answer, said, timeZone, by) });
    this.armSnoozeTimer();
    this.log.info('hibernated in words', { threadId: st.rec.id, until: answer.until, topic: answer.topic });
    return { thread: this.summary(st), ...answer };
  },

  // Threads hibernating until Mike next talks about something, or (`happened`)
  // until something their agent waits for has happened, for the librarian
  // to listen for.
  waitingTopics(this: Runner): { threadId: string; title: string; topic: string; happened?: true }[] {
    const now = this.clock.now();
    const out: { threadId: string; title: string; topic: string; happened?: true }[] = [];
    for (const { rec, rt } of this.threads.values()) {
      const topic = rec.wakeWhen?.topic;
      if (!topic || rt.deleted || rec.archivedAt || !rec.snoozedUntil || Date.parse(rec.snoozedUntil) <= now) continue;
      out.push({ threadId: rec.id, title: rec.title, topic, ...(rec.wakeWhen?.by === 'agent' ? { happened: true as const } : {}) });
    }
    return out;
  },

  // The librarian heard Mike talk about a hibernating thread's topic in
  // another thread: it wakes, unread, saying where. One its agent put to
  // sleep until something happened gets a turn as well, to check it really
  // has (the librarian reads one thread, and can be wrong) and go back to
  // sleep if not, rather than sitting in the inbox with nothing done.
  wakeForTopic(this: Runner, threadId: string, inThreadId: string, shown?: { topic: string; happened: boolean }): boolean {
    const st = this.threads.get(threadId);
    const wakeWhen = st?.rec.wakeWhen;
    const topic = wakeWhen?.topic;
    if (!st || !wakeWhen || !topic || st.rt.deleted || st.rec.archivedAt || threadId === inThreadId || this.bucket(st.rec) !== 'snoozed') return false;
    // Not the wait the librarian judged: set again while its model ran.
    if (shown && (shown.topic !== topic || shown.happened !== (wakeWhen.by === 'agent'))) return false;
    const where = this.threads.get(inThreadId)?.rec.title;
    if (wakeWhen.by !== 'agent') {
      this.wake(st, { why: `you talked about ${topic}${where ? ` in "${where}"` : ' in another thread'}` });
      this.armSnoozeTimer();
      this.log.info('woke for a topic', { threadId, inThreadId, topic });
      return true;
    }
    // Already woken from this thread for these words, and it has said
    // nothing since: the agent checked that claim and went back to sleep.
    const seq = this.threads.get(inThreadId)?.rec.lastSeq ?? 0;
    const checked = st.rec.wakeChecked?.said === wakeWhen.said ? st.rec.wakeChecked : undefined;
    if (checked && (checked.from[inThreadId] ?? -1) >= seq) return false;
    st.rec.wakeChecked = { said: wakeWhen.said, from: { ...checked?.from, [inThreadId]: seq } };
    this.wake(st, { why: `to check whether ${topic}${where ? `, from "${where}"` : ''}` });
    this.armSnoozeTimer();
    // Which thread, only where send_message would let it say so without a
    // card: its id is the way in to its words (read_thread).
    const tell = this.mayTellOf(this.authorityNow(st.rec.id), inThreadId);
    const seen = tell ? `another thread (${inThreadId.slice(0, 8)}) looks to show it has` : 'another thread looks to show it has';
    const said = wakeWhen.said.replace(/\s+/g, ' ').trim();
    this.sendTo(
      st,
      `You hibernated this thread until: ${said}\n\nThreadbear woke it because ${seen} (${topic}), but that's a guess from reading one thread. Check for yourself that it really has${tell ? ' (read_thread on that thread, and the work itself)' : ''}. If it has, carry on with what you meant to do. If it hasn't, call end_turn with needs "nothing" and snooze "${said.replace(/"/g, "'")}" to go back to sleep, and say in a line what's still missing.`,
      [],
      { origin: 'runner', queue: true },
    );
    this.log.info('woke to check what it waited for', { threadId, inThreadId, topic });
    return true;
  },

  // end_turn's waitFor, once the turn is over: hibernates until every
  // named thread finishes a turn with nothing left (or is archived or
  // deleted), then wakes with what they said. A thread that never hears
  // wakes at the snooze time, or after a day. Before 5 Nov 2026 a thread
  // waiting on another's report could only end as done, in the inbox.
  // `askedBy`: its access when its agent asked to wait (end_turn), if it
  // has more now, as `by` is the less of the two.
  waitOn(this: Runner, st: ThreadState, waits: WaitFor[], when: string | undefined, askedBy?: Authority): void {
    const { rec } = st;
    const list: { threadId: string; title: string; until?: string }[] = [];
    const skipped: string[] = [];
    const ids = waits.map((w) => w.id);
    for (const { id, until } of waits) {
      const hits = [...this.threads.values()].filter((t) => !t.rt.deleted && (t.rec.id === id || t.rec.id.startsWith(id)));
      let found: { id: string; title: string; archived: boolean } | undefined;
      if (hits.length === 1) found = { id: hits[0].rec.id, title: hits[0].rec.title, archived: Boolean(hits[0].rec.archivedAt) };
      else if (!hits.length) {
        // On another runner of this Mac's (one that's draining, or the new
        // one it moved to while this turn ended): waited on all the same,
        // and checked when either comes across.
        const there = this.store.ids().filter((n) => n === id || n.startsWith(id));
        const rec = there.length === 1 && !this.threads.has(there[0]) ? this.store.load(there[0]) : undefined;
        if (rec) found = { id: rec.id, title: rec.title, archived: Boolean(rec.archivedAt) };
      }
      if (!found || found.id === rec.id || found.archived) skipped.push(id);
      else if (!list.some((x) => x.threadId === found.id)) list.push({ threadId: found.id, title: found.title, ...(until ? { until } : {}) });
    }
    if (!list.length) {
      this.emit(st, { type: 'system.note', text: `Not hibernated: none of the threads it was to wait on (${ids.join(', ')}) are here and still going.` });
      return;
    }
    const now = this.clock.now();
    let until: string | undefined;
    if (when && !['restart', 'release'].includes(when.toLowerCase())) {
      try {
        until = parseWhen(when, now);
      } catch {
        until = undefined;
      }
    }
    if (!until || Date.parse(until) <= now) until = new Date(now + WAIT_FOR_DEFAULT_MS).toISOString();
    this.update({ threadId: rec.id, snoozedUntil: until });
    const by = leastAuthority(askedBy, this.authorityNow(rec.id));
    rec.waitingOn = list.map((x) => ({ ...x, by }));
    this.markDirty(st);
    const timeZone = Intl.DateTimeFormat().resolvedOptions().timeZone;
    const also = skipped.length ? ` (${skipped.join(', ')} isn't a thread here that's still going, so it isn't waited on.)` : '';
    this.emit(st, { type: 'system.note', text: `Hibernating until ${waitsLine(list)}, or ${localWhen(Date.parse(until), timeZone)} at the latest.${also}` });
    // One may have finished already (while this turn was ending, or before
    // it with nothing new since): it wakes now with that reply.
    this.reconcileWaits();
    // One waited on for something may have done it already, and had its
    // look before this wait was there to be judged: one more look.
    for (const x of list) if (x.until && this.threads.has(x.threadId)) this.tellActivity({ kind: 'waited-on', threadId: x.threadId });
  },

  // A review of this thread's change has started (runner/lab-review.ts):
  // it hibernates until the review finishes, as end_turn's waitFor would.
  waitOnReview(this: Runner, st: ThreadState, reviewId: string): void {
    this.waitOn(st, [{ id: reviewId }], undefined);
    const w = st.rec.waitingOn?.find((x) => x.threadId === reviewId);
    if (w) {
      w.review = true;
      this.markDirty(st);
    }
  },

  // A turn finished with nothing needed and nothing left running: kept with
  // its reply, then any thread waiting on it hears.
  markFinished(this: Runner, st: ThreadState, lastText: string | undefined): void {
    // A review's findings are the point, so more of them go across.
    const max = st.rec.reviewOf ? LAB_REVIEW_REPLY_CHARS : WAIT_FOR_REPLY_CHARS;
    let reply = lastText?.trim() ?? '';
    if (!reply) {
      try {
        reply = this.threadGist(st.rec.id, { recent: 1, chars: max }).recent.filter((x) => x.who === 'assistant').at(-1)?.text ?? '';
      } catch {
        reply = '';
      }
    }
    if (reply.length > max) reply = reply.slice(0, safeEnd(reply, max)) + '…';
    st.rec.finished = { at: this.isoNow(), ...(reply ? { reply } : {}) };
    this.markDirty(st);
    this.reconcileWaits();
  },

  waitedOnDeleted(this: Runner, threadId: string): void {
    for (const st of this.threads.values()) {
      for (const w of st.rec.waitingOn ?? []) {
        if (w.threadId === threadId && !w.done) {
          w.done = { at: this.isoNow(), how: 'deleted' };
          this.markDirty(st);
        }
      }
    }
    this.reconcileWaits();
  },

  // Marks each wait whose thread has finished (with no new work since) or
  // been archived, and wakes any thread whose waits are all done with their
  // replies. Run whenever that could have changed: a finish, an archive,
  // a delete, a new wait, a start and a thread coming across from a
  // draining runner. A thread not here (still on that runner) waits.
  reconcileWaits(this: Runner): void {
    if (this.holdWaits) return;
    // One thread at a time: a wait another runner wrote wrong (a moved
    // record's waitingOn) costs that thread, not the others or the start.
    for (const st of this.threads.values()) {
      try {
        this.reconcileWaitsOf(st);
      } catch (e) {
        this.recoveryFailed(st, e);
      }
    }
  },

  reconcileWaitsOf(this: Runner, st: ThreadState): void {
    {
      const waits = st.rec.waitingOn;
      if (!waits || st.rt.deleted || st.rec.archivedAt || this.bucket(st.rec) !== 'snoozed') return;
      for (const w of waits) {
        if (w.done) continue;
        const t = this.threads.get(w.threadId);
        if (!t || t.rt.deleted) continue;
        const fin = t.rec.finished;
        // One waiting for something in particular hears from the librarian
        // (waitMet), not from any finish.
        if (fin && !w.until) w.done = { at: fin.at, how: 'finished', ...(fin.reply ? { reply: fin.reply } : {}) };
        else if (t.rec.archivedAt) w.done = { at: t.rec.archivedAt, how: 'archived' };
        else continue;
        w.title = t.rec.title;
        this.markDirty(st);
      }
      if (waits.every((x) => x.done)) this.deliverWaits(st, waits);
    }
  },

  // What a thread's agent can do now (threadbear-mcp.ts, Authority).
  authorityNow(this: Runner, threadId: string): Authority {
    if (!this.threads.has(threadId)) return LEAST;
    return { mode: this.accessOf(threadId), confined: this.confinedOf(threadId) };
  },

  // Whether a thread with access `me` may be handed another thread's words
  // without a card: the other has no less access (mode or sandbox), as
  // send_message goes by. One not here counts as the least.
  mayTellOf(this: Runner, me: Authority, threadId: string): boolean {
    return !above(me, this.authoritySaved(threadId));
  },

  // What a thread's agent is held to as a sender: its saved mode, as
  // send_message goes by (threadbear-mcp.ts, restricted), not the more a
  // running turn may still have after it was lowered (Codex's review of
  // Daybreak finding 111). One not here counts as the least.
  authoritySaved(this: Runner, threadId: string): Authority {
    const st = this.threads.get(threadId);
    if (!st) return LEAST;
    return { mode: st.rec.permissionMode ?? 'ask', confined: Boolean(st.rec.confined) };
  },

  // Whether what a waited-on thread said may go to the waiting thread's
  // agent as a message, as send_message would let it without a card: the
  // waited-on thread has no less access than the waiting one has now, or the
  // waiting one's agent asked for it itself (end_turn's waitFor) and hasn't
  // been raised since. A wait the quiet inbox made was never asked for, so
  // it counts only the first (Daybreak, 12 Nov 2026: an Ask helper's closing
  // reply started a turn in its Full-access starter).
  mayHear(this: Runner, me: Authority, w: NonNullable<ThreadState['rec']['waitingOn']>[number]): boolean {
    if (this.mayTellOf(me, w.threadId)) return true;
    return !w.quiet && w.by !== undefined && !above(me, w.by);
  },

  deliverWaits(this: Runner, st: ThreadState, waits: NonNullable<ThreadState['rec']['waitingOn']>): void {
    // Every wait, whatever it carries: a title is the waited-on thread's
    // own words too (update_thread), and a review's findings the reviewer's
    // (Codex's review of the Daybreak fix, 12 Nov 2026).
    const me = this.authorityNow(st.rec.id);
    const held = waits.filter((x) => !this.mayHear(me, x));
    if (!held.length && waits.length === 1 && waits[0].review) return this.deliverReview(st, waits[0]);
    if (held.length) {
      // Woken for Mike instead, with no turn: he reads them and says what next.
      const names = held.map((x) => `"${x.title}"`).join(', ');
      const one = held.length === 1;
      this.wake(st, { why: one ? `${names} finished` : 'the threads it was waiting on finished' });
      this.armSnoozeTimer();
      this.emit(st, { type: 'system.note', text: `${names} ${one ? 'has' : 'have'} less access than this thread, so what ${one ? 'it' : 'they'} said wasn't passed to this thread's agent without you. Read ${one ? 'it' : 'them'}, and send a message here to carry on.` });
      this.log.info('woke without passing on a reply from a thread with less access', { threadId: st.rec.id, held: held.map((x) => x.threadId) });
      return;
    }
    const parts = waits.map((x) => {
      const head = `"${x.title}" (thread ${x.threadId.slice(0, 8)})`;
      const said = x.done?.how === 'deleted' ? 'was deleted' : x.done?.how === 'archived' ? 'was archived'
        : x.done?.how === 'met' ? `has done what you waited for (${x.until}): ${x.done.why || 'the librarian read it so'}` : 'finished';
      return x.done?.reply ? `${head} ${said}. Its last reply:\n\n${x.done.reply}` : `${head} ${said}.`;
    });
    const one = waits.length === 1;
    const why = one ? `"${waits[0].title}" finished` : 'the threads it was waiting on finished';
    // A wait the runner started (experiment quietInbox, quiet-inbox.ts): the
    // agent never said it would wait, and may have nothing more to do.
    const text = waits.every((x) => x.quiet)
      ? `Threadbear hibernated this thread after your last turn, as all it had left open was the thread${one ? '' : 's'} it started. ${one ? 'It has' : 'They have'} finished now.\n\n${parts.join('\n\n')}\n\nIf that leaves something to do, or to tell the user, do it now; if not, say so in a line. read_thread has the rest of ${one ? 'it' : 'them'}.`
      : `The thread${one ? '' : 's'} you hibernated to wait on ${one ? 'has' : 'have'} finished.\n\n${parts.join('\n\n')}\n\nCarry on from here. read_thread has the rest of ${one ? 'it' : 'them'}.`;
    this.wake(st, { why });
    this.armSnoozeTimer();
    this.sendTo(st, text, [], { origin: 'runner', queue: true });
    this.log.info('woke after waiting on threads', { threadId: st.rec.id, on: waits.map((x) => x.threadId) });
  },

  // A review of this thread's change has finished (or gone): it wakes with
  // the findings and is asked to fix what holds up. The review thread is
  // archived, its job done; Mike can still open it from the link.
  deliverReview(this: Runner, st: ThreadState, w: NonNullable<ThreadState['rec']['waitingOn']>[number]): void {
    const id8 = w.threadId.slice(0, 8);
    const reviewer = this.threads.get(w.threadId);
    const who = reviewer ? providerName(reviewer.rec.provider) : 'The reviewer';
    let text: string;
    if (w.done?.how === 'finished' || w.done?.how === 'met') {
      const edited = reviewer ? [...reviewEdits(this, reviewer)] : [];
      const findings = w.done.reply?.trim() || '(It gave no reply. read_thread has what it did.)';
      const edits = edited.length ? `\n\nIt was asked not to edit files but changed ${edited.join(', ')}. Look at those changes before going on, and keep or undo them.` : '';
      // Its verdict, numbered into the ledger (lab-review.ts): the agent
      // answers each by id in end_turn, and can't end the turn without.
      const ledger = st.rec.labReview;
      const added = ledger ? addVerdict(ledger, reviewer?.rec.reviewVerdict) : [];
      if (ledger) this.markDirty(st);
      const round = ledger && ledger.rounds > 1 ? ` (review round ${ledger.rounds})` : '';
      if (ledger && !ledger.unstructured && !added.length) {
        text = `${who} reviewed the change you just made${round} (thread ${id8}) and found nothing that holds up.${edits}\n\nIts reply:\n\n${findings}\n\nSay so in a line in your reply and finish as you were.`;
      } else if (added.length) {
        text = `${who} reviewed the change you just made${round} (thread ${id8}). Its findings, numbered:\n\n${added.map(findingLine).join('\n')}\n\nIts reply, with the detail:\n\n${findings}${edits}\n\n` +
          'Check each finding against the code: fix what holds up and run the tests again. When you call end_turn, answer every one in its `findings`, by number: `[{ "id": ' + added[0].id + ', "outcome": "fixed" }]`, one per finding, each "fixed", "disputed" with a note saying why it doesn\'t hold up, or "left" with a note saying what the user has to decide (that goes with needs "you" and an ask). Fixes get another review. In your reply, say briefly what it found and what you did about each.';
      } else {
        text = `${who} reviewed the change you just made (thread ${id8}). Its findings:\n\n${findings}${edits}\n\n` +
          'Check each finding against the code: fix what holds up and run the tests again. In your reply, say briefly what it found, what you fixed, and anything you left and why. If it found nothing, say so in a line and finish as you were.';
      }
    } else {
      text = `The review of your change (thread ${id8}) ${w.done?.how === 'deleted' ? 'was deleted' : 'was archived'} before it finished, so it wasn't reviewed. Say so in a line in your reply and finish as you were.`;
    }
    this.wake(st, { why: `${who} reviewed the change` });
    this.armSnoozeTimer();
    this.sendTo(st, text, [], { origin: 'runner', queue: true });
    this.log.info('woke after a lab review', { threadId: st.rec.id, reviewer: w.threadId });
    if (reviewer && !reviewer.rt.deleted && !reviewer.rec.archivedAt && !reviewer.rt.turn) {
      this.update({ threadId: w.threadId, archived: true, archivedBy: 'threadbear' });
    }
  },

  // The waits on this thread for something in particular, for the
  // librarian's look at it.
  waitedOnFor(this: Runner, threadId: string): { threadId: string; title: string; until: string }[] {
    const out: { threadId: string; title: string; until: string }[] = [];
    for (const { rec, rt } of this.threads.values()) {
      if (rt.deleted || rec.archivedAt || this.bucket(rec) !== 'snoozed') continue;
      for (const w of rec.waitingOn ?? []) if (w.threadId === threadId && w.until && !w.done) out.push({ threadId: rec.id, title: rec.title, until: w.until });
    }
    return out;
  },

  // The librarian read in a look at `targetId` that what `waiterId` waits
  // for there has happened: that wait is done, with the thread's latest
  // reply, and the waiter wakes once all of its waits are.
  waitMet(this: Runner, waiterId: string, targetId: string, why: string, until: string): boolean {
    const st = this.threads.get(waiterId);
    // The same condition the look was shown, not one set since.
    const w = st?.rec.waitingOn?.find((x) => x.threadId === targetId && x.until === until && !x.done);
    if (!st || !w || st.rt.deleted || this.bucket(st.rec) !== 'snoozed') return false;
    let reply = '';
    try {
      reply = this.threadGist(targetId, { recent: 2, chars: WAIT_FOR_REPLY_CHARS }).recent.filter((x) => x.who === 'assistant').at(-1)?.text ?? '';
    } catch {
      reply = '';
    }
    w.done = { at: this.isoNow(), how: 'met', ...(reply ? { reply } : {}), ...(why ? { why: cutSafely(why, 200) } : {}) };
    this.markDirty(st);
    this.log.info('a wait was met', { waiterId, targetId, why });
    this.reconcileWaits();
    return true;
  },

  // Whether a starting runner (or one taking a thread over from a draining
  // one) wakes it: hibernating until a restart, or until a new release when
  // this isn't the one it went to sleep on, or a wake-when whose answer was
  // lost with the old runner.
  wakesOnStart(this: Runner, rec: ThreadState['rec']): boolean {
    if (rec.wakeWhen?.pending) return true;
    if (!rec.snoozedUntilRestart) return false;
    return !rec.untilRelease || rec.untilRelease !== this.release;
  },

  armSnoozeTimer(this: Runner): void {
    this.snoozeTimer?.cancel();
    this.snoozeTimer = undefined;
    if (this.stopping) return;
    const now = this.clock.now();
    let next = Infinity;
    for (const st of this.threads.values()) {
      if (!st.rec.snoozedUntil) continue;
      const t = Date.parse(st.rec.snoozedUntil);
      if (t <= now) this.wake(st, 'time');
      else next = Math.min(next, t);
    }
    if (next === Infinity) return;
    // Re-check at least hourly so long snoozes survive clock changes.
    this.snoozeTimer = this.clock.setTimeout(() => this.armSnoozeTimer(), Math.min(next - now, 3_600_000));
  },

  // A thread coming back by itself (its time, a restart, a topic) says so
  // in its conversation, with how long it was away, and its row says
  // "Woke up" until Mike looks (`woke`): otherwise it just turns up in the
  // inbox with nothing to say why (30 Oct).
  wake(this: Runner, st: ThreadState, cause: 'time' | 'restart' | { why: string }): void {
    const { rec } = st;
    // Archived since it went to sleep: its hibernation just ends, quietly.
    // Waking it there gave it a "Woke up" and an unread mark in the
    // archive, and started any message it had queued while still archived
    // (13 Nov 2026, the implicit state machines scan). Brought back later,
    // it's in the inbox, as it would have been.
    if (rec.archivedAt) {
      this.endHibernation(st);
      st.rec.updatedAt = this.isoNow();
      this.markDirty(st);
      return;
    }
    const now = this.clock.now();
    const since = rec.snoozedAt && Date.parse(rec.snoozedAt) <= now ? rec.snoozedAt : undefined;
    // Under a minute (a wake-when cut short by a restart) isn't worth saying.
    const ms = since ? now - Date.parse(since) : 0;
    const away = ms >= 60_000 ? ` after ${awayFor(ms)}` : '';
    const text = typeof cause === 'object'
      ? `Woke up${away}: ${cause.why}.`
      : cause === 'restart'
        ? `Woke up${away}, as the runner restarted.`
        : away ? `Woke up after hibernating for ${awayFor(ms)}.` : 'Woke up from hibernating.';
    // Starting "Woke up" is what gives it the bear in the app (RowContent.isWakeNote).
    this.emit(st, { type: 'system.note', text });
    rec.woke = { at: new Date(now).toISOString(), ...(since ? { since } : {}) };
    this.endHibernation(st);
    st.rec.forceUnread = true;
    st.rec.updatedAt = this.isoNow();
    this.markDirty(st);
    this.kickQueue(st);
  },

  endHibernation(this: Runner, st: ThreadState): void {
    const { rec } = st;
    delete rec.snoozedAt;
    delete rec.snoozedUntil;
    delete rec.snoozedUntilRestart;
    delete rec.untilRelease;
    delete rec.wakeWhen;
    delete rec.waitingOn;
  },
};

export type HibernationPart = RunnerPart<typeof hibernationPart>;

// "\"Convex I/O\" finishes", "\"Convex I/O\" reports the saving", joined.
function waitsLine(list: { title: string; until?: string }[]): string {
  const one = (x: { title: string; until?: string }) => `"${x.title}" ${x.until ? `does this: ${x.until}` : 'finishes'}`;
  if (list.every((x) => !x.until)) return `${list.map((x) => `"${x.title}"`).join(list.length === 2 ? ' and ' : ', ')} ${list.length === 1 ? 'finishes' : 'finish'}`;
  return list.map(one).join('; ');
}

// The files a review thread changed, though it was told not to.
function reviewEdits(runner: Runner, st: ThreadState): Set<string> {
  const out = new Set<string>();
  const last = st.rec.lastSeq ?? 0;
  try {
    for (const e of runner.store.log(st.rec.id).read(1, last)) if (e.type === 'edit.diff') out.add(path.basename((e as unknown as { path: string }).path));
  } catch {
    // Unreadable: said as no edits.
  }
  return out;
}
