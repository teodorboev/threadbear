// Someone's own changes to the old Swift Mac app, for rebuilding in the
// GPUI one (docs/protocol.md, "Your changes to the old app").
//
// Other people run forks of Threadbear, and some changed the Swift app
// (app/, dock/) before it froze. GPUI is different code, so those changes
// can't be copied across, only rebuilt in desktop/. Mike picked two ways in
// (13 Nov 2026, the GPUI port thread df5c7153):
//
// - The first time the GPUI app says hello to this runner, it looks for
//   them, and if there are any starts one thread in the checkout that says
//   in plain words what they changed and offers to rebuild them. It changes
//   nothing until they say yes, so it's not an experiment: like bring-in.ts
//   it only offers, and only once.
// - The Updates panel lists what's left ("Not in the new app yet"), each
//   with Port this, which starts a thread for that one change, and Dismiss.
//   Nothing there is automatic.
//
// Theirs means: a commit on main that touches app/ or dock/ (outside what
// the GPUI and iPhone apps still use from there), that isn't a merge, isn't
// in upstream (Mike's verified releases or upstream's main, whichever this
// copy has), and isn't the same patch as one upstream has (a cherry-pick of
// Mike's). In Mike's own copy (origin is mikecann/threadbear) it's off.
//
// Done means a commit on main has a `Ports: <commit>` trailer naming it
// (the porting thread is told to add one), or they dismissed it here.
import { execFile } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import type { Clock, TimerHandle } from './clock.ts';
import type { Logger } from './log.ts';
import { untrustedGitEnv } from './safe-git.ts';
import { bareRepoArgs, repoKey } from './upstream.ts';

const MIKES_REPO = 'github.com/mikecann/threadbear';
// What of app/ the GPUI app still uses, so a change there isn't one to
// rebuild: the same as scripts/swift-freeze.sh --gpui-shared, which
// swift-changes.test.ts checks.
export const GPUI_SHARED = /^app\/Tools\/mock-runner\/|^app\/Tools\/(droptarget|post-gesture|realdrag)\.swift$|^app\/Tests\/Fixtures\/|^app\/Sources\/Threadbear\/Resources\/(Art|Rich)\//;
const MAX_CHANGES = 100;
const MAX_FILES = 12;
const GIT_TIMEOUT_MS = 60_000;
// Looks again at most this often from hellos, and every so often anyway, so
// a port landed (or a commit made by hand) moves off the list.
const RESCAN_MS = 60_000;
const EVERY_MS = 60 * 60_000;
const FIRST_LOOK_DELAY_MS = 2 * 60_000;

export type SwiftChange = {
  commit: string;
  short: string;
  subject: string;
  // The committer date, ISO.
  date: string;
  // Its files in app/ and dock/, at most MAX_FILES, and how many more.
  files: string[];
  moreFiles?: number;
  // An open thread rebuilding it (Port this, or the first look's thread).
  threadId?: string;
};

export type SwiftChangesStatus = {
  // off: Mike's own copy, or nothing to compare with; ready: looked.
  state: 'off' | 'ready' | 'failed';
  reason?: string;
  checkout?: string;
  changes: SwiftChange[];
};

type State = {
  // The first look after the GPUI app said hello, done once.
  firstLook?: { at: string; threadId?: string; commits?: string[] };
  dismissed?: string[];
  // Port this: commit -> the thread rebuilding it.
  porting?: Record<string, string>;
};

export type Git = (cwd: string, args: string[]) => Promise<string>;

export const git: Git = (cwd, args) =>
  new Promise((resolve, reject) => {
    execFile('git', ['-C', cwd, ...bareRepoArgs(cwd), ...args], { env: { ...untrustedGitEnv(), GIT_TERMINAL_PROMPT: '0', GIT_OPTIONAL_LOCKS: '0' }, timeout: GIT_TIMEOUT_MS, maxBuffer: 32 * 1024 * 1024 }, (err, stdout, stderr) => {
      if (err) reject(new Error(`git ${args.slice(0, 2).join(' ')} failed: ${String(stderr).trim() || err.message}`));
      else resolve(stdout);
    });
  });

const PATHS = ['--', 'app/', 'dock/'];

/** Their commits to the Swift app, newest first, before anything's ported or dismissed. */
export async function scanSwiftChanges(checkout: string | undefined, run: Git = git): Promise<SwiftChangesStatus & { ported: Set<string> }> {
  const ported = new Set<string>();
  if (!checkout) return { state: 'off', reason: 'no checkout to look in', changes: [], ported };
  const origin = await run(checkout, ['remote', 'get-url', 'origin']).catch(() => '');
  if (repoKey(origin) === MIKES_REPO) return { state: 'off', reason: 'this is the upstream copy itself', checkout, changes: [], ported };
  // What upstream has: Mike's verified releases (upstream.ts) and upstream's
  // main, when this copy has fetched it.
  const refs = (await run(checkout, ['for-each-ref', '--format=%(refname)', 'refs/threadbear/upstream/', 'refs/remotes/upstream/main'])).split('\n').filter(Boolean);
  if (!refs.length) return { state: 'off', reason: 'nothing of upstream\'s to compare with yet', checkout, changes: [], ported };
  const main = (await run(checkout, ['rev-parse', '--verify', '-q', 'refs/heads/main']).catch(() => '')).trim() ? 'refs/heads/main' : 'HEAD';
  const not = refs.map((r) => `^${r}`);
  const out = await run(checkout, ['log', '--no-merges', '--name-only', '-z', '--format=%x1e%H%x1f%h%x1f%cI%x1f%s', main, ...not, ...PATHS]);
  const found: SwiftChange[] = [];
  for (const rec of out.split('\x1e').slice(1)) {
    const [head, ...rest] = rec.split('\0');
    const [commit, short, date, subject] = head.split('\x1f');
    // -z: the format line, then the names separated by NULs (the first
    // one after a newline).
    const names = [head.split('\n').slice(1).join('\n'), ...rest].map((n) => n.replace(/^\n/, '')).filter(Boolean);
    const files = names.filter((f) => /^(app|dock)\//.test(f) && !GPUI_SHARED.test(f));
    if (!commit || !files.length) continue;
    found.push({ commit, short, subject: subject.split('\n')[0], date: date.trim(), files: files.slice(0, MAX_FILES), ...(files.length > MAX_FILES ? { moreFiles: files.length - MAX_FILES } : {}) });
  }
  // The same change as one upstream has: a cherry-pick of Mike's, or a fix
  // they sent him that he took (a rebase merge). Both keep the author, the
  // author's date and the subject, which is cheap to compare, where patch
  // ids would mean diffing every Swift commit of Mike's since. Only
  // upstream's commits their oldest one doesn't have can be one.
  let changes = found;
  if (found.length) {
    const who = '--format=%H%x1f%ae%x1f%at%x1f%s';
    const key = (l: string) => l.split('\x1f').slice(1).join('\x1f');
    const upstreams = new Set((await run(checkout, ['log', '--no-merges', who, ...refs, `^${found[found.length - 1].commit}`, ...PATHS])).split('\n').filter(Boolean).map(key));
    const same = new Set((await run(checkout, ['log', '--no-walk=unsorted', who, ...found.map((c) => c.commit)])).split('\n').filter((l) => l && upstreams.has(key(l))).map((l) => l.split('\x1f')[0]));
    changes = found.filter((c) => !same.has(c.commit));
  }
  // Ports: trailers on their own commits (a port is never upstream's).
  const trailers = await run(checkout, ['log', '--format=%(trailers:key=Ports,valueonly,separator=%x20)', main, ...not]);
  for (const id of trailers.match(/\b[0-9a-f]{7,40}\b/g) ?? []) ported.add(id);
  return { state: 'ready', checkout, changes: changes.slice(0, MAX_CHANGES), ported };
}

export function isPorted(commit: string, ported: Set<string>): boolean {
  for (const id of ported) if (commit.startsWith(id)) return true;
  return false;
}

export type SwiftChangesDeps = {
  home: string;
  checkout: string | undefined;
  clock: Clock;
  log: Logger;
  start: (r: { text: string; title: string; why: string }) => string;
  open: (threadId: string) => boolean;
  onChange: () => void;
  git?: Git;
};

export class SwiftChanges {
  private readonly deps: SwiftChangesDeps;
  private st: SwiftChangesStatus = { state: 'off', changes: [] };
  private scanning: Promise<SwiftChangesStatus> | undefined;
  private lastScan = 0;
  private timer: TimerHandle | undefined;
  private stopped = false;

  constructor(deps: SwiftChangesDeps) {
    this.deps = deps;
  }

  private file(): string {
    return path.join(this.deps.home, 'runner', 'swift-changes.json');
  }

  private read(): State {
    try {
      const s = JSON.parse(fs.readFileSync(this.file(), 'utf8')) as State;
      return s && typeof s === 'object' ? s : {};
    } catch {
      return {};
    }
  }

  private write(s: State): void {
    fs.mkdirSync(path.dirname(this.file()), { recursive: true });
    fs.writeFileSync(this.file(), JSON.stringify(s, null, 2) + '\n');
  }

  status(): SwiftChangesStatus {
    return this.st;
  }

  /** Looks a while after the runner starts, then every hour. */
  start(): void {
    const tick = (ms: number) => {
      this.timer = this.deps.clock.setTimeout(() => {
        if (this.stopped) return;
        void this.scan().finally(() => tick(EVERY_MS));
      }, ms);
    };
    tick(FIRST_LOOK_DELAY_MS);
  }

  stop(): void {
    this.stopped = true;
    this.timer?.cancel();
  }

  /** Looks again, one look at a time. */
  scan(): Promise<SwiftChangesStatus> {
    if (this.scanning) return this.scanning;
    this.scanning = (async () => {
      try {
        const found = await scanSwiftChanges(this.deps.checkout, this.deps.git);
        this.lastScan = this.deps.clock.now();
        this.st = this.dress(found);
      } catch (e) {
        this.deps.log.warn('swift changes: look failed', { error: e instanceof Error ? e.message : String(e) });
        this.st = { state: 'failed', reason: e instanceof Error ? e.message.slice(0, 300) : String(e), checkout: this.deps.checkout, changes: this.st.changes };
      } finally {
        this.scanning = undefined;
      }
      this.deps.onChange();
      return this.st;
    })();
    return this.scanning;
  }

  // Ported and dismissed ones left out; an open thread on each named.
  private dress(found: SwiftChangesStatus & { ported: Set<string> }): SwiftChangesStatus {
    const s = this.read();
    const dismissed = new Set(s.dismissed ?? []);
    const firstThread = s.firstLook?.threadId && this.deps.open(s.firstLook.threadId) ? s.firstLook.threadId : undefined;
    const firstCommits = new Set(s.firstLook?.commits ?? []);
    const changes = found.changes
      .filter((c) => !dismissed.has(c.commit) && !isPorted(c.commit, found.ported))
      .map((c) => {
        const own = s.porting?.[c.commit];
        const threadId = own && this.deps.open(own) ? own : firstThread && firstCommits.has(c.commit) ? firstThread : undefined;
        return threadId ? { ...c, threadId } : c;
      });
    const { ported: _, ...rest } = found;
    return { ...rest, changes };
  }

  /**
   * The GPUI app said hello. The first time, look and offer a thread when
   * there's anything; after that, look again (at most once a minute).
   */
  async desktopHello(): Promise<'off' | 'nothing' | 'started' | 'again' | 'failed'> {
    const s = this.read();
    if (s.firstLook) {
      if (this.deps.clock.now() - this.lastScan >= RESCAN_MS) await this.scan();
      return 'again';
    }
    const st = await this.scan();
    if (st.state === 'failed') return 'failed';
    const now = new Date(this.deps.clock.now()).toISOString();
    // Read again: a dismiss may have landed during the look.
    const s2 = this.read();
    if (s2.firstLook) return 'again';
    if (st.state === 'off' || !st.changes.length) {
      this.write({ ...s2, firstLook: { at: now } });
      return st.state === 'off' ? 'off' : 'nothing';
    }
    const threadId = this.deps.start({ text: firstLookMessage(st.changes), title: 'Bring your changes to the new app', why: 'your changes to the old app' });
    this.write({ ...s2, firstLook: { at: now, threadId, commits: st.changes.map((c) => c.commit) } });
    this.deps.log.info('swift changes: started the first look thread', { threadId, changes: st.changes.length });
    this.st = this.dress({ ...st, ported: new Set() });
    this.deps.onChange();
    return 'started';
  }

  private find(commit: string): SwiftChange {
    const c = this.st.changes.find((x) => x.commit === commit || (commit.length >= 7 && x.commit.startsWith(commit)));
    if (!c) throw new Error("That change isn't on the list (it may be ported already).");
    return c;
  }

  /** Port this: a thread for that one change, or the one already on it. */
  async port(commit: string): Promise<{ threadId: string; swiftChanges: SwiftChangesStatus }> {
    await this.scan();
    const c = this.find(commit);
    if (c.threadId) return { threadId: c.threadId, swiftChanges: this.st };
    const threadId = this.deps.start({ text: portMessage(c), title: `Rebuild "${c.subject.slice(0, 60)}" in the new app`, why: 'your change to the old app' });
    const s = this.read();
    this.write({ ...s, porting: { ...(s.porting ?? {}), [c.commit]: threadId } });
    this.st = { ...this.st, changes: this.st.changes.map((x) => (x.commit === c.commit ? { ...x, threadId } : x)) };
    this.deps.onChange();
    return { threadId, swiftChanges: this.st };
  }

  /** Dismiss (or, with undo, bring back) one change. */
  async dismiss(commit: string, undo = false): Promise<{ swiftChanges: SwiftChangesStatus }> {
    const s = this.read();
    const full = undo ? (s.dismissed ?? []).find((d) => d === commit || (commit.length >= 7 && d.startsWith(commit))) : this.find(commit).commit;
    if (!full) throw new Error("That change wasn't dismissed.");
    const dismissed = new Set(s.dismissed ?? []);
    if (undo) dismissed.delete(full);
    else dismissed.add(full);
    this.write({ ...s, dismissed: [...dismissed] });
    return { swiftChanges: await this.scan() };
  }
}

function day(iso: string): string {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return '';
  const months = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec'];
  return `${d.getUTCDate()} ${months[d.getUTCMonth()]} ${d.getUTCFullYear()}`;
}

function line(c: SwiftChange): string {
  const files = c.files.join(', ') + (c.moreFiles ? ` and ${c.moreFiles} more` : '');
  return `- ${c.commit} (${day(c.date)}) "${c.subject}": ${files}`;
}

const HOW = `For each change: read the Swift commit (git show), rebuild what it does in the GPUI app in desktop/ in this thread's worktree (desktop/README.md and docs/gpui-port.md say how it's laid out), and add a spec in features/ for what it does (features/README.md). End each commit's message with a \`Ports: <the Swift commit's full id>\` trailer: that's how the new app's Updates panel knows it's been rebuilt. Show me before and after for each, a shot of the new app without it and with it (desktop/scripts/dev-app.sh), then land as usual. This thread runs on this install's default mode, so a step that mode asks about waits for my yes on a card.`;

export function firstLookMessage(changes: SwiftChange[]): string {
  const n = changes.length;
  return `I've just started using the new Threadbear app (the GPUI one in desktop/). This copy has ${n} change${n === 1 ? '' : 's'} of my own to the old Swift Mac app (app/ and dock/) that upstream doesn't have:

${changes.map(line).join('\n')}

The new app is different code, so these can't be copied across, only rebuilt there. First, without changing anything: read each one and tell me in plain words what it changed, starting "You changed the old app in ${n} place${n === 1 ? '' : 's'}:", one short line each about what I'd see or do differently, and whether the new app already does it. Then ask whether to rebuild them in the new app, all of them or which ones. Change nothing until I say yes.

${HOW}

Anything I say no to stays in the new app's Updates panel under "Not in the new app yet", where I can port it later or dismiss it.`;
}

export function portMessage(c: SwiftChange): string {
  return `Rebuild one of my own changes to the old Swift Mac app (app/, dock/) in the new GPUI app (desktop/). The new app is different code, so it can't be copied, only rebuilt:

${line(c)}

First tell me in plain words what it changed and how you'd do it in the new app (or that the new app already does it), and ask before changing anything.

${HOW}`;
}
