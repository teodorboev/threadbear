// How much of a free Convex plan this runner is using, as near as a runner can
// tell, and the once-only question that turns full sync on for an install from
// before it was the default (docs/full-convex.md step 6).
//
// A runner only holds a deploy key and a runner key, and neither can read the
// team's usage: that takes a Convex login (the dashboard, or the API with a
// team token). So this counts what it can: the bytes of every mutation this
// runner sends, by day, kept for 30 days. Measured on Mike's deployment
// (13 Nov 2026, 15 minutes of prod logs), all reads and writes came to 1.7
// times what runners wrote, so the I/O estimate doubles what was sent to leave
// room for phones and the website. Storage is the tool output sent in the last
// 30 days, as that's how long full sync keeps it, plus a third for indexes.
// Both are estimates, and only of this runner: the plan's limits are the team's,
// shared with its other projects and runners.
import fs from 'node:fs';
import path from 'node:path';

const DAY_MS = 24 * 60 * 60_000;
export const WINDOW_DAYS = 30;
// Convex's Free plan (docs.convex.dev/production/state/limits).
export const FREE_IO_BYTES = 1_000_000_000;
export const FREE_STORAGE_BYTES = 500_000_000;
// Warn early: the limits are the team's, and this sees one runner.
export const WARN_AT = 0.7;
// All I/O for each byte a runner writes (1.7 measured), rounded up.
export const IO_PER_SENT_BYTE = 2;
export const STORAGE_PER_BODY_BYTE = 1.3;
// How often the counts are written to disk at most.
const SAVE_MS = 60_000;

type Day = { sent: number; bodies: number };
type File = { days: Record<string, Day> };

export type ConvexUse = {
  // What this runner sent in the last 30 days, and the part that was tool output.
  sent: number;
  bodies: number;
  // Estimates, against Free's monthly I/O and its storage.
  io: number;
  storage: number;
  // When it was first counted, so a count that's only days old says so.
  since: string;
  // Set once either estimate passes WARN_AT of Free's limit.
  warning?: string;
};

export class ConvexUseTally {
  private file: string;
  private now: () => number;
  private days: Record<string, Day> = {};
  private since: number;
  private dirty = false;
  private savedAt = 0;

  constructor(opts: { home: string; now?: () => number }) {
    this.file = path.join(opts.home, 'convex-use.json');
    this.now = opts.now ?? Date.now;
    try {
      const saved = JSON.parse(fs.readFileSync(this.file, 'utf8')) as File;
      // Only well-formed days: it's a cache of counts, and a broken one starts again.
      if (saved?.days && typeof saved.days === 'object' && !Array.isArray(saved.days)) {
        for (const [k, d] of Object.entries(saved.days)) {
          if (/^\d{4}-\d{2}-\d{2}$/.test(k) && Number.isFinite(Date.parse(k)) && d && Number.isFinite(d.sent) && Number.isFinite(d.bodies)) this.days[k] = { sent: d.sent, bodies: d.bodies };
        }
      }
    } catch {
      // None yet.
    }
    const first = Object.keys(this.days).sort()[0];
    this.since = first ? Date.parse(first) : this.now();
    this.prune();
  }

  /** One mutation sent: its arguments' size in bytes, and whether it was tool output. */
  record(bytes: number, isBodies: boolean): void {
    if (!(bytes > 0)) return;
    const key = dayKey(this.now());
    const d = (this.days[key] ??= { sent: 0, bodies: 0 });
    d.sent += bytes;
    if (isBodies) d.bodies += bytes;
    this.dirty = true;
    if (this.now() - this.savedAt >= SAVE_MS) this.save();
  }

  save(): void {
    // Days dropping out of the window are a change too.
    if (this.prune()) this.dirty = true;
    if (!this.dirty) return;
    const tmp = `${this.file}.${process.pid}.tmp`;
    try {
      fs.writeFileSync(tmp, JSON.stringify({ days: this.days }) + '\n', { mode: 0o600 });
      fs.renameSync(tmp, this.file);
      this.dirty = false;
      this.savedAt = this.now();
    } catch {
      fs.rmSync(tmp, { force: true });
    }
  }

  use(): ConvexUse {
    const from = dayKey(this.now() - (WINDOW_DAYS - 1) * DAY_MS);
    let sent = 0;
    let bodies = 0;
    for (const [k, d] of Object.entries(this.days)) {
      if (k < from) continue;
      sent += d.sent;
      bodies += d.bodies;
    }
    return estimate({ sent, bodies, since: new Date(this.since).toISOString(), days: Math.max(1, (this.now() - this.since) / DAY_MS) });
  }

  private prune(): boolean {
    const from = dayKey(this.now() - WINDOW_DAYS * DAY_MS);
    let gone = false;
    for (const k of Object.keys(this.days)) {
      if (k < from) {
        delete this.days[k];
        gone = true;
      }
    }
    return gone;
  }
}

// The estimate from 30 days' counts, scaled up to a month while there are
// fewer days than that, so a busy first week warns before the month is out.
export function estimate(c: { sent: number; bodies: number; since: string; days: number }): ConvexUse {
  const scale = WINDOW_DAYS / Math.min(WINDOW_DAYS, Math.max(1, c.days));
  const io = Math.round(c.sent * IO_PER_SENT_BYTE * scale);
  // Bodies are kept 30 days, so what's stored is a month's worth however long it's been.
  const storage = Math.round(c.bodies * STORAGE_PER_BODY_BYTE * scale);
  const out: ConvexUse = { sent: c.sent, bodies: c.bodies, io, storage, since: c.since };
  const ioOver = io >= FREE_IO_BYTES * WARN_AT;
  const storageOver = storage >= FREE_STORAGE_BYTES * WARN_AT;
  if (ioOver || storageOver) out.warning = warningText(io, storage, ioOver, storageOver);
  return out;
}

export function warningText(io: number, storage: number, ioOver: boolean, storageOver: boolean): string {
  const what = [ioOver ? `about ${mb(io)} of database I/O a month` : '', storageOver ? `about ${mb(storage)} of stored tool output` : ''].filter(Boolean).join(' and ');
  return (
    `Convex: this runner looks to be heading for ${what}, near the Free plan's limits (1 GB of I/O a month, 0.5 GB stored). ` +
    `It's an estimate, and the limits are your team's. On Free, writes can start failing once you're over. ` +
    `To use less, turn full sync off ("sync": { "full": false } in runner.json); or move to a paid plan in the Convex dashboard. ` +
    `On a paid plan already? Hide this with "sync": { "usageWarning": false }.`
  );
}

function mb(bytes: number): string {
  return bytes >= 1e9 ? `${(bytes / 1e9).toFixed(1)} GB` : `${Math.round(bytes / 1e6)} MB`;
}

function dayKey(ms: number): string {
  return new Date(ms).toISOString().slice(0, 10);
}

// ---- The question for installs from before full sync was the default.

/**
 * Whether this runner may ask, once, if full sync should go on. Only the main
 * Mac's runner asks (a runner key is a second machine's): with sync on, no
 * `sync.full` in runner.json either way, and not asked before. Full sync starts
 * off (Mike, 13 Nov: a one-Mac user gets the costs and none of the benefits),
 * so this is asked only once it's useful: see fullSyncReason. An install with
 * the key set, either way, is never asked or changed.
 */
export function shouldAskFullSync(o: { syncOn: boolean; key: string | undefined; full: boolean | undefined; asked: string | undefined }): boolean {
  if (!o.syncOn || !o.key || o.key.startsWith('tbr_')) return false;
  return o.full === undefined && !o.asked;
}

// What made full sync useful: a phone or browser paired to this backend, or
// another machine on it. Every benefit of full sync is reading a thread
// somewhere other than the machine that ran it, so these are the moments to ask.
export type FullSyncReason = 'phone' | 'browser' | 'machine';

/** Why it'd help now, from the backend's live phone and browser tokens and the other runners this one has heard of. */
export function fullSyncReason(o: { devices: { kind: string; expired?: boolean }[]; otherRunners: number }): FullSyncReason | undefined {
  const live = o.devices.filter((d) => !d.expired);
  if (live.some((d) => d.kind === 'phone')) return 'phone';
  if (live.some((d) => d.kind === 'browser')) return 'browser';
  return o.otherRunners > 0 ? 'machine' : undefined;
}

/** The question, naming what just made it useful. `machine` is "Mac" on a Mac. */
export function fullSyncQuestion(reason: FullSyncReason | undefined, machine: string): string {
  const when = machine === 'Mac' ? "even when the Mac's asleep" : `even when the ${machine}'s off`;
  const what = `this ${machine}'s tool output and diffs too, ${when}?`;
  if (reason === 'phone') return `You've paired your phone. Want it to show ${what}`;
  if (reason === 'browser') return `You've signed in on the website. Want it to show ${what}`;
  if (reason === 'machine') return `You've added another machine. Want it to show ${what}`;
  return `Want your phone, the website and your other machines to show ${what}`;
}

// The brief for the thread that asks. It goes to the install's own agent,
// which asks with its end-of-turn card and sets the answer with tb.
// The asking thread's title, so a runner that crashed before noting it can find
// it again rather than ask twice.
export const FULL_SYNC_ASK_TITLE = 'Show tool output on your other devices?';

// The card's two options. The runner sets the answer itself when Mike picks
// one on the card (fullSyncAnswer), not the asking thread's agent: an agent
// is only told the answer as text, and another thread could send it the
// same words (Daybreak 121, 13 Nov 2026).
export const FULL_SYNC_ON = 'Turn it on';
export const FULL_SYNC_OFF = 'Leave it off';

// What each option says it does, on the card. The pick counts only on the
// card as the runner worded it (fullSyncAnswer).
export const FULL_SYNC_ON_DOES = 'Tool output, input and diffs go up too, scrubbed, kept 30 days';
export const FULL_SYNC_OFF_DOES = 'Only messages and a line per tool, as now';

type CardAsk = { action: string; options?: string[]; optionNotes?: string[]; why?: string; where?: unknown; reply?: string; links?: unknown[]; run?: unknown };

/**
 * What Mike picked on the asking thread's open card, or undefined. Only a
 * message's `answers` count, which come from the app, the phone or the
 * website (an agent's are taken out, rpc-gate.ts), and only for the ask the
 * runner worded: its question as the runner noted it, the two options with
 * what they do, and nothing else on it. The agent writes the card, so
 * another thread could talk it into the same two options under another
 * question ("Turn on local logging?"), and his pick would have answered
 * that (Codex's review). Any other card sets nothing.
 */
export function fullSyncAnswer(answers: { action: string; answer: string }[] | undefined, card: CardAsk[] | undefined, question: string | undefined): boolean | undefined {
  // One ask on the card, so a second with the same words but other notes
  // can't take his pick (answers name an ask by its words; Codex's review).
  if (!question || card?.length !== 1) return undefined;
  const ask = card.find(
    (a) =>
      a.action === question &&
      a.options?.length === 2 &&
      a.options[0] === FULL_SYNC_ON &&
      a.options[1] === FULL_SYNC_OFF &&
      (a.optionNotes === undefined || (a.optionNotes.length === 2 && a.optionNotes[0] === FULL_SYNC_ON_DOES && a.optionNotes[1] === FULL_SYNC_OFF_DOES)) &&
      !a.why &&
      !a.where &&
      !a.reply &&
      !a.links?.length &&
      // A card's command from Run on the card (taken out 14 Nov 2026) can still be in an old record.
      !('run' in a),
  );
  const picked = ask && answers?.find((a) => a.action === ask.action)?.answer;
  return picked === FULL_SYNC_ON ? true : picked === FULL_SYNC_OFF ? false : undefined;
}

/** Runner's `answered`: Mike's pick on the asking thread's card sets full sync, once (settings.update forgets the thread). */
export function fullSyncAnswerer(o: { asked: () => { thread?: string; question?: string } | undefined; set: (on: boolean) => void }) {
  return (threadId: string, answers: { action: string; answer: string }[], card: CardAsk[] | undefined): void => {
    const asked = o.asked();
    if (!asked?.thread || threadId !== asked.thread) return;
    const on = fullSyncAnswer(answers, card, asked.question);
    if (on !== undefined) o.set(on);
  };
}

export function askBrief(tb: string, reason?: FullSyncReason, machine = 'Mac'): { title: string; why: string; text: string } {
  const question = fullSyncQuestion(reason, machine);
  return {
    title: FULL_SYNC_ASK_TITLE,
    why: 'asks once about full sync',
    text: [
      "You're starting this on the runner's behalf (docs/full-convex.md step 6). Ask the user one question. Don't do anything else.",
      '',
      `End your turn at once with end_turn: needs "you", one ask, the question "${question}", options "${FULL_SYNC_ON}" (does: "${FULL_SYNC_ON_DOES}") and "${FULL_SYNC_OFF}" (does: "${FULL_SYNC_OFF_DOES}"), suggested none, and nothing else on the ask (no why, where, reply or links). Word the question exactly so: the runner reads the pick only from that card.`,
      `Your reply, in plain words and short: with this on, the phone, the website and other machines can open a thread's tool output and diffs, not just its messages and a line per tool, even when this ${machine} isn't there to ask. They go to the user's own Convex deployment, kept 30 days, with secrets scrubbed out first, but that isn't a guarantee; screenshots and files that hold secrets stay on this ${machine}.`,
      '',
      `Picking an option on the card sets it: the runner does that itself, and you can't (don't run \`${tb} settings\`). When they've picked one, say in one line which they picked and that it takes effect when the runner next starts, then end_turn with needs "nothing" and archive true.`,
      `If they write something else, answer it briefly and ask again with the same card: only a pick on it counts. If they never answer, nothing changes: it stays off and this isn't asked again.`,
    ].join('\n'),
  };
}

/**
 * Asks at most once, whatever sets it off: the runner's start (a phone, a
 * browser or another machine already there), a phone or browser being paired
 * here, or another runner turning up. It notes `fullAsked` before it opens the
 * thread, so a crash in between asks never rather than twice, and only the
 * thread it opened is recorded as the one that may set the answer; a thread
 * that won't open takes the note back, for a later trigger. `busy` stops two
 * triggers in one process starting two threads.
 */
type FullSyncOfferDeps = {
  read(): { syncOn: boolean; key: string | undefined; full: boolean | undefined; asked: string | undefined };
  start(reason: FullSyncReason): string;
  // Before the thread opens (no id), then again with its id; `undefined` with
  // `clear` takes the note back.
  note(threadId: string | undefined, clear?: boolean): void;
  later(fn: () => void, ms: number): void;
  delayMs: number;
  // False while the runner is stopping or handing over: then it waits for a later trigger.
  allowed(): boolean;
  log?: (msg: string, meta: Record<string, unknown>) => void;
};

export class FullSyncOffer {
  private busy = false;
  private deps: FullSyncOfferDeps;
  constructor(deps: FullSyncOfferDeps) {
    this.deps = deps;
  }

  /** Whether it could still ask, so a caller can skip a lookup. */
  get open(): boolean {
    return !this.busy && this.eligible;
  }

  /** Whether it may ever ask: asked, answered or a runner key means no, for good. */
  get eligible(): boolean {
    return shouldAskFullSync(this.deps.read());
  }

  trigger(reason: FullSyncReason | undefined): void {
    if (!reason || !this.open) return;
    this.busy = true;
    this.deps.later(() => {
      try {
        // Answered, or asked by an earlier release, in the meantime: nothing.
        if (!shouldAskFullSync(this.deps.read()) || !this.deps.allowed()) return;
        this.deps.note(undefined);
        let threadId: string;
        try {
          threadId = this.deps.start(reason);
        } catch (e) {
          // A runner handing over refuses new threads: the next trigger, or the next start, asks.
          this.deps.note(undefined, true);
          this.deps.log?.('could not ask about full sync', { error: String(e) });
          return;
        }
        this.deps.note(threadId);
        this.deps.log?.('asked about full sync', { threadId, reason });
      } finally {
        this.busy = false;
      }
    }, this.deps.delayMs);
  }
}
