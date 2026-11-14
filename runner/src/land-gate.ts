// `tb land` holding a landing while a GitHub workflow it would set off is red
// on main (docs/thread-worktrees.md, "Red on GitHub").
//
// The Desktop app workflow failed on every push of main for two days (10 to 12
// Nov) while threads went on landing GPUI changes, each adding to what was
// red: the Windows build stopped linking, and specs failed only on GitHub's
// Macs (their bash) and Linux (its words for Finder and ⌘Q). None of it showed
// in tb land's quick check, and couldn't have: those specs pass on Tinker
// Desk, and running them all takes 15 to 30 minutes a system. So a workflow
// that says so in a comment of its own holds the landings it would run for
// while its last run on main failed, and the red gets fixed before more goes
// on top of it.
//
// A workflow opts in with a line of its own anywhere in its file:
//
//   # tb land: hold while red
//
// Its `on: push: paths:` say which landings it holds (all of them, with no
// paths). Its own runs are asked of GitHub with gh. When gh isn't there, or
// can't say, the landing goes ahead, saying so: being offline shouldn't stop
// work. `tb land --despite-red` lands anyway, for the landing that's the fix.
import { execFile, execFileSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';

export const HOLD_MARK = /^\s*#\s*tb land:\s*hold while red\s*$/m;

export type HeldWorkflow = {
  file: string; // its file name, as gh's --workflow takes it
  name: string; // its `name:`, or the file name
  paths?: string[]; // its push paths; undefined when every push runs it
};

// The workflows in `root` that hold landings while red.
export function holdingWorkflows(root: string): HeldWorkflow[] {
  const dir = path.join(root, '.github', 'workflows');
  let names: string[];
  try {
    names = fs.readdirSync(dir).filter((n) => /\.ya?ml$/.test(n)).sort();
  } catch {
    return [];
  }
  const out: HeldWorkflow[] = [];
  for (const file of names) {
    let text: string;
    try {
      text = fs.readFileSync(path.join(dir, file), 'utf8');
    } catch {
      continue;
    }
    if (!HOLD_MARK.test(text)) continue;
    const name = /^name:\s*(.+?)\s*$/m.exec(text)?.[1]?.replace(/^(['"])(.*)\1$/, '$2') ?? file;
    out.push({ file, name, paths: pushPaths(text) });
  }
  return out;
}

const unquote = (s: string): string => s.trim().replace(/^(['"])(.*)\1$/, '$2');

// The `paths:` under `on:` / `push:`, as a list (block or `[a, b]`), or
// undefined when there are none, so every push runs it. Only as much YAML
// as a workflow's trigger is written in: keys by their indent.
export function pushPaths(yaml: string): string[] | undefined {
  const lines = yaml.split('\n');
  const indent = (l: string) => l.length - l.trimStart().length;
  const key = (l: string) => /^\s*(['"]?)([\w-]+)\1\s*:\s*(.*)$/.exec(l.replace(/\s+#.*$/, ''));
  const blockUnder = (from: number, to: number, name: string): number | undefined => {
    let level: number | undefined;
    for (let i = from; i < to; i++) {
      const l = lines[i];
      if (!l.trim() || l.trimStart().startsWith('#')) continue;
      level ??= indent(l);
      if (indent(l) < level) return undefined;
      if (indent(l) === level && key(l)?.[2] === name) return i;
    }
    return undefined;
  };
  const end = (start: number): number => {
    const level = indent(lines[start]);
    let i = start + 1;
    while (i < lines.length && (!lines[i].trim() || lines[i].trimStart().startsWith('#') || indent(lines[i]) > level)) i++;
    return i;
  };
  const on = blockUnder(0, lines.length, 'on');
  if (on === undefined) return undefined;
  const push = blockUnder(on + 1, end(on), 'push');
  if (push === undefined) return undefined;
  const at = blockUnder(push + 1, end(push), 'paths');
  if (at === undefined) return undefined;
  const inline = key(lines[at])?.[3]?.trim() ?? '';
  // What this can't read (an alias to an anchored list, a `[` list over
  // several lines) counts as every push, so it holds more, never less.
  // A quoted item may have a comma in it, which splitting would break.
  if (inline.startsWith('[')) return inline.endsWith(']') && !/['"]/.test(inline) ? inline.slice(1, -1).split(',').map(unquote).filter(Boolean) : undefined;
  if (inline) return undefined;
  // Its items, indented further or (as YAML allows) level with it.
  const items: string[] = [];
  for (let i = at + 1; i < lines.length; i++) {
    const l = lines[i].replace(/\s+#.*$/, '');
    if (!l.trim() || l.trimStart().startsWith('#')) continue;
    const item = /^\s*-\s*(.+?)\s*$/.exec(l);
    if (!item || indent(l) < indent(lines[at])) break;
    items.push(unquote(item[1]));
  }
  return items.length ? items : undefined;
}

// A path filter as GitHub matches it: `*` within a folder, `**` across them
// (`a/**/b` takes a/b too), `?` and `+` the character before it zero or one
// and one or more times, `[ab]` and `[a-z]` one of those, and a later
// `!pattern` taking files back out.
function globRegex(glob: string): RegExp {
  let re = '';
  for (let i = 0; i < glob.length; i++) {
    const c = glob[i];
    if (c === '*' && glob[i + 1] === '*') {
      i++;
      if (glob[i + 1] === '/') {
        re += '(?:.*/)?';
        i++;
      } else re += '.*';
    } else if (c === '*') re += '[^/]*';
    else if ((c === '?' || c === '+') && i > 0) re += c;
    else if (c === '[' && glob.indexOf(']', i + 2) > i) {
      const end = glob.indexOf(']', i + 2);
      re += `[${glob.slice(i + 1, end).replace(/[\\^]/g, '\\$&')}]`;
      i = end;
    } else re += c.replace(/[.+^${}()|[\]\\]/g, '\\$&');
  }
  return new RegExp(`^${re}$`);
}

// Which of `files` set the workflow off.
export function filesThatRunIt(paths: string[] | undefined, files: string[]): string[] {
  if (!paths) return files;
  const rules = paths.map((p) => (p.startsWith('!') ? { out: true, re: globRegex(p.slice(1)) } : { out: false, re: globRegex(p) }));
  return files.filter((f) => {
    let hit = false;
    for (const r of rules) if (r.re.test(f)) hit = !r.out;
    return hit;
  });
}

export type Gh = (args: string[], cwd: string) => Promise<{ ok: boolean; stdout: string; stderr: string }>;

const GH_TIMEOUT_MS = 20_000;

export const gh: Gh = (args, cwd) =>
  new Promise((resolve) => {
    execFile('gh', args, { cwd, timeout: GH_TIMEOUT_MS, killSignal: 'SIGKILL', maxBuffer: 4 * 1024 * 1024, env: { ...process.env, GH_PROMPT_DISABLED: '1' } }, (err, stdout, stderr) => {
      resolve({ ok: !err, stdout: String(stdout), stderr: String(stderr || (err as Error | null)?.message || '') });
    });
  });

type Run = { databaseId: number; status: string; conclusion: string; headSha: string; url: string; createdAt: string; displayTitle?: string };

// How many of a workflow's latest runs on main are looked through for one
// that finished: every push cancels the run before it (concurrency), so a
// busy day has runs of cancelled ones.
export const RUNS_LOOKED_AT = 100;

// A run that says nothing about main: stopped for a newer push, or skipped.
const NO_VERDICT = new Set(['cancelled', 'skipped', 'neutral', 'stale', '']);
const RED = new Set(['failure', 'timed_out', 'startup_failure', 'action_required']);

export type Verdict =
  | { state: 'green' | 'none' }
  | { state: 'red'; run: Run; failedJobs: string[]; newer?: Run }
  | { state: 'unknown'; why: string };

// The workflow's last finished run on main, from pushes.
export async function lastVerdict(root: string, file: string, run: Gh = gh): Promise<Verdict> {
  const r = await run(['run', 'list', '--workflow', file, '--branch', 'main', '--event', 'push', '--limit', String(RUNS_LOOKED_AT), '--json', 'databaseId,status,conclusion,headSha,url,createdAt,displayTitle'], root);
  if (!r.ok) return { state: 'unknown', why: firstLine(r.stderr) || 'gh failed' };
  let runs: Run[];
  try {
    runs = JSON.parse(r.stdout) as Run[];
  } catch {
    return { state: 'unknown', why: "gh's answer wasn't JSON" };
  }
  const finished = runs.find((x) => x.status === 'completed' && !NO_VERDICT.has(x.conclusion ?? ''));
  // A whole page of cancelled runs says nothing about the one before them.
  if (!finished) return runs.length >= RUNS_LOOKED_AT ? { state: 'unknown', why: `none of its last ${RUNS_LOOKED_AT} runs on main finished` } : { state: 'none' };
  if (!RED.has(finished.conclusion)) return { state: 'green' };
  // One still going, for a newer push: it may be the fix.
  const newer = runs.find((x) => x.status !== 'completed' && x.createdAt > finished.createdAt);
  let failedJobs: string[] = [];
  const jobs = await run(['run', 'view', String(finished.databaseId), '--json', 'jobs'], root);
  if (jobs.ok) {
    try {
      const parsed = JSON.parse(jobs.stdout) as { jobs?: { name: string; conclusion: string; steps?: { name: string; conclusion: string }[] }[] };
      failedJobs = (parsed.jobs ?? [])
        .filter((j) => RED.has(j.conclusion))
        .map((j) => {
          const step = j.steps?.find((s) => RED.has(s.conclusion))?.name;
          return step ? `${j.name} (${step})` : j.name;
        });
    } catch {
      // Only the detail goes.
    }
  }
  return { state: 'red', run: finished, failedJobs, ...(newer ? { newer } : {}) };
}

const firstLine = (s: string): string => s.trim().split('\n')[0] ?? '';

// Every landing asks GitHub before its check and again before main moves,
// and a dozen threads landing at once (each with gh run watch of its own)
// ran the shared API allowance out on 13 Nov: gh answered 403, and a gate
// that can't ask lets the landing through. So the answer is kept in the
// repo's git folder, shared by every worktree: one from the last minute is
// used as it is, and when GitHub won't answer, one from the last half hour
// still holds a landing on red.
export type GateCache = { dir: string; now?: () => number };
export const FRESH_MS = 60_000;
export const STALE_MS = 30 * 60_000;

type Kept = { at: number; verdict: Verdict };

// The repo's shared git folder, for `root` (a worktree or the main checkout).
export function gateCacheDir(root: string): string | undefined {
  try {
    const out = execFileSync('git', ['rev-parse', '--path-format=absolute', '--git-common-dir'], { cwd: root, encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] }).trim();
    return out ? path.join(out, 'threadbear-land-gate') : undefined;
  } catch {
    return undefined;
  }
}

// lastVerdict through the kept answer, and a line to say when it's an old one.
export async function keptVerdict(root: string, file: string, run: Gh, cache: GateCache | undefined): Promise<{ verdict: Verdict; note?: string }> {
  if (!cache) return { verdict: await lastVerdict(root, file, run) };
  const now = (cache.now ?? Date.now)();
  const at = path.join(cache.dir, `${file}.json`);
  let kept: Kept | undefined;
  try {
    kept = JSON.parse(fs.readFileSync(at, 'utf8')) as Kept;
  } catch {
    // none yet, or unreadable: ask
  }
  const age = kept ? now - kept.at : Infinity;
  if (kept && age >= 0 && age < FRESH_MS) return { verdict: kept.verdict };
  const verdict = await lastVerdict(root, file, run);
  if (verdict.state !== 'unknown') {
    try {
      fs.mkdirSync(cache.dir, { recursive: true });
      fs.writeFileSync(`${at}.tmp`, JSON.stringify({ at: now, verdict }));
      fs.renameSync(`${at}.tmp`, at);
    } catch {
      // Only the sharing goes.
    }
    return { verdict };
  }
  if (kept && age >= 0 && age < STALE_MS) {
    const mins = Math.max(1, Math.round(age / 60_000));
    return { verdict: kept.verdict, note: `GitHub didn't answer (${verdict.why}), so this goes by what it said ${mins} min ago.` };
  }
  return { verdict };
}

// Why `files` can't land on main now, or undefined when they can. `note`
// gets a line for a workflow GitHub couldn't be asked about.
export async function landHold(root: string, files: string[], note: (line: string) => void, run: Gh = gh, cache?: GateCache): Promise<string | undefined> {
  for (const w of holdingWorkflows(root)) {
    const hits = filesThatRunIt(w.paths, files);
    if (!hits.length) continue;
    const kept = await keptVerdict(root, w.file, run, cache);
    const v = kept.verdict;
    if (kept.note) note(kept.note);
    if (v.state === 'unknown') {
      note(`Couldn't ask GitHub whether ${w.name} (${w.file}) is green on main (${v.why}), so this lands without that check.`);
      continue;
    }
    if (v.state !== 'red') continue;
    const shown = hits.slice(0, 5).map((f) => `  ${f}`);
    if (hits.length > shown.length) shown.push(`  and ${hits.length - shown.length} more`);
    return [
      `${w.name} (${w.file}) failed on main's last run, so nothing landed: this changes files it checks, and it holds landings while it's red so they don't pile onto what's broken.`,
      `  ${v.run.url} (${v.run.headSha.slice(0, 8)}, ${v.run.createdAt}${v.run.displayTitle ? `: ${v.run.displayTitle}` : ''})`,
      ...(v.failedJobs.length ? [`  Failed: ${v.failedJobs.join('; ')}`] : []),
      ...(v.newer ? [`  A newer run is still going (${v.newer.url}); if it passes, this can land.`] : []),
      'The files of yours it checks:',
      ...shown,
      `See what failed with gh run view ${v.run.databaseId} --log-failed, and fix that first.`,
      'If this landing is that fix, or main already has one GitHub hasn\'t run yet, run tb land --despite-red.',
    ].join('\n');
  }
  return undefined;
}
