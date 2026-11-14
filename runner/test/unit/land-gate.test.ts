// tb land holding a landing while a GitHub workflow it'd set off is red on
// main (land-gate.ts).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { tmpDir } from '../helpers/fakes.ts';
import { FRESH_MS, filesThatRunIt, holdingWorkflows, landHold, lastVerdict, pushPaths, STALE_MS, type Gh } from '../../src/land-gate.ts';
import { land } from '../../src/worktrees.ts';
import { capture, commit, makeBranch, makeRepo, sh } from '../helpers/worktrees-harness.ts';

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../../..');

const WORKFLOW = `# What it checks.
# tb land: hold while red
name: Desktop app

on:
  push:
    branches: [main]
    paths:
      - desktop/**
      - 'runner/src/protocol.ts'   # the contract
      - "features/**"
      - '!features/README.md'
  pull_request:
    paths:
      - pr-only/**
  workflow_dispatch:

jobs:
  build:
    runs-on: ubuntu-24.04
`;

function workflows(files: Record<string, string>): string {
  const root = tmpDir();
  fs.mkdirSync(path.join(root, '.github/workflows'), { recursive: true });
  for (const [name, text] of Object.entries(files)) fs.writeFileSync(path.join(root, '.github/workflows', name), text);
  return root;
}

// gh as GitHub would answer: the runs, newest first, and each run's jobs.
function fakeGh(runs: object[] | 'offline', jobs: Record<number, object[]> = {}): Gh & { calls: string[][] } {
  const calls: string[][] = [];
  const fn = async (args: string[]) => {
    calls.push(args);
    if (runs === 'offline') return { ok: false, stdout: '', stderr: 'error connecting to api.github.com\nmore' };
    if (args[1] === 'list') return { ok: true, stdout: JSON.stringify(runs), stderr: '' };
    return { ok: true, stdout: JSON.stringify({ jobs: jobs[Number(args[2])] ?? [] }), stderr: '' };
  };
  return Object.assign(fn, { calls });
}

const run = (id: number, conclusion: string, at: string, status = 'completed') => ({ databaseId: id, status, conclusion, headSha: `${id}abcdef0123`, url: `https://github.com/o/r/actions/runs/${id}`, createdAt: at, displayTitle: `Commit ${id}` });

test("a workflow's push paths are read from its trigger, not the pull request's", () => {
  assert.deepEqual(pushPaths(WORKFLOW), ['desktop/**', 'runner/src/protocol.ts', 'features/**', '!features/README.md']);
  assert.deepEqual(pushPaths('on:\n  push:\n    branches: [main]\n    paths: [a/**, b]\n'), ['a/**', 'b']);
  // Items level with their key, as YAML allows.
  assert.deepEqual(pushPaths('on:\n  push:\n    paths:\n    - a/**\n    - b\n    branches: [main]\n'), ['a/**', 'b']);
  // No paths: every push runs it.
  assert.equal(pushPaths('on:\n  push:\n    branches: [main]\n  pull_request:\n    paths:\n      - x/**\n'), undefined);
  assert.equal(pushPaths('on: [push]\n'), undefined);
  // What it can't read holds every landing rather than none (Codex's review).
  assert.equal(pushPaths('on:\n  push:\n    paths: *desktop_paths\n'), undefined);
  assert.equal(pushPaths('on:\n  push:\n    paths: [\n      a/**,\n    ]\n'), undefined);
});

test("path filters match as GitHub's do", () => {
  assert.deepEqual(filesThatRunIt(['desktop/**/file.rs'], ['desktop/file.rs', 'desktop/a/b/file.rs', 'desktop/notfile.rs']), ['desktop/file.rs', 'desktop/a/b/file.rs']);
  assert.deepEqual(filesThatRunIt(['src/[ab].ts', 'docs/v[0-9].md'], ['src/a.ts', 'src/c.ts', 'docs/v2.md', 'docs/vx.md']), ['src/a.ts', 'docs/v2.md']);
  assert.deepEqual(filesThatRunIt(['**', '!**.md'], ['a/b.rs', 'a/b.md']), ['a/b.rs']);
  assert.deepEqual(filesThatRunIt(['desktop/file[0-9]+.rs', 'x/ab?c'], ['desktop/file12.rs', 'desktop/file.rs', 'x/ac', 'x/abc', 'x/abbc']), ['desktop/file12.rs', 'x/ac', 'x/abc']);
  assert.equal(pushPaths(`on:\n  push:\n    paths: ['desktop/a,b.rs']\n`), undefined);
});

test("this repo's Desktop app workflow holds landings to desktop/ and features/", () => {
  const held = holdingWorkflows(repoRoot);
  const desktop = held.find((w) => w.file === 'desktop.yml');
  assert.ok(desktop, 'desktop.yml has the mark');
  assert.equal(desktop.name, 'Desktop app');
  assert.deepEqual(filesThatRunIt(desktop.paths, ['desktop/gpui/src/terminal.rs', 'features/terminal.feature', 'runner/src/cli.ts', 'docs/guide.md']), ['desktop/gpui/src/terminal.rs', 'features/terminal.feature']);
});

test('only marked workflows hold, and their paths say which files', () => {
  const root = workflows({ 'desktop.yml': WORKFLOW, 'other.yml': WORKFLOW.replace('# tb land: hold while red\n', ''), 'notes.txt': '# tb land: hold while red' });
  assert.deepEqual(holdingWorkflows(root).map((w) => w.file), ['desktop.yml']);
  const paths = holdingWorkflows(root)[0].paths;
  assert.deepEqual(filesThatRunIt(paths, ['desktop/a/b.rs', 'features/x.feature', 'features/README.md', 'runner/src/protocol.ts', 'runner/src/protocol.tsx', 'pr-only/x', 'desktopish/a']), ['desktop/a/b.rs', 'features/x.feature', 'runner/src/protocol.ts']);
  assert.deepEqual(filesThatRunIt(undefined, ['anything']), ['anything']);
  assert.deepEqual(filesThatRunIt(['*.md'], ['a.md', 'docs/a.md']), ['a.md']);
  assert.deepEqual(holdingWorkflows(tmpDir()), []);
});

test("the last finished run on main is the verdict, past cancelled ones", async () => {
  const root = tmpDir();
  // Newest first: one going, one cancelled for a newer push, then a failure.
  const gh = fakeGh([run(4, '', '2026-11-12T03:00:00Z', 'in_progress'), run(3, 'cancelled', '2026-11-12T02:00:00Z'), run(2, 'failure', '2026-11-12T01:00:00Z'), run(1, 'success', '2026-11-11T01:00:00Z')], {
    2: [{ name: 'Mac', conclusion: 'failure', steps: [{ name: 'Set up', conclusion: 'success' }, { name: 'Behaviour specs', conclusion: 'failure' }] }, { name: 'Linux', conclusion: 'success' }, { name: 'Windows', conclusion: 'failure' }],
  });
  const v = await lastVerdict(root, 'desktop.yml', gh);
  assert.equal(v.state, 'red');
  assert.ok(v.state === 'red');
  assert.equal(v.run.databaseId, 2);
  assert.deepEqual(v.failedJobs, ['Mac (Behaviour specs)', 'Windows']);
  assert.equal(v.newer?.databaseId, 4);
  assert.deepEqual(gh.calls[0].slice(0, 9), ['run', 'list', '--workflow', 'desktop.yml', '--branch', 'main', '--event', 'push', '--limit']);
  assert.equal((await lastVerdict(root, 'x.yml', fakeGh([run(5, 'success', 'b'), run(2, 'failure', 'a')]))).state, 'green');
  assert.equal((await lastVerdict(root, 'x.yml', fakeGh([run(5, '', 'b', 'queued')]))).state, 'none');
  assert.deepEqual(await lastVerdict(root, 'x.yml', fakeGh('offline')), { state: 'unknown', why: 'error connecting to api.github.com' });
  // A whole page of cancelled runs can't say the failure before them was fixed.
  const cancelled = Array.from({ length: 100 }, (_, i) => run(200 - i, 'cancelled', `c${i}`));
  assert.equal((await lastVerdict(root, 'x.yml', fakeGh(cancelled))).state, 'unknown');
});

test('a landing touching what a red workflow checks is held, with what failed and how to go on', async () => {
  const root = workflows({ 'desktop.yml': WORKFLOW });
  const gh = fakeGh([run(7, 'failure', '2026-11-12T01:00:00Z')], { 7: [{ name: 'Linux', conclusion: 'failure', steps: [{ name: 'Behaviour specs', conclusion: 'failure' }] }] });
  const notes: string[] = [];
  const held = await landHold(root, ['desktop/gpui/src/a.rs', 'docs/x.md'], (l) => notes.push(l), gh);
  assert.ok(held);
  assert.match(held, /^Desktop app \(desktop\.yml\) failed on main's last run, so nothing landed/);
  assert.match(held, /https:\/\/github\.com\/o\/r\/actions\/runs\/7 \(7abcdef0/);
  assert.match(held, /Failed: Linux \(Behaviour specs\)/);
  assert.match(held, /^ {2}desktop\/gpui\/src\/a\.rs$/m);
  assert.doesNotMatch(held, /docs\/x\.md/);
  assert.match(held, /gh run view 7 --log-failed/);
  assert.match(held, /tb land --despite-red/);
  assert.equal(notes.length, 0);
  // Files it doesn't check never ask GitHub at all.
  const quiet = fakeGh([run(7, 'failure', 'a')]);
  assert.equal(await landHold(root, ['docs/x.md', 'features/README.md'], () => {}, quiet), undefined);
  assert.deepEqual(quiet.calls, []);
  // Green goes ahead; GitHub out of reach goes ahead, saying so.
  assert.equal(await landHold(root, ['desktop/a'], () => {}, fakeGh([run(8, 'success', 'a')])), undefined);
  assert.equal(await landHold(root, ['desktop/a'], (l) => notes.push(l), fakeGh('offline')), undefined);
  assert.match(notes.join('\n'), /Couldn't ask GitHub whether Desktop app \(desktop\.yml\) is green on main \(error connecting to api\.github\.com\), so this lands without that check/);
});

test("GitHub's answer is shared for a minute, and an old red one still holds when GitHub won't answer", async () => {
  const root = workflows({ 'desktop.yml': WORKFLOW });
  let now = 1_000_000;
  const cache = { dir: path.join(tmpDir(), 'gate'), now: () => now };
  const red = fakeGh([run(7, 'failure', 'a')], { 7: [{ name: 'Linux', conclusion: 'failure' }] });
  assert.ok(await landHold(root, ['desktop/a'], () => {}, red, cache));
  // Within the minute another landing doesn't ask again.
  now += FRESH_MS - 1;
  const quiet = fakeGh([run(8, 'success', 'b')]);
  assert.ok(await landHold(root, ['desktop/a'], () => {}, quiet, cache));
  assert.deepEqual(quiet.calls, []);
  // After it, GitHub out of reach (a 403 for the rate limit): still held, saying why.
  now += 2;
  const notes: string[] = [];
  assert.ok(await landHold(root, ['desktop/a'], (l) => notes.push(l), fakeGh('offline'), cache));
  assert.match(notes.join('\n'), /GitHub didn't answer \(error connecting to api\.github\.com\), so this goes by what it said 1 min ago/);
  // A green answer replaces it.
  assert.equal(await landHold(root, ['desktop/a'], () => {}, fakeGh([run(9, 'success', 'c')]), cache), undefined);
  // Older than half an hour, it's no answer at all, and the landing goes ahead as before.
  now += FRESH_MS;
  assert.ok(await landHold(root, ['desktop/a'], () => {}, red, cache));
  now += STALE_MS;
  const late: string[] = [];
  assert.equal(await landHold(root, ['desktop/a'], (l) => late.push(l), fakeGh('offline'), cache), undefined);
  assert.match(late.join('\n'), /so this lands without that check/);
});

test('tb land asks after the rebase, about what the branch changes, and a hold leaves main as it was before any check', async () => {
  const { main } = makeRepo();
  const t = makeBranch(main);
  commit(main, { 'on-main.txt': 'main moved\n' }, 'Main moves');
  commit(t.path, { 'desktop/a.rs': 'fn a() {}\n', 'desktop/café.rs': '\n' }, 'Desktop change');
  const before = sh(main, 'git rev-parse main');
  const asked: { root: string; files: string[] }[] = [];
  const c = capture();
  const code = await land({ ...t, check: 'touch checked' }, { ...c.opts, hold: async (root, files) => (asked.push({ root, files }), 'Held: red on main.') });
  assert.equal(code, 1);
  assert.match(c.err.join('\n'), /Held: red on main\./);
  // A name with other letters in it as it is, not git's quoted form.
  assert.deepEqual(asked, [{ root: t.path, files: ['desktop/a.rs', 'desktop/café.rs'] }]);
  assert.equal(sh(main, 'git rev-parse main'), before);
  assert.ok(!fs.existsSync(path.join(t.path, 'checked')), 'the check never ran');
  // Gone red while the check ran: asked again before main moves.
  let calls = 0;
  const late = capture();
  assert.equal(await land({ ...t, check: 'true' }, { ...late.opts, hold: async () => (++calls === 2 ? 'Red now.' : undefined) }), 1);
  assert.match(late.err.join('\n'), /Red now\./);
  assert.equal(sh(main, 'git rev-parse main'), before);
  // Not held: it lands as before.
  const ok = capture();
  assert.equal(await land({ ...t, check: 'true' }, { ...ok.opts, hold: async () => undefined }), 0, ok.err.join('\n'));
  assert.equal(sh(main, 'git rev-parse main'), sh(t.path, 'git rev-parse HEAD'));
});
