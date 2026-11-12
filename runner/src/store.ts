import type { HighlightState } from './highlights.ts';
import fs from 'node:fs';
import path from 'node:path';
import type { WaitingForRoom, Ask, AskAnswer, NeedsWho, DesignReview, ReplyButton, ReviewDraft, EffortLevel, MessageOrigin, PanelRequest, PermissionMode, ThreadShare, PersistedEvent, Provider, ArchivedBy, ArchiveVerdict, Subagent, ThreadProject, ThreadSchedule, ThreadStartedBy, ThreadStatus, ImageRef, LooseEnd, ContextUsage, LabReviewLedger, ReviewFinding } from './protocol.ts';
import { atomicWriteFileSync, readJsonSync } from './fsutil.ts';
import { paths } from './config.ts';
import type { WorktreeRecord } from './worktrees.ts';
import type { WakeWhen } from './wake-when.ts';

// Review by another lab's ledger as the runner keeps it: what goes on the
// card (LabReviewLedger), plus where it is. delivered: the latest round's
// findings went to the agent; answered: the round whose answers the
// agent's turn gave (and the card showed).
export type LabLedger = LabReviewLedger & { delivered?: number; answered?: number };
// A reviewer's verdict: its findings, none when nothing holds up.
export type ReviewVerdict = { findings: Pick<ReviewFinding, 'severity' | 'claim' | 'where' | 'case'>[] };

export type QueuedSend = { turnId: string; text: string; images: ImageRef[]; origin?: MessageOrigin; scheduleId?: string; reviewId?: string; reviewResponse?: ReviewDraft; answers?: AskAnswer[]; editOf?: number };

// What thread.json holds: the public summary fields plus a few internal ones.
export type PressNote = { id: string; line: string; told?: { seq: number; inFlight?: boolean } };

// A thread's press notes as read from disk, where another runner may have
// written them (a moved thread's thread.json is the old runner's word):
// only well-formed ones, and no more than Runner.notePress keeps (20 told,
// one per button untold). `presses: {}` from a moved thread stopped the
// runner at every start, as recover went through it (security review, 11
// Nov 2026, finding 96).
const MAX_PRESSES = 20 + 200;
const MAX_PRESS_LINE = 2000;
export function cleanPresses(raw: unknown): PressNote[] | undefined {
  if (!Array.isArray(raw)) return undefined;
  const out: PressNote[] = [];
  for (const p of raw.slice(-MAX_PRESSES)) {
    if (!p || typeof p !== 'object') continue;
    const { id, line, told } = p as Record<string, unknown>;
    if (typeof id !== 'string' || typeof line !== 'string') continue;
    const note: PressNote = { id: id.slice(0, 200), line: line.slice(0, MAX_PRESS_LINE) };
    if (told && typeof told === 'object') {
      const { seq, inFlight } = told as Record<string, unknown>;
      if (typeof seq === 'number' && Number.isFinite(seq)) note.told = { seq, ...(inFlight === true ? { inFlight: true } : {}) };
    }
    out.push(note);
  }
  return out.length ? out : undefined;
}

// What a thread.json read from disk may hold where the runner goes through
// a list or a field of an object, put in the shape the runner expects: a
// list that isn't one goes (or is empty, for the two every record has), as
// do its entries that aren't the kind it holds, and an object that isn't
// one goes. Another runner may have written it (a move), and recover,
// summary and the start loop walk these for every thread: `presses: {}`,
// then `queue: {}`, stopped the runner at every start (security review and
// Muse's review of the fix, 11 Nov 2026, finding 96). True if it changed.
const OBJECT_LISTS = ['waitingOn', 'reviews', 'buttons', 'subagents', 'projects', 'rewinds', 'queue'];
const STRING_LISTS = ['reviewDeliveries', 'allowRules', 'pendingApprovals', 'projectsLeft', 'projectsTold'];
const OBJECTS = ['needs', 'looseEnds', 'stoppedWork', 'stoppedMidTurn', 'highlights', 'worktree', 'handoff', 'moved', 'startedBy', 'labReview', 'labReviewOwed', 'reviewVerdict', 'finished'];
const NESTED: [string, string, 'object' | 'string' | 'number'][] = [
  ['needs', 'asks', 'object'],
  ['needs', 'ticked', 'number'],
  ['needs', 'open', 'string'],
  ['looseEnds', 'ends', 'object'],
  ['stoppedWork', 'what', 'string'],
  ['highlights', 'items', 'object'],
  ['highlights', 'edited', 'string'],
  ['highlights', 'removed', 'string'],
  ['labReview', 'findings', 'object'],
  ['reviewVerdict', 'findings', 'object'],
];
const isObject = (v: unknown): v is Record<string, unknown> => typeof v === 'object' && v !== null && !Array.isArray(v);
function listOf(v: unknown, kind: 'object' | 'string' | 'number'): unknown[] | undefined {
  if (!Array.isArray(v)) return undefined;
  return v.filter((x) => (kind === 'object' ? isObject(x) : typeof x === kind));
}
export function normaliseRecord(rec: Record<string, unknown>): boolean {
  const before = JSON.stringify(rec);
  for (const k of OBJECTS) if (rec[k] !== undefined && !isObject(rec[k])) delete rec[k];
  for (const [k, kind] of [...OBJECT_LISTS.map((k) => [k, 'object'] as const), ...STRING_LISTS.map((k) => [k, 'string'] as const)]) {
    if (rec[k] === undefined) continue;
    const list = listOf(rec[k], kind);
    if (list) rec[k] = list;
    else delete rec[k];
  }
  rec.queue ??= [];
  rec.pendingApprovals ??= [];
  for (const [o, k, kind] of NESTED) {
    const obj = rec[o];
    if (!isObject(obj) || obj[k] === undefined) continue;
    const list = listOf(obj[k], kind);
    if (list) obj[k] = list;
    else delete obj[k];
  }
  // looseEnds is drawn from its ends; one without them is no check. The
  // Highlights need both their lists, so without either they start again.
  if (isObject(rec.looseEnds) && !Array.isArray(rec.looseEnds.ends)) delete rec.looseEnds;
  // A lab review's ledger is read as its rounds and findings.
  if (isObject(rec.labReview) && !(typeof rec.labReview.rounds === 'number' && Array.isArray(rec.labReview.findings))) delete rec.labReview;
  if (isObject(rec.highlights) && !(Array.isArray(rec.highlights.items) && Array.isArray(rec.highlights.edited))) delete rec.highlights;
  // A wait is on a thread, by its id.
  if (Array.isArray(rec.waitingOn)) rec.waitingOn = rec.waitingOn.filter((w) => typeof w.threadId === 'string' && typeof (w.title ?? '') === 'string' && (w.done === undefined || (isObject(w.done) && typeof (w.done.reply ?? '') === 'string' && typeof (w.done.why ?? '') === 'string')));
  // What it said last, which a wait on it carries.
  if (isObject(rec.finished) && rec.finished.reply !== undefined && typeof rec.finished.reply !== 'string') delete rec.finished.reply;
  if (isObject(rec.reviewVerdict) && !Array.isArray(rec.reviewVerdict.findings)) delete rec.reviewVerdict;
  if (rec.presses !== undefined) {
    const presses = cleanPresses(rec.presses);
    if (presses) rec.presses = presses;
    else delete rec.presses;
  }
  if (rec.title !== undefined && typeof rec.title !== 'string') rec.title = String(rec.title);
  return JSON.stringify(rec) !== before;
}

export type ThreadRecord = {
  id: string;
  title: string;
  cwd: string;
  accountId: string;
  // The pool that picks the login (pools.ts). Absent: pinned to accountId,
  // never moved; every thread from before pools.
  poolId?: string;
  // Every login in the pool is out of room: the thread carries on by
  // itself at `until` (an ISO time). Kept here so the wait survives a
  // restart; start() re-arms its timer.
  waitingForRoom?: WaitingForRoom;
  // Mike turned off carrying on by itself once its login has room again
  // (the limit box by the composer). Absent: on, the default.
  carryOnAtReset?: false;
  // The turn that stopped at its login's limit, until the next turn
  // starts, so turning carryOnAtReset back on knows it's still stuck there.
  limitHitTurn?: string;
  // A turn failed because the API couldn't be reached (errorKind network):
  // the thread carries on by itself once a probe reaches it again
  // (Runner.onNetworkLost). `since` is when the line first went, `tries`
  // the carry-ons sent since. Kept here so the wait survives a restart.
  waitingForNetwork?: { since: string; tries: number };
  // The runner's own carry-on after that, so if it can't get through
  // either the thread waits again, counting on from the same outage.
  netCarryOn?: { turnId: string; since: string; tries: number };
  // The turn a restart (or a crash, or the Mac restarting) cut off, until
  // the runner that comes next carries it on (Runner.carryOnStopped).
  stoppedMidTurn?: { turnId: string; at: string };
  // That carry-on's turn id: if it's cut off too, it isn't carried on again.
  restartCarryOn?: string;
  // Background work a handover or a restart stopped, until the runner that
  // comes next tells the thread (Runner.tellStoppedWork). Kept here because
  // the stop and the telling happen in different runners.
  stoppedWork?: { what: string[]; at: string };
  // Times it has been woken for that since anyone last wrote to it.
  stoppedWorkWakes?: number;
  provider: Provider;
  model?: string;
  effort?: EffortLevel;
  modelInUse?: string; // full id the CLI ran the last turn on
  effortInUse?: EffortLevel; // the level the CLI reported for the last turn
  permissionMode: PermissionMode;
  // Its CLI runs in a sandbox (sandbox.ts) and must stay in one: set when it
  // starts in a sandbox folder or a sandboxed thread's agent starts it
  // there, and taken off only by a move Mike made or allowed. A thread with
  // it never starts outside a sandbox, whatever its folder has become
  // (Daybreak, 7 Nov 2026), and its agent counts as restricted
  // (threadbear-mcp.ts, "Raising access").
  confined?: true;
  // "Plan first": the mode approving the plan goes back to, and the plan
  // waiting for Mike (its plan.proposed itemId).
  modeBeforePlan?: PermissionMode;
  openPlan?: string;
  status: ThreadStatus;
  createdAt: string;
  updatedAt: string;
  lastActivityAt: string;
  snoozedUntil?: string;
  snoozedUntilRestart?: boolean; // snoozedUntil is UNTIL_RESTART; start() wakes it
  untilRelease?: string; // with snoozedUntilRestart: only a start on another release wakes it
  wakeWhen?: WakeWhen; // hibernated in Mike's words (wake-when.ts)
  // Hibernating until these threads finish (end_turn's waitFor,
  // runner/hibernation.ts): each counts once it has finished (with no new
  // work since), been archived or been deleted.
  // `until`: waits for something in particular, which the librarian
  // judges from its looks at that thread (met), not for any finish.
  // `review`: the thread is a review of this one's change (runner/lab-review.ts).
  waitingOn?: { threadId: string; title: string; until?: string; review?: true; quiet?: true; done?: { at: string; how: 'finished' | 'met' | 'archived' | 'deleted'; reply?: string; why?: string } }[];
  // Its last finished turn (nothing needed, nothing left running) and that
  // turn's reply, for threads waiting on it, including on a runner it
  // moves to (runner/hibernation.ts, reconcileWaits). Cleared by any new
  // work (a message, a turn its CLI opens), so no clock is compared.
  finished?: { at: string; reply?: string };
  // Review by another lab (runner/lab-review.ts): reviewOf is set on a
  // review thread (the thread whose change it checks), which never gets a
  // review of its own; labReviews counts the reviews since Mike last wrote.
  reviewOf?: string;
  labReviews?: number;
  // The ledger of the review under way (lab-review.ts, LabLedger): its
  // findings and the agent's answers, until Mike next writes.
  labReview?: LabLedger;
  // On a review thread: the verdict its end_turn gave (findings), for the
  // thread it reviewed.
  reviewVerdict?: ReviewVerdict;
  // A review a draining, restarting or stopping runner couldn't start (it
  // makes no threads), for the runner that adopts or loads this one to start.
  // `seq`: where that turn's events end, which its diff is read back from.
  labReviewOwed?: { turnId: string; seq?: number; reply?: string };
  snoozedAt?: string; // when it was last put away, for how long it slept; may be stale once awake
  woke?: { at: string; since?: string }; // woke by itself and not looked at yet (since = snoozedAt)
  archivedAt?: string;
  // Who archived it, and Mike's verdict from the done card. Set afresh by
  // every archive and only read while archivedAt is set, so the paths that
  // bring a thread back needn't clear them.
  archivedBy?: ArchivedBy;
  archiveVerdict?: ArchiveVerdict;
  // A schedule's message brought it out of the archive (Runner.sendTo), and
  // who had archived it: a run that ends with nothing for Mike puts it back.
  unarchivedBySchedule?: { by?: ArchivedBy };
  // Someone took it back out of the archive by hand (Runner.update): a
  // helper thread that would archive itself when it finishes stays in the
  // inbox instead (runner/quiet-inbox.ts).
  keptOut?: boolean;
  starredAt?: string; // Mike starred it: the Starred section, oldest star first
  preview: string;
  lastSeq: number;
  lastSeenSeq: number;
  nativeSessionId?: string;
  contextTokens?: number;
  contextWindow?: number; // the model's window, when the CLI reports it
  // Claude's last breakdown of the context (threads.contextUsage), for when
  // its CLI has gone: only for the native session it was asked of.
  contextUsage?: ContextUsage & { sessionId: string };
  costUsd?: number;
  reviews?: DesignReview[];
  reviewDeliveries?: string[]; // review IDs whose reply reached session.send
  buttons?: ReplyButton[]; // its reply buttons, oldest first (runner/buttons.ts)
  // Internal.
  // The version of the CLI that ran the thread's last session, so a new
  // session on another version can say so ("Codex 0.165.0 from this turn
  // on (was 0.164.0)") even after an idle release.
  cliVersion?: string;
  // Set when a same-account folder move on an adapter that moves folders
  // itself (Codex) is waiting for the next turn, which says so. The
  // session carries on; there's no handoff.
  moved?: { fromCwd: string };
  sessionUsed?: boolean; // nativeSessionId has a transcript, so the next start resumes it
  sessionCostTotal?: number; // the CLI's running total_cost_usd for this native session
  lastTurnCompletedSeq: number;
  forceUnread?: boolean; // set when a snooze ends
  // What the last completed turn said it needs from Mike, with the seq of
  // its turn.completed and which of its asks he has ticked.
  // open: what the agent said is still open (experiment looseEnds).
  needs?: { seq: number; needs: NeedsWho; asks?: Ask[]; ticked?: number[]; open?: string[] };
  // Set when the folder or account changes, or the record had to be
  // rebuilt; the next turn starts a new native session with a note.
  // lost: the session to resume couldn't be found (never saved; only set
  // while the fresh session's first message is put together).
  // quickAsk: the thread began as a quick ask from the phone (Runner.quickAsk),
  // answered by a fast model rather than a session of its own.
  handoff?: { fromCwd: string; fromAccountId: string; rebuilt?: boolean; lost?: boolean; quickAsk?: boolean };
  copied?: true; // a copy of a real thread on a dev runner's --clone-real data: never runs (real-copy.ts)
  sample?: true; // one of the tour's sample threads (Runner.samples): made up, never sent to
  allowRules?: string[]; // "Always allow" rules, in the CLI's syntax
  queue: QueuedSend[];
  pendingApprovals: string[];
  turnId?: string; // turn in progress when the record was saved
  titleSet?: boolean;
  titleByMike?: boolean; // named by Mike, so agents leave it alone (Runner.update)
  subagents?: SubagentRecord[]; // newest last, at most SUBAGENTS_KEPT
  // The projects the thread is in (projects.ts), oldest first; titles come
  // from the project list when the summary is made.
  projects?: ThreadProjectRecord[];
  // Projects Mike took the thread out of: the librarian never puts it back.
  projectsLeft?: string[];
  // The projects the native session has been told about (the brief, or a
  // line when it joined one), so it hears about each once.
  projectsTold?: string[];
  // The hash of Mike's standing instructions the native session was last
  // told, so it hears each change once; 'cleared' or 'rewound' (instructions.ts).
  // Absent while it has never held any.
  instructionsTold?: string;
  // Its reply buttons Mike has pressed, for its agent to hear of before his
  // next message (Runner.takePressNews), so it doesn't send the same job
  // again itself. Each keeps its own line, as the button can go (MAX_BUTTONS,
  // a move); `told` once a message carried it, `inFlight` until that turn
  // completes, and `seq` so an edit back past that message tells it again.
  presses?: PressNote[];
  // Highlights (highlights.ts): what's been found and pinned, and the
  // event it had read up to. Brought up to date when a client asks.
  highlights?: HighlightState;
  highlightsAt?: string; // a pin, a removal or a new brief
  brief?: { text: string; at: string }; // the librarian's line on where the thread is up to
  // The thread's own git worktree (worktrees.ts), once it's made.
  worktree?: WorktreeRecord;
  // The done card's loose ends (runner/loose-ends.ts), for the card at seq.
  looseEnds?: { seq: number; ends: LooseEnd[]; at: string };
  // Made in a repo that asks for worktrees and not told otherwise: the
  // first turn makes one (and runs its setup) before the CLI starts.
  worktreePending?: boolean;
  // The schedule that started the thread, with its title then (the
  // schedule may be renamed or deleted later).
  schedule?: ThreadSchedule;
  // Who started it, when that wasn't Mike. Its why is the starter's, until
  // the thread's own end_turn says how it came out.
  startedBy?: ThreadStartedBy;
  // Mike's last message, one line (askedLine in runner.ts).
  asked?: string;
  // The librarian's search words: what someone might type to find it that
  // its title doesn't say ("dentist" for a wisdom tooth appointment).
  // "" once a look or the keyword sweep found none, so the sweep moves on.
  keywords?: string;
  // show_panel's request, until the app shows it or Mike sends a message.
  panelRequest?: PanelRequest;
  // The seq ranges edits of earlier messages cut out of the conversation
  // (docs/protocol.md, "Editing a message"), oldest first. Everything that
  // reads the thread for the agent or the librarian leaves them out.
  rewinds?: { from: number; to: number }[];
  // Claude: the transcript entry the next start resumes at, after an edit
  // cut the conversation back. Used once.
  resumeAt?: string;
  // Sharing on the website (Runner.share): who else can read it.
  share?: ThreadShare;
};

// Whether an edit cut the event at `seq` out of the conversation.
export function cutOut(rec: Pick<ThreadRecord, 'rewinds'>, seq: number): boolean {
  return rec.rewinds?.some((r) => seq >= r.from && seq <= r.to) ?? false;
}

export type ThreadProjectRecord = Omit<ThreadProject, 'title'>;

// A sub-agent as the summary shows it, plus where its transcript is: the
// thread can change folder, account or session after it ran.
export type SubagentRecord = Subagent & { sessionId?: string; cwd?: string; accountId?: string };

// How much of the end of an events.ndjson opening one reads (EventLog).
const TAIL_BYTES = 64 * 1024;

// Append-only events.ndjson with a byte-offset index, so a page of a
// 4,000-event thread is one read of the bytes it needs. seq N is line N.
//
// Opening one reads only the end of the file: the last line's seq is the
// count, which is all a start needs for most threads. The index is built
// on the first read(). Reading every log in full at start (456 MB over
// 1,273 threads) took 1.5 to 2 s of each runner restart (12 Nov).
export class EventLog {
  readonly file: string;
  // Built on the first read(); appends keep it up to date once it is.
  private offsets: number[] | undefined;
  private count = 0;
  private size = 0;

  constructor(file: string) {
    this.file = file;
    if (!this.openTail()) this.loadIndex();
  }

  // The count and size from the last whole line, cutting off half a line a
  // crash mid-append left. False when the tail can't say (a last line
  // longer than the bit read, or one that isn't an event), and the whole
  // file is read instead.
  private openTail(): boolean {
    let fd: number;
    try {
      fd = fs.openSync(this.file, 'r');
    } catch {
      this.offsets = [];
      this.count = 0;
      this.size = 0;
      return true;
    }
    try {
      const total = fs.fstatSync(fd).size;
      if (total === 0) {
        this.offsets = [];
        this.count = 0;
        this.size = 0;
        return true;
      }
      const len = Math.min(total, TAIL_BYTES);
      const at = total - len;
      const buf = Buffer.alloc(len);
      fs.readSync(fd, buf, 0, len, at);
      const lastNl = buf.lastIndexOf(0x0a);
      if (lastNl < 0) return false;
      const prevNl = lastNl === 0 ? -1 : buf.lastIndexOf(0x0a, lastNl - 1);
      // The last whole line starts after prevNl, or at the file's start.
      if (prevNl < 0 && at > 0) return false;
      const seq = (JSON.parse(buf.subarray(prevNl + 1, lastNl).toString('utf8')) as { seq?: unknown }).seq;
      if (typeof seq !== 'number' || !Number.isInteger(seq) || seq < 1) return false;
      const whole = at + lastNl + 1;
      // A crash mid-append left half a line. Drop it so seq stays gapless.
      if (whole < total) fs.truncateSync(this.file, whole);
      this.count = seq;
      this.size = whole;
      return true;
    } catch {
      return false;
    } finally {
      fs.closeSync(fd);
    }
  }

  private loadIndex(): void {
    let buf: Buffer;
    try {
      buf = fs.readFileSync(this.file);
    } catch {
      this.offsets = [];
      this.count = 0;
      this.size = 0;
      return;
    }
    const offsets: number[] = [];
    let start = 0;
    for (let i = 0; i < buf.length; i++) {
      if (buf[i] === 0x0a) {
        offsets.push(start);
        start = i + 1;
      }
    }
    if (start < buf.length) {
      // A crash mid-append left half a line. Drop it so seq stays gapless.
      fs.truncateSync(this.file, start);
    }
    this.offsets = offsets;
    this.count = offsets.length;
    this.size = start;
  }

  private index(): number[] {
    if (!this.offsets) this.loadIndex();
    return this.offsets!;
  }

  get lastSeq(): number {
    return this.count;
  }

  append(event: PersistedEvent): void {
    if (event.seq !== this.count + 1) {
      throw new Error(`event seq ${event.seq} out of order (last ${this.count})`);
    }
    const line = Buffer.from(JSON.stringify(event) + '\n', 'utf8');
    fs.mkdirSync(path.dirname(this.file), { recursive: true });
    try {
      fs.appendFileSync(this.file, line);
    } catch (e) {
      // A write that failed part way (disk full) would leave half a line
      // for the next append to run into; cut it off.
      try {
        if (fs.statSync(this.file).size > this.size) fs.truncateSync(this.file, this.size);
      } catch {
        // can't fix it now; the next open drops a half line
      }
      throw e;
    }
    this.offsets?.push(this.size);
    this.count++;
    this.size += line.length;
  }

  // Events with fromSeq <= seq <= toSeq.
  read(fromSeq: number, toSeq: number): PersistedEvent[] {
    // The index first: building it can correct the count, in a log whose
    // last seq isn't its line count (only a damaged one: append keeps them
    // equal, and all 1,273 of Mike's agreed on 12 Nov).
    const offsets = this.index();
    const from = Math.max(1, fromSeq);
    const to = Math.min(this.count, toSeq);
    if (to < from) return [];
    const startByte = offsets[from - 1];
    const endByte = to < offsets.length ? offsets[to] : this.size;
    const buf = Buffer.alloc(endByte - startByte);
    const fd = fs.openSync(this.file, 'r');
    try {
      fs.readSync(fd, buf, 0, buf.length, startByte);
    } finally {
      fs.closeSync(fd);
    }
    const out: PersistedEvent[] = [];
    for (const line of buf.toString('utf8').split('\n')) {
      if (!line) continue;
      try {
        out.push(JSON.parse(line) as PersistedEvent);
      } catch {
        // Skip a corrupt line rather than failing the whole page.
      }
    }
    return out;
  }
}

export class ThreadStore {
  readonly home: string;
  private logs = new Map<string, EventLog>();

  constructor(home: string) {
    this.home = home;
  }

  threadDir(id: string): string {
    return paths.thread(this.home, id);
  }

  mediaDir(id: string): string {
    return paths.media(this.home, id);
  }

  blobsDir(id: string): string {
    return paths.blobs(this.home, id);
  }

  // The ids of every thread folder on disk, this runner's or not.
  ids(): string[] {
    try {
      return fs.readdirSync(paths.threads(this.home)).filter((n) => !n.startsWith('.'));
    } catch {
      return [];
    }
  }

  // Every thread on disk. `broken` lists folders that have an event log but
  // no readable thread.json, so the runner can rebuild them rather than
  // have the thread vanish.
  loadAll(): { records: ThreadRecord[]; broken: string[] } {
    const dir = paths.threads(this.home);
    let names: string[] = [];
    try {
      names = fs.readdirSync(dir);
    } catch {
      return { records: [], broken: [] };
    }
    const records: ThreadRecord[] = [];
    const broken: string[] = [];
    for (const name of names) {
      if (name.startsWith('.')) continue;
      const rec = this.load(name);
      if (!rec) {
        if (fs.existsSync(path.join(dir, name, 'events.ndjson'))) broken.push(name);
        continue;
      }
      records.push(rec);
    }
    return { records, broken };
  }

  // One thread's record, or undefined when thread.json is missing or isn't
  // that thread's.
  load(id: string): ThreadRecord | undefined {
    const rec = readJsonSync<ThreadRecord>(path.join(paths.threads(this.home), id, 'thread.json'));
    if (!rec || !rec.id || rec.id !== id) return undefined;
    rec.provider ??= 'claude'; // records from before Codex
    rec.queue ??= [];
    rec.pendingApprovals ??= [];
    rec.lastTurnCompletedSeq ??= 0;
    // A folder the runner can't use isn't a thread it can load: rebuilt.
    if (typeof rec.cwd !== 'string') return undefined;
    normaliseRecord(rec as unknown as Record<string, unknown>);
    return rec;
  }

  // Drops the cached event log, so the next log() reads the file again:
  // another runner (a draining one, docs/protocol.md "Handovers") may have
  // written to it since.
  forget(id: string): void {
    this.logs.delete(id);
  }

  // Keeps an unreadable thread.json for a look later, out of the way.
  setAside(id: string): void {
    const file = path.join(this.threadDir(id), 'thread.json');
    try {
      if (fs.existsSync(file)) fs.renameSync(file, `${file}.unreadable-${Date.now()}`);
    } catch {
      // nothing to keep
    }
  }

  create(rec: ThreadRecord): void {
    fs.mkdirSync(this.mediaDir(rec.id), { recursive: true });
    fs.mkdirSync(this.blobsDir(rec.id), { recursive: true });
    this.save(rec);
  }

  save(rec: ThreadRecord): void {
    atomicWriteFileSync(path.join(this.threadDir(rec.id), 'thread.json'), JSON.stringify(rec, null, 1) + '\n');
  }

  // save(), unless thread.json already holds exactly this. A handover lets
  // go of every idle thread at once (letGo), nearly all of them unchanged,
  // and an fsync each for ~1,250 threads held the new runner back 5.5 s
  // (12 Nov). Reading the file back is a few milliseconds for all of them.
  saveIfChanged(rec: ThreadRecord): boolean {
    const file = path.join(this.threadDir(rec.id), 'thread.json');
    const text = JSON.stringify(rec, null, 1) + '\n';
    try {
      if (fs.readFileSync(file, 'utf8') === text) return false;
    } catch {
      // missing or unreadable: write it
    }
    atomicWriteFileSync(file, text);
    return true;
  }

  // Moves a thread's folder to trash/<id>-<time> so it's out of the list but
  // can still be put back by hand.
  trash(id: string, now: number): void {
    const dir = this.threadDir(id);
    if (!fs.existsSync(dir)) return;
    const bin = path.join(this.home, 'trash');
    fs.mkdirSync(bin, { recursive: true });
    fs.renameSync(dir, path.join(bin, `${id}-${now}`));
    this.logs.delete(id);
  }

  log(id: string): EventLog {
    let log = this.logs.get(id);
    if (!log) {
      log = new EventLog(path.join(this.threadDir(id), 'events.ndjson'));
      this.logs.set(id, log);
    }
    return log;
  }
}
