// scripts/swift-freeze.sh keeps new code out of the Swift Mac app (app/,
// dock/) now Mike uses the GPUI one (12 Nov 2026): it refuses in his copy,
// and once WARN_FORKS is on, only warns in other people's. Each test plays it
// in a throwaway repo, as git's pre-commit hook and as Claude Code's
// PreToolUse hook.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const script = path.join(path.dirname(fileURLToPath(import.meta.url)), '../../../scripts/swift-freeze.sh');

function repo(origin: string, warnForks = false): string {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'swift-freeze-'));
  const git = (...args: string[]) => spawnSync('git', args, { cwd: dir, encoding: 'utf8' });
  git('init', '-q', '-b', 'main');
  git('remote', 'add', 'origin', origin);
  fs.mkdirSync(path.join(dir, 'scripts'));
  let text = fs.readFileSync(script, 'utf8');
  if (warnForks) text = text.replace(/^WARN_FORKS=0$/m, 'WARN_FORKS=1');
  fs.writeFileSync(path.join(dir, 'scripts/swift-freeze.sh'), text, { mode: 0o755 });
  return dir;
}

function stage(dir: string, file: string) {
  fs.mkdirSync(path.dirname(path.join(dir, file)), { recursive: true });
  fs.writeFileSync(path.join(dir, file), 'x\n');
  spawnSync('git', ['add', file], { cwd: dir });
}

const env = (extra: Record<string, string> = {}) => {
  const e: Record<string, string | undefined> = { ...process.env, ...extra };
  delete e.CLAUDE_PROJECT_DIR;
  delete e.THREADBEAR_SWIFT_OK;
  delete e.THREADBEAR_SWIFT_FREEZE_COPY;
  return { ...e, ...extra };
};

function commit(dir: string, extra: Record<string, string> = {}) {
  return spawnSync('scripts/swift-freeze.sh', ['--commit'], { cwd: dir, encoding: 'utf8', env: env(extra) });
}

function edit(dir: string, file: string, extra: Record<string, string> = {}) {
  const input = JSON.stringify({ tool_name: 'Edit', tool_input: { file_path: path.join(dir, file), old_string: 'a', new_string: 'b' } });
  return spawnSync(path.join(dir, 'scripts/swift-freeze.sh'), ['--tool'], {
    cwd: dir, encoding: 'utf8', input, env: env({ CLAUDE_PROJECT_DIR: dir, ...extra }),
  });
}

const MIKE = 'https://github.com/mikecann/threadbear.git';
const FORK = 'git@github.com:someone/threadbear.git';

test("Mike's copy refuses a commit that adds to the Swift app", () => {
  const dir = repo(MIKE);
  stage(dir, 'app/Sources/Threadbear/New.swift');
  stage(dir, 'desktop/gpui/src/new.rs');
  const r = commit(dir);
  assert.equal(r.status, 2);
  assert.match(r.stderr, /frozen/);
  assert.match(r.stderr, /app\/Sources\/Threadbear\/New\.swift/);
  assert.doesNotMatch(r.stderr, /new\.rs/);
});

test("Mike's copy refuses BearDock too, and an ssh origin counts as his", () => {
  const dir = repo('git@github.com:mikecann/threadbear.git');
  stage(dir, 'dock/Sources/BearDock/Drag.swift');
  assert.equal(commit(dir).status, 2);
});

test('the GPUI app, the shared test tools and deletions go through', () => {
  const dir = repo(MIKE);
  stage(dir, 'desktop/gpui/src/new.rs');
  stage(dir, 'app/Tools/mock-runner/server.ts');
  stage(dir, 'app/Tools/post-gesture.swift');
  assert.equal(commit(dir).status, 0);
  spawnSync('git', ['commit', '-qm', 'x', '--no-verify'], { cwd: dir, env: env({ GIT_AUTHOR_NAME: 't', GIT_AUTHOR_EMAIL: 't@t', GIT_COMMITTER_NAME: 't', GIT_COMMITTER_EMAIL: 't@t' }) });
  const old = repo(MIKE);
  stage(old, 'app/Sources/Old.swift');
  spawnSync('git', ['commit', '-qm', 'x', '--no-verify'], { cwd: old, env: env({ GIT_AUTHOR_NAME: 't', GIT_AUTHOR_EMAIL: 't@t', GIT_COMMITTER_NAME: 't', GIT_COMMITTER_EMAIL: 't@t' }) });
  spawnSync('git', ['rm', '-q', 'app/Sources/Old.swift'], { cwd: old });
  assert.equal(commit(old).status, 0, 'removing Swift code is fine');
});

test('Mike asking for a Swift change lets it through, by file or variable', () => {
  const dir = repo(MIKE);
  stage(dir, 'app/Sources/Threadbear/New.swift');
  assert.equal(commit(dir, { THREADBEAR_SWIFT_OK: '1' }).status, 0);
  fs.writeFileSync(path.join(dir, '.git/threadbear-swift-ok'), '');
  assert.equal(commit(dir).status, 0);
  assert.equal(edit(dir, 'app/Sources/Threadbear/New.swift').status, 0);
});

test("Mike's copy refuses an agent's edit to the Swift app before it's written", () => {
  const dir = repo(MIKE);
  const r = edit(dir, 'app/Sources/Threadbear/ThreadView.swift');
  assert.equal(r.status, 2);
  assert.match(r.stderr, /desktop\//);
  assert.equal(edit(dir, 'desktop/gpui/src/thread_view.rs').status, 0);
  assert.equal(edit(dir, 'app/Tools/mock-runner/server.ts').status, 0);
});

test("someone else's copy says nothing until the GPUI app is released to them", () => {
  const dir = repo(FORK);
  stage(dir, 'app/Sources/Threadbear/New.swift');
  const c = commit(dir);
  assert.equal(c.status, 0);
  assert.equal(c.stderr, '');
  const e = edit(dir, 'app/Sources/Threadbear/New.swift');
  assert.equal(e.status, 0);
  assert.equal(e.stdout, '');
});

test("once released, someone else's copy warns but doesn't refuse", () => {
  const dir = repo(FORK, true);
  stage(dir, 'app/Sources/Threadbear/New.swift');
  const c = commit(dir);
  assert.equal(c.status, 0);
  assert.match(c.stderr, /being retired in favour of the GPUI app/);
  const e = edit(dir, 'app/Sources/Threadbear/New.swift');
  assert.equal(e.status, 0);
  const out = JSON.parse(e.stdout);
  assert.equal(out.hookSpecificOutput.hookEventName, 'PreToolUse');
  assert.match(out.hookSpecificOutput.additionalContext, /desktop\//);
  assert.equal(out.hookSpecificOutput.permissionDecision, undefined, 'a warning never approves the edit');
});

test('finishing a merge brings in commits already made, so it passes', () => {
  const dir = repo(MIKE);
  stage(dir, 'app/Sources/Threadbear/New.swift');
  fs.writeFileSync(path.join(dir, '.git/MERGE_HEAD'), '');
  // MERGE_HEAD has to name a commit for rev-parse --verify.
  spawnSync('git', ['commit', '-qm', 'x', '--no-verify', '--allow-empty'], { cwd: dir, env: env({ GIT_AUTHOR_NAME: 't', GIT_AUTHOR_EMAIL: 't@t', GIT_COMMITTER_NAME: 't', GIT_COMMITTER_EMAIL: 't@t' }) });
  const head = spawnSync('git', ['rev-parse', 'HEAD'], { cwd: dir, encoding: 'utf8' }).stdout.trim();
  fs.writeFileSync(path.join(dir, '.git/MERGE_HEAD'), head + '\n');
  stage(dir, 'app/Sources/Threadbear/Other.swift');
  assert.equal(commit(dir).status, 0);
});

test('what the GPUI app uses from app/ stays open: fixtures, art, rich scripts', () => {
  const dir = repo(MIKE);
  stage(dir, 'app/Tests/Fixtures/inbox-urgency-cases.json');
  stage(dir, 'app/Sources/Threadbear/Resources/Art/librarian.png');
  stage(dir, 'app/Sources/Threadbear/Resources/Rich/pele.js');
  assert.equal(commit(dir).status, 0);
});

test("what the iPhone app builds from app/ stays open, found from ThreadbearKit's links", () => {
  const dir = repo(MIKE);
  stage(dir, 'app/Sources/ThreadbearCore/Threads/SlashCommands.swift');
  stage(dir, 'app/SentryBinary/Sentry.xcframework/Info.plist');
  const kit = path.join(dir, 'ios/ThreadbearKit/Sources/ThreadbearKit');
  fs.mkdirSync(kit, { recursive: true });
  fs.symlinkSync('../../../../app/Sources/ThreadbearCore/Threads/SlashCommands.swift', path.join(kit, 'SlashCommands.swift'));
  assert.equal(commit(dir).status, 0);
  assert.equal(edit(dir, 'app/Sources/ThreadbearCore/Threads/SlashCommands.swift').status, 0);
  stage(dir, 'app/Sources/ThreadbearCore/Threads/ThreadSections.swift');
  assert.equal(commit(dir).status, 2, 'a file next to it the kit has no link to is still frozen');
});

// check.sh's DESKTOP_NEEDS is what the GPUI app's checks read from app/. Each
// of those has to be open in the freeze, or the GPUI app couldn't change it,
// and has to not count as a Swift change, or it would build the Swift app.
test("everything the GPUI app's checks read from app/ is shared, not frozen", () => {
  const root = path.join(path.dirname(fileURLToPath(import.meta.url)), '../../..');
  const check = fs.readFileSync(path.join(root, 'check.sh'), 'utf8');
  const needs = check.match(/^DESKTOP_NEEDS='([^']*)'/m)?.[1];
  assert.ok(needs, 'DESKTOP_NEEDS in check.sh');
  const shared = new RegExp(spawnSync(script, ['--gpui-shared'], { encoding: 'utf8' }).stdout.trim());
  const fromApp = needs.split('|').filter((p) => p.startsWith('^app/'));
  assert.ok(fromApp.length >= 3);
  for (const p of fromApp) {
    const example = p.slice(1).replace(/\\\./g, '.') + 'example';
    assert.match(example, shared, p);
  }
});

// Codex's review, 12 Nov: each of these got past the first version.
test('a name git would quote, and a file swapped for a link, are still caught', () => {
  const dir = repo(MIKE);
  stage(dir, 'app/Sources/Café "new".swift');
  assert.equal(commit(dir).status, 2);
  const swap = repo(MIKE);
  stage(swap, 'app/Sources/A.swift');
  stage(swap, 'app/Sources/B.swift');
  spawnSync('git', ['commit', '-qm', 'x', '--no-verify'], { cwd: swap, env: env({ GIT_AUTHOR_NAME: 't', GIT_AUTHOR_EMAIL: 't@t', GIT_COMMITTER_NAME: 't', GIT_COMMITTER_EMAIL: 't@t' }) });
  fs.rmSync(path.join(swap, 'app/Sources/A.swift'));
  fs.symlinkSync('B.swift', path.join(swap, 'app/Sources/A.swift'));
  spawnSync('git', ['add', 'app/Sources/A.swift'], { cwd: swap });
  assert.equal(commit(swap).status, 2);
});

test("an edit's path counts however it's written", () => {
  const dir = repo(MIKE);
  fs.mkdirSync(path.join(dir, 'desktop'));
  fs.mkdirSync(path.join(dir, 'app'));
  const run = (file: string) => {
    const input = JSON.stringify({ tool_name: 'Write', tool_input: { file_path: file, content: 'x' } });
    return spawnSync(path.join(dir, 'scripts/swift-freeze.sh'), ['--tool'], { cwd: dir, encoding: 'utf8', input, env: env({ CLAUDE_PROJECT_DIR: dir }) }).status;
  };
  assert.equal(run('app/Sources/New.swift'), 2, 'relative');
  assert.equal(run(`${dir}/desktop/../app/Sources/New.swift`), 2, 'through ..');
  assert.equal(run(`${fs.realpathSync(dir)}/app/Sources/New.swift`), 2, 'through the real path');
  assert.equal(run(`${dir}/app/../desktop/new.rs`), 0, 'out of app/ through ..');
  assert.equal(run(`${dir}/app/Tools/realdrag.swift`), 0, 'a test tool the GPUI specs compile');
  assert.equal(run('/tmp/elsewhere/app/x.swift'), 0, 'another folder');
});

test('the installed hook refuses a real commit, in a worktree older than the script too', () => {
  const dir = repo(MIKE);
  const who = env({ GIT_AUTHOR_NAME: 't', GIT_AUTHOR_EMAIL: 't@t', GIT_COMMITTER_NAME: 't', GIT_COMMITTER_EMAIL: 't@t' });
  const hook = path.join(dir, '.git/hooks/pre-commit');
  fs.mkdirSync(path.dirname(hook), { recursive: true });
  fs.copyFileSync(path.join(path.dirname(script), 'swift-freeze-pre-commit'), hook);
  fs.chmodSync(hook, 0o755);
  // The first commit leaves the script out, as a branch from before it would.
  spawnSync('git', ['commit', '-qm', 'base', '--allow-empty'], { cwd: dir, env: who });
  stage(dir, 'app/Sources/New.swift');
  const here = spawnSync('git', ['commit', '-qm', 'swift'], { cwd: path.join(dir, 'app'), encoding: 'utf8', env: who });
  assert.notEqual(here.status, 0, 'from a subfolder of the main checkout');
  assert.match(here.stderr, /frozen/);
  const wt = `${dir}-wt`;
  spawnSync('git', ['worktree', 'add', '-q', wt, 'HEAD'], { cwd: dir });
  assert.ok(!fs.existsSync(path.join(wt, 'scripts/swift-freeze.sh')));
  stage(wt, 'app/Sources/Old.swift');
  const old = spawnSync('git', ['commit', '-qm', 'swift'], { cwd: wt, encoding: 'utf8', env: who });
  assert.notEqual(old.status, 0, "an older worktree uses the main checkout's script");
  assert.match(old.stderr, /frozen/);
  stage(wt, 'desktop/gpui/src/new.rs');
  spawnSync('git', ['reset', '-q', 'app/Sources/Old.swift'], { cwd: wt });
  assert.equal(spawnSync('git', ['commit', '-qm', 'gpui'], { cwd: wt, env: who }).status, 0);
});

test("a path's parts aren't globbed, and macOS's Data-volume name is the same disk", () => {
  const dir = repo(MIKE);
  fs.mkdirSync(path.join(dir, 'app'));
  const run = (file: string, project = dir) => {
    const input = JSON.stringify({ tool_name: 'Write', tool_input: { file_path: file, content: 'x' } });
    return spawnSync(path.join(dir, 'scripts/swift-freeze.sh'), ['--tool'], { cwd: dir, encoding: 'utf8', input, env: env({ CLAUDE_PROJECT_DIR: project }) }).status;
  };
  assert.equal(run(`${dir}/a[p]p/New.swift`), 0, 'a[p]p is a folder of its own, not app');
  const data = '/System/Volumes/Data';
  if (process.platform === 'darwin' && fs.existsSync(data + fs.realpathSync(dir))) {
    const real = fs.realpathSync(dir);
    assert.equal(run(`${data}${real}/app/New.swift`), 2, 'the file by its Data name');
    assert.equal(run(`${real}/app/New.swift`, data + real), 2, 'the checkout by its Data name');
  }
});
