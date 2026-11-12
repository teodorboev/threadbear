import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { execFileSync } from 'node:child_process';
import { tmpDir } from '../helpers/fakes.ts';
import { claudeCommands } from '../../src/claude-trust.ts';
import { trackedNodeModules } from '../../src/tracked-packages.ts';
import { git as worktreeGit } from '../../src/worktrees.ts';
import { changedFiles } from '../../src/move-git.ts';
import { untrustedGit, untrustedGitSync } from '../../src/safe-git.ts';

// A folder that arrived with its own .git (a zip, a shared drive), whose
// .git/config names an fsmonitor program. Nothing the runner does before
// the folder is trusted may start it (security review, 9 Nov 2026: the
// Claude settings check's `git ls-files` did, in every mode, before the
// first turn).
function planted(): { repo: string; ran: () => number } {
  const root = tmpDir();
  const repo = path.join(root, 'from-a-zip');
  fs.mkdirSync(repo);
  const g = (...args: string[]) => execFileSync('git', ['-C', repo, ...args], { stdio: 'pipe' });
  g('init', '-q');
  fs.writeFileSync(path.join(repo, 'README.md'), 'hi\n');
  g('add', 'README.md');
  g('-c', 'user.email=t@t', '-c', 'user.name=t', 'commit', '-qm', 'first');
  // Untracked and changed files too, so every listing reads the working tree.
  fs.writeFileSync(path.join(repo, 'untracked.txt'), 'x\n');
  fs.writeFileSync(path.join(repo, 'README.md'), 'changed\n');
  const marker = path.join(root, 'fsmonitor-ran');
  const hook = path.join(root, 'fsmonitor.sh');
  fs.writeFileSync(hook, `#!/bin/sh\necho ran >> '${marker}'\nexit 1\n`, { mode: 0o755 });
  g('config', 'core.fsmonitor', hook);
  const ran = () => (fs.existsSync(marker) ? fs.readFileSync(marker, 'utf8').split('\n').filter(Boolean).length : 0);
  return { repo, ran };
}

test('the planted fsmonitor really runs under plain git, so the checks below mean something', () => {
  const { repo, ran } = planted();
  execFileSync('git', ['-C', repo, 'status', '--porcelain'], { stdio: 'pipe' });
  assert.ok(ran() > 0);
});

test("the Claude settings check lists a folder's files without starting its fsmonitor", () => {
  const { repo, ran } = planted();
  // A nested skill with a hook, so the listing has something to find, and
  // finding it shows the listing really ran.
  const skill = path.join(repo, 'sub', '.claude', 'skills', 's', 'SKILL.md');
  fs.mkdirSync(path.dirname(skill), { recursive: true });
  fs.writeFileSync(skill, '---\nname: s\ndescription: x\nhooks:\n  Stop:\n    - hooks:\n        - type: command\n          command: say hi\n---\n');
  assert.ok(claudeCommands(repo, tmpDir())?.files.includes(skill), 'the nested skill is found');
  claudeCommands(path.join(repo, 'sub'), tmpDir());
  assert.equal(ran(), 0);
});

test("tracked packages, worktree git and a move's changed files leave the fsmonitor off", async () => {
  const { repo, ran } = planted();
  trackedNodeModules(repo);
  const st = await worktreeGit(repo, ['status', '--porcelain', '--untracked-files=all']);
  assert.ok(st.ok, st.stderr);
  assert.match(st.stdout, /untracked\.txt/);
  assert.deepEqual((await changedFiles(repo)).sort(), ['README.md', 'untracked.txt']);
  assert.equal(ran(), 0);
});

test("git config the runner's environment carries can't turn the fsmonitor back on for a listing", () => {
  const { repo, ran } = planted();
  const hook = execFileSync('git', ['-C', repo, 'config', 'core.fsmonitor'], { stdio: 'pipe' }).toString().trim();
  const saved = { ...process.env };
  try {
    Object.assign(process.env, { GIT_CONFIG_COUNT: '1', GIT_CONFIG_KEY_0: 'core.fsmonitor', GIT_CONFIG_VALUE_0: hook, GIT_CONFIG_PARAMETERS: `'core.fsmonitor'='${hook}'` });
    assert.match(untrustedGitSync(repo, ['ls-files', '-o', '--exclude-standard']), /untracked\.txt/);
  } finally {
    for (const k of Object.keys(process.env)) if (!(k in saved)) delete process.env[k];
  }
  assert.equal(ran(), 0);
});

// The same folder can say it's a partial clone with a tree missing from a
// sparse index: git then fetches the tree by itself as it reads the index,
// through the repo's own core.sshCommand (Codex's review of this fix,
// 9 Nov 2026). A listing before trust never fetches.
function lazy(): { repo: string; ran: () => number } {
  const root = tmpDir();
  const repo = path.join(root, 'partial');
  const g = (...args: string[]) => execFileSync('git', ['-C', repo, ...args], { stdio: 'pipe' }).toString().trim();
  fs.mkdirSync(path.join(repo, 'd', 'e'), { recursive: true });
  g('init', '-q');
  fs.writeFileSync(path.join(repo, 'd', 'e', 'f'), 'a\n');
  fs.writeFileSync(path.join(repo, 'top'), 'b\n');
  g('add', '.');
  g('-c', 'user.email=t@t', '-c', 'user.name=t', 'commit', '-qm', 'first');
  g('config', 'index.sparse', 'true');
  g('sparse-checkout', 'set', '--cone');
  const tree = g('rev-parse', 'HEAD:d');
  fs.rmSync(path.join(repo, '.git', 'objects', tree.slice(0, 2), tree.slice(2)));
  const marker = path.join(root, 'ssh-ran');
  const hook = path.join(root, 'ssh.sh');
  fs.writeFileSync(hook, `#!/bin/sh\necho ran >> '${marker}'\nexit 1\n`, { mode: 0o755 });
  g('config', 'remote.origin.url', 'ssh://example.invalid/x');
  g('config', 'remote.origin.promisor', 'true');
  g('config', 'extensions.partialClone', 'origin');
  g('config', 'core.sshCommand', hook);
  const ran = () => (fs.existsSync(marker) ? fs.readFileSync(marker, 'utf8').split('\n').filter(Boolean).length : 0);
  return { repo, ran };
}

test('the planted lazy fetch really runs under plain git', () => {
  const { repo, ran } = lazy();
  try {
    execFileSync('git', ['-C', repo, 'ls-files', '-s'], { stdio: 'pipe' });
  } catch {
    // It fails to fetch; what matters is that it tried.
  }
  assert.ok(ran() > 0);
});

test("a listing before trust doesn't fetch a partial clone's missing objects", () => {
  const { repo, ran } = lazy();
  assert.match(untrustedGitSync(repo, ['ls-files', '-s']), /top/);
  untrustedGitSync(repo, ['ls-files', '-co', '--exclude-standard']);
  claudeCommands(repo, tmpDir());
  trackedNodeModules(repo);
  assert.equal(ran(), 0);
});

// A submodule is a repo of its own, and git runs another git inside it to
// see whether it changed, which reads the submodule's own config: a
// `filter.<name>.clean` there ran through untrustedGit's `git status`,
// though every filter the outer repo's config names was switched off
// (security review, 11 Nov 2026). The outer repo can also say to look
// (`submodule.<name>.ignore none`), which beats -c diff.ignoreSubmodules.
test("untrustedGit never looks inside a submodule, whatever the repo's config says", async () => {
  const root = tmpDir();
  const marker = path.join(root, 'ran');
  const payload = path.join(root, 'payload.sh');
  fs.writeFileSync(payload, `#!/bin/sh\necho ran >> '${marker}'\ncat\n`, { mode: 0o755 });
  const env = { ...process.env, GIT_CONFIG_GLOBAL: '/dev/null', GIT_CONFIG_NOSYSTEM: '1' };
  const g = (dir: string, ...args: string[]) => execFileSync('git', ['-C', dir, '-c', 'user.email=t@t', '-c', 'user.name=t', ...args], { stdio: 'pipe', env }).toString().trim();
  const sub = path.join(root, 'sub-src');
  fs.mkdirSync(sub);
  g(sub, 'init', '-q');
  fs.writeFileSync(path.join(sub, '.gitattributes'), '*.txt filter=sub\n');
  fs.writeFileSync(path.join(sub, 'v.txt'), 'one\n');
  g(sub, 'add', '-A');
  g(sub, 'commit', '-qm', 'v');
  const repo = path.join(root, 'outer');
  fs.mkdirSync(repo);
  g(repo, 'init', '-q');
  fs.writeFileSync(path.join(repo, 'a.txt'), 'a\n');
  g(repo, 'add', '-A');
  g(repo, 'commit', '-qm', 'a');
  g(repo, '-c', 'protocol.file.allow=always', 'submodule', 'add', '-q', sub, 'vendor');
  g(repo, 'commit', '-qm', 'vendor');
  const base = g(repo, 'rev-parse', 'HEAD');
  g(repo, 'config', 'submodule.vendor.ignore', 'none');
  g(repo, 'config', 'submodule.vendor.fetchRecurseSubmodules', 'true');
  g(path.join(repo, 'vendor'), 'config', 'filter.sub.clean', payload);
  const file = path.join(repo, 'vendor', 'v.txt');
  fs.writeFileSync(file, 'two\n');
  const ran = () => fs.existsSync(marker);
  const stale = (n: number) => fs.utimesSync(file, new Date(2020, 0, 1, 0, n), new Date(2020, 0, 1, 0, n));
  stale(0);
  execFileSync('git', ['-C', repo, 'status', '--porcelain'], { stdio: 'pipe', env });
  assert.ok(ran(), 'plain git runs it, so the checks below mean something');
  fs.rmSync(marker);
  const calls: string[][] = [
    ['status', '--porcelain', '--untracked-files=all'],
    ['diff', '--name-only', '-z', '--no-renames', '--no-ext-diff', '--no-textconv', base],
    ['diff', '--quiet', '--no-ext-diff', '--no-textconv', base, '--', 'vendor'],
    ['diff-index', '--name-only', base],
    ['diff-files', '--name-only'],
    // What a move's pack and unpack run in a worktree (Muse's review).
    ['ls-files', '-z', '--others'],
    ['read-tree', 'HEAD'],
    ['reset', '--quiet'],
  ];
  for (const [i, args] of calls.entries()) {
    stale(i + 1);
    await untrustedGit(repo, args, { env: { GIT_CONFIG_GLOBAL: '/dev/null', GIT_CONFIG_NOSYSTEM: '1' } });
    assert.equal(ran(), false, `${args.join(' ')} ran the submodule's filter`);
  }
  // git submodule is git in each submodule, by design: refused outright.
  const r = await untrustedGit(repo, ['submodule', 'foreach', `${payload} </dev/null`]);
  assert.equal(r.ok, false);
  assert.equal(ran(), false);
});

// Muse's review of the fix above: diff.external was never blanked, so a
// caller's --ext-diff (or one smuggled in as a revision) ran the repo's own;
// and anything not looked at (log, show, merge, stash) went through as is.
test('untrustedGit runs no diff.external, and only the commands it has been looked at for', async () => {
  const root = tmpDir();
  const repo = path.join(root, 'r');
  fs.mkdirSync(repo);
  const marker = path.join(root, 'ran');
  const payload = path.join(root, 'ext.sh');
  fs.writeFileSync(payload, `#!/bin/sh\necho ran >> '${marker}'\n`, { mode: 0o755 });
  const env = { ...process.env, GIT_CONFIG_GLOBAL: '/dev/null', GIT_CONFIG_NOSYSTEM: '1' };
  const g = (...args: string[]) => execFileSync('git', ['-C', repo, '-c', 'user.email=t@t', '-c', 'user.name=t', ...args], { stdio: 'pipe', env }).toString().trim();
  g('init', '-q');
  fs.writeFileSync(path.join(repo, 'a.txt'), 'a\n');
  g('add', '-A');
  g('commit', '-qm', 'a');
  g('config', 'diff.external', payload);
  fs.writeFileSync(path.join(repo, 'a.txt'), 'b\n');
  execFileSync('git', ['-C', repo, 'diff'], { stdio: 'pipe', env });
  assert.ok(fs.existsSync(marker), 'plain git runs it, so the checks below mean something');
  fs.rmSync(marker);
  const quiet = { env: { GIT_CONFIG_GLOBAL: '/dev/null', GIT_CONFIG_NOSYSTEM: '1' } };
  // A blank program is none: asked for, git fails rather than run one.
  await untrustedGit(repo, ['diff', '--ext-diff', 'HEAD'], quiet);
  assert.equal(fs.existsSync(marker), false, 'diff --ext-diff');
  const plain = await untrustedGit(repo, ['diff', 'HEAD'], quiet);
  assert.ok(plain.ok && plain.stdout.includes('+b'), 'a plain diff is git\'s own');
  assert.equal(fs.existsSync(marker), false, 'diff');
  for (const args of [['log', '-p'], ['show', 'HEAD'], ['merge', 'HEAD'], ['stash'], ['worktree', 'add', path.join(root, 'w')], ['submodule', 'status'], []]) {
    const out = await untrustedGit(repo, args, quiet);
    assert.equal(out.ok, false, `git ${args.join(' ')} is refused`);
  }
  assert.ok((await untrustedGit(repo, ['worktree', 'list', '--porcelain'], quiet)).ok);
  assert.equal(fs.existsSync(marker), false);
  assert.throws(() => untrustedGitSync(repo, ['diff']), /listings/);
});
