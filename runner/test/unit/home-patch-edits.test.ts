// Home patches (~/.threadbear/patches) load without a hash, so outside Full
// access an edit of anything one runs asks, and when the runner can't tell
// what one runs, every edit asks. Ville's patches (11 Nov 2026) were written
// the way the guide shows, with a URL, a regex, a quoting helper and a
// command built with join(dir, ...), and every edit in every Auto thread
// asked, with nothing saying why. These are patches like his.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { makeHarness, tick, tmpDir } from '../helpers/fakes.ts';
import { PatchTrust } from '../../src/patch-trust.ts';
import { patchCommands, VALUE } from '../../src/patch-commands.ts';
import type { ThreadRecord } from '../../src/store.ts';

function put(file: string, text: string): void {
  fs.mkdirSync(path.dirname(file), { recursive: true });
  fs.writeFileSync(file, text);
  const later = new Date(Date.now() + Math.random() * 1000 + 1000);
  fs.utimesSync(file, later, later);
}

async function settle(h: ReturnType<typeof makeHarness>, threadId: string) {
  await tick();
  await h.runner.patchesIdle();
  h.clock.advance(60_000);
  h.runner.get(threadId);
  await tick();
  await h.runner.patchesIdle();
  return h.runner.get(threadId);
}

// A home folder holding one home patch (and what it's given), and an Auto
// thread in a folder of its own.
async function withHomePatch(patch: string, files: Record<string, string> = {}) {
  const root = fs.realpathSync(tmpDir());
  const patches = path.join(root, '.threadbear', 'patches');
  for (const [f, text] of Object.entries(files)) put(path.join(root, f), text);
  put(path.join(patches, 'p.ts'), `${patch}\n`);
  const cwd = path.join(root, 'work');
  put(path.join(cwd, 'README.md'), 'hi\n');
  const data = tmpDir();
  const h = makeHarness(data, {}, { panelsRoot: root, patchTrust: new PatchTrust({ file: path.join(data, 'patch-trust.json'), claudeHome: root }) });
  const { thread } = h.runner.create({ text: 'hi', accountId: 'claude-3', cwd, permissionMode: 'auto' });
  const st = await settle(h, thread.id);
  const runsAsCode = (file: string) => h.runner.writeRunsAsCode({ ...thread, cwd } as unknown as ThreadRecord, file);
  return { h, root, cwd, thread, st, runsAsCode, patchFile: path.join(patches, 'p.ts') };
}

const DESK = `#!/usr/bin/env node\nconsole.log(JSON.stringify({ open: 2 }));\n`;

test('a home patch written the way the guide shows makes only what it runs ask', async () => {
  const patch = `import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { execFileSync } from 'node:child_process';
type Ctx = { cwd: string; matched: string[]; sh: (command: string) => Promise<string> };
const dir = dirname(fileURLToPath(import.meta.url));
const quote = (s: string) => \`'\${s.replaceAll("'", \`'\\\\''\`)}'\`;
const PR = /^#?(\\d+)$/;
const REPO_URL = 'https://github.com/acme/app/pull/';
async function prs(sh: Ctx['sh'], repo: string) {
  return JSON.parse(await sh(\`gh pr list --repo \${quote(repo)} --json number,title\`));
}
export default {
  icon: 'arrow.triangle.pull',
  actions: [
    { title: 'Desk', prompt: "Look at the desk's findings with \`node desk.mjs\` and fix what's real (don't push).", when: async ({ sh }: Ctx) => (await sh(\`node \${join(dir, 'desk', 'desk.mjs')} status\`)).length > 0 },
    { title: 'Tidy', run: 'git status --short' },
    { title: 'Review', url: 'https://github.com/acme/app/pulls?q=is%3Aopen+review%3Arequired' },
  ],
  panels: [
    {
      title: 'PRs',
      items: async ({ sh, cwd }: Ctx) => (await prs(sh, cwd)).map((p: { number: number; title: string }) => ({ title: p.title.replace(PR, ''), url: REPO_URL + p.number })),
      badge: async () => Number(execFileSync('node', [join(import.meta.dirname, 'desk', 'count.mjs')]).toString()),
    },
  ],
};`;
  const { runsAsCode, cwd, root } = await withHomePatch(patch, { '.threadbear/patches/desk/desk.mjs': DESK, '.threadbear/patches/desk/count.mjs': DESK });
  assert.equal(runsAsCode(path.join(cwd, 'README.md')), false, 'an ordinary edit goes ahead');
  // Under ~/.threadbear they ask by name anyway; what matters is they're seen.
  assert.equal(runsAsCode(path.join(root, '.threadbear/patches/desk/desk.mjs')), true);
});

test('what a home patch runs outside ~/.threadbear still asks, by the path it builds', async () => {
  const tools = fs.realpathSync(tmpDir());
  put(path.join(tools, 'desk.mjs'), `import './lib.mjs';\n`);
  put(path.join(tools, 'lib.mjs'), `console.log(1);\n`);
  put(path.join(tools, 'other.mjs'), `console.log(1);\n`);
  const patch = `import path from 'node:path';
const TOOLS = ${JSON.stringify(tools)};
const script = path.join(TOOLS, 'desk.mjs');
export default {
  actions: [{ title: 'Desk', prompt: 'x', badge: async (ctx: { sh: (c: string) => Promise<string> }) => Number(await ctx.sh(['node', script, '--count'].join(' '))) }],
};`;
  const { runsAsCode, cwd } = await withHomePatch(patch);
  assert.equal(runsAsCode(path.join(tools, 'desk.mjs')), true, 'the script it runs');
  assert.equal(runsAsCode(path.join(tools, 'lib.mjs')), true, 'and what that imports');
  assert.equal(runsAsCode(path.join(tools, 'other.mjs')), false);
  assert.equal(runsAsCode(path.join(cwd, 'README.md')), false);
});

test("a home patch that runs something it can't follow still makes every edit ask, and says which and why, once", async () => {
  const cases: Record<string, string> = {
    'a script picked as it runs': `export default { actions: [{ title: 'A', prompt: 'x', when: async ({ sh, cwd }: any) => !!(await sh(\`node \${cwd}/tool.mjs\`)) }] };`,
    'a program picked as it runs': `const bin = process.env.TOOL; export default { actions: [{ title: 'A', prompt: 'x', when: async ({ sh }: any) => !!(await sh(\`\${bin} --check\`)) }] };`,
    'a command read from a file': `import fs from 'node:fs'; export default { actions: [{ title: 'A', prompt: 'x', when: async ({ sh }: any) => !!(await sh(fs.readFileSync('cmd.txt', 'utf8'))) }] };`,
    'sh -c with code worked out': `import { execFileSync } from 'node:child_process'; const c = process.argv[2]; export default { actions: [{ title: 'A', prompt: 'x', when: () => !!execFileSync('sh', ['-c', c]) }] };`,
    'execSync of a variable': `import * as cp from 'node:child_process'; export default { actions: [{ title: 'A', prompt: 'x', when: (ctx: any) => !!cp.execSync(ctx.cmd) }] };`,
    'promisified exec': `import { exec } from 'node:child_process'; import { promisify } from 'node:util'; const run = promisify(exec); export default { actions: [{ title: 'A', prompt: 'x', when: async (ctx: any) => !!(await run(ctx.cwd + '/go.sh')) }] };`,
    'sh handed to something it can\'t see': `import { list } from '../lib/lib.ts'; export default { panels: [{ title: 'P', items: (ctx: any) => list(ctx.sh) }] };`,
    'a run read back from an object': `const c = { title: 'node ./x.mjs' }; export default { hooks: [{ title: 'H', on: 'turn.end', run: c.title }] };`,
    'a module that could run anything, with a string that can\'t be followed': `import cluster from 'node:cluster'; const c = 'node '; export default { actions: [{ title: 'A', prompt: 'x', when: () => cluster.isPrimary && c === 'x' }] };`,
    'a tagged sh': `export default { actions: [{ title: 'A', prompt: 'x', when: async ({ sh }: any) => !!(await sh\`node x.mjs\`) }] };`,
    'a run built by a helper': `const mk = (c: string) => ({ title: 'T', run: c }); export default { actions: [mk(process.env.X ?? 'true')] };`,
  };
  for (const [name, patch] of Object.entries(cases)) {
    const { h, thread, runsAsCode, cwd } = await withHomePatch(patch, { '.threadbear/lib/lib.ts': `export const list = (sh: any) => sh(process.env.X);\n` });
    assert.equal(runsAsCode(path.join(cwd, 'README.md')), 'unsure', name);
    assert.equal(runsAsCode(path.join(cwd, 'other.md')), 'unsure', name);
    const notes = h.events(thread.id).filter((e) => e.type === 'system.note' && String(e.text).includes('ask before every file edit'));
    assert.equal(notes.length, 1, `${name}: said once`);
    assert.match(notes[0].text, /p\.ts/, name);
    assert.doesNotMatch(notes[0].text, /__threadbear_value/, name);
    // And on the patch in Patches.
    h.clock.advance(60_000);
    const st = await settle(h, thread.id);
    const info = st.patches?.find((p: { file?: string }) => p.file?.endsWith('p.ts'));
    assert.match(info?.editsAsk ?? '', /ask before every file edit/, name);
  }
});

test('a home patch the scan reads plainly has nothing on it in Patches', async () => {
  const { thread, h } = await withHomePatch(`export default { actions: [{ title: 'Tidy', run: 'git status --short', note: "Don't stop; it's (only) a thought" }] };`);
  const st = await settle(h, thread.id);
  const info = st.patches?.find((p: { file?: string }) => p.file?.endsWith('p.ts'));
  assert.ok(info);
  assert.equal(info!.editsAsk, undefined);
});

test('patchCommands: what each sink runs, worked out', () => {
  const read = (src: string, file = '/x/.threadbear/patches/p.ts') => {
    const r = patchCommands(src, file, true);
    assert.ok('commands' in r, JSON.stringify(r));
    return r.commands.map((c) => c.text);
  };
  assert.deepEqual(read(`const q = (s: string) => \`'\${s}'\`; export default { panels: [{ title: 'P', items: async ({ sh }: any) => sh(\`gh pr view \${q('a b')}\`) }] };`), ["gh pr view 'a b'"]);
  assert.deepEqual(read(`import { join } from 'node:path'; export default { actions: [{ title: 'A', prompt: 'p', when: ({ sh }: any) => sh(\`node \${join(import.meta.dirname, 'x.mjs')} \${Date.now()}\`) }] };`), [`node /x/.threadbear/patches/x.mjs ${VALUE}`]);
  assert.deepEqual(read(`export default { actions: [{ title: 'A', run: process.env.CI ? './a.sh' : './b.sh' }] };`), ['./a.sh', './b.sh']);
  assert.deepEqual(read(`import { execFileSync } from 'node:child_process'; execFileSync('git', ['log', '-1', process.env.X!]);`), [`cd '${process.cwd()}' && 'git' 'log' '-1' "${VALUE}"`]);
  assert.deepEqual(read(`import { spawn } from 'node:child_process'; spawn('ls -la | wc', { shell: true });`), [`cd '${process.cwd()}' && ls -la | wc`]);
  assert.deepEqual(read(`export default { worktree: { setup: './setup.sh', teardown: './down.sh' } };`).length, 2);
  const wt = patchCommands(`export default { worktree: { setup: './setup.sh', teardown: './down.sh' } };`, '/x/p.ts', true);
  assert.ok('commands' in wt);
  assert.deepEqual(wt.commands.map((c) => [c.text, !!c.worktree]), [['./setup.sh', true], ['./down.sh', false]], 'teardown runs in the worktree as the agent left it');
  // Things it won't guess at.
  for (const src of [
    `const f = (ctx: any) => { const s = ctx.sh; return s('x'); };`,
    `import cp from 'node:child_process'; const { execSync } = cp;`,
    `const k = 'run'; export default { hooks: [{ [k]: 'x' }] };`,
    `export default { hooks: [{ title: 'H', get run() { return 'x'; } }] };`,
    `const m = await import('node:child_process');`,
    `import x from 'execa';`,
    `const cp = require('child_process');`,
  ]) assert.ok('unknown' in patchCommands(src, '/x/p.ts', true), src);
});

test("patchCommands: what it won't reason about, and a file built to take forever doesn't", () => {
  for (const src of [
    `export { execSync } from 'node:child_process';`,
    `export * from 'child_process';`,
    `process.binding('spawn_sync');`,
    `process.execve('/bin/sh', ['sh']);`,
  ]) assert.ok('unknown' in patchCommands(src, '/x/p.ts', true), src);
  // 2^40 ways to work it out without each constant being kept.
  const chain = Array.from({ length: 40 }, (_, i) => `const a${i + 1} = a${i} + a${i};`).join('\n');
  const started = Date.now();
  const r = patchCommands(`const a0 = 'x';\n${chain}\nexport default { actions: [{ title: 'A', run: a40 }] };`, '/x/p.ts', true);
  assert.ok(Date.now() - started < 2000, 'quick');
  assert.ok('commands' in r);
});

// Codex's review of the fix (11 Nov): each of these ran a file the scan
// didn't see, or hid the reason. Each must ask about that file, or ask about
// every edit.
test("what Codex's review found: each still asks", async () => {
  const outside = fs.realpathSync(tmpDir());
  put(path.join(outside, 'go.mjs'), 'console.log(1);\n');
  const cases: Record<string, { patch: string; file: string }> = {
    'too many ways for a path call': {
      patch: `import { relative } from 'node:path';
export default { actions: [{ title: 'Run', run: 'node ${path.join(outside, 'go.mjs')}' }], panels: [{ title: 'P', items: ({ sh, a, b, c, d, e, f, g }: any) => sh('node ' + relative(a ? '/a' : b ? '/b' : c ? '/c' : '/d', d ? '/e' : e ? '/f' : f ? '/g' : g ? '/h' : '/i')) }] };`,
      file: path.join(outside, 'go.mjs'),
    },
    'a cwd in the options': { patch: `import { execFileSync } from 'node:child_process'; export default { panels: [{ title: 'P', items: () => execFileSync('node', ['go.mjs'], { cwd: ${JSON.stringify(outside)} }) }] };`, file: path.join(outside, 'go.mjs') },
    'a path through ..': { patch: `import { join } from 'node:path'; export default { panels: [{ title: 'P', items: ({ sh, cwd }: any) => sh(join(cwd, '..', 'status.sh')) }] };`, file: path.join(outside, 'status.sh') },
    'a script outside that isn\'t there yet': { patch: `import { join } from 'node:path'; export default { panels: [{ title: 'P', items: ({ sh }: any) => sh('node ' + join(${JSON.stringify(path.join(outside, 'later'))}, 'go.mjs')) }] };`, file: path.join(outside, 'later', 'go.mjs') },
    'a parameter named like an import': { patch: `import { join } from 'node:path'; function command(join: any) { return 'node ' + join('/safe', 'tool.mjs'); } export default { panels: [{ title: 'P', items: ({ sh }: any) => sh(command((a: string, b: string) => '/actual/' + b)) }] };`, file: path.join(outside, 'x.md') },
    'numbers added': { patch: `export default { panels: [{ title: 'P', items: ({ sh }: any) => sh('node ${outside}/tool' + (1 + 2) + '.mjs') }] };`, file: path.join(outside, 'tool3.mjs') },
    'sh in an object': { patch: `export default { panels: [{ title: 'P', items: ({ sh }: any) => { const cbs = { sh }; for (const fn of Object.values(cbs)) fn('node ' + process.env.SCRIPT); return []; } }] };`, file: path.join(outside, 'x.md') },
  };
  for (const [name, c] of Object.entries(cases)) {
    const { runsAsCode } = await withHomePatch(c.patch);
    assert.ok(runsAsCode(c.file), name);
  }
});

test('the note leaves out what could be a credential in the command', async () => {
  const { h, thread, runsAsCode, cwd } = await withHomePatch(`export default { panels: [{ title: 'P', items: ({ sh }: any) => sh('node ' + process.env.SCRIPT + ' --token SECRET_TOKEN_abc') }] };`);
  assert.equal(runsAsCode(path.join(cwd, 'README.md')), 'unsure');
  const note = h.events(thread.id).find((e) => e.type === 'system.note' && String(e.text).includes('ask before every file edit'));
  assert.ok(note);
  assert.doesNotMatch(note.text, /SECRET/);
});

test("what Codex's second review found: each still asks", async () => {
  const outside = fs.realpathSync(tmpDir());
  put(path.join(outside, 'go.mjs'), `import './lib.mjs';\n`);
  put(path.join(outside, 'lib.mjs'), 'console.log(1);\n');
  const go = path.join(outside, 'go.mjs');
  const cases: Record<string, { patch: string; file: string; files?: Record<string, string> }> = {
    'child_process handed on by another file': {
      patch: `import { run } from '../lib/process.ts'; import { join } from 'node:path'; export default { panels: [{ title: 'P', items: () => run('node ' + join(${JSON.stringify(outside)}, 'go.mjs')) }] };`,
      files: { '.threadbear/lib/process.ts': `export { execSync as run } from 'node:child_process';\n` },
      file: path.join(outside, 'lib.mjs'),
    },
    'hooks from JSON': { patch: `const hooks = JSON.parse('[{"title":"After","on":"turn.end","run":"node ${go}"}]'); export default { hooks };`, file: path.join(outside, 'lib.mjs') },
    'a helper written over': { patch: `function script() { return '/opt/linux/' + 'status.mjs'; } if (process.platform === 'darwin') script = () => '${outside}/' + 'mac.mjs'; export default { actions: [{ title: 'Run', run: 'node ' + script() }] };`, file: path.join(outside, 'mac.mjs') },
    'null in a join': { patch: `export default { panels: [{ title: 'P', items: ({ sh }: any) => sh('node ' + [${JSON.stringify(outside)}, null, 'go.mjs'].join('/')) }] };`, file: path.join(outside, 'lib.mjs') },
    'a relative script outside, not there yet': { patch: `import { join } from 'node:path'; export default { panels: [{ title: 'P', items: ({ sh }: any) => sh('node ' + join('..', 'later', 'go.mjs')) }] };`, file: '../later/go.mjs' },
    "child_process from the runner's own folder": { patch: `import { execFileSync } from 'node:child_process'; import { join } from 'node:path'; export default { panels: [{ title: 'P', items: () => execFileSync('node', [join('tools', 'go.mjs')]) }] };`, file: path.join(process.cwd(), 'tools', 'go.mjs') },
  };
  for (const [name, c] of Object.entries(cases)) {
    const { runsAsCode, cwd } = await withHomePatch(c.patch, c.files);
    assert.ok(runsAsCode(c.file.startsWith('..') ? path.resolve(cwd, c.file) : c.file), name);
  }
});

test("the note leaves out a URL's password", async () => {
  const { h, thread, runsAsCode, cwd } = await withHomePatch(`export default { panels: [{ title: 'P', items: ({ sh }: any) => sh('node ' + process.env.SCRIPT + ' https://alice:p4ss@example.test/') }] };`);
  assert.equal(runsAsCode(path.join(cwd, 'README.md')), 'unsure');
  const note = h.events(thread.id).find((e) => e.type === 'system.note' && String(e.text).includes('ask before every file edit'));
  assert.doesNotMatch(note!.text, /p4ss/);
});

test("what Codex's third review found: each still asks", async () => {
  const outside = fs.realpathSync(tmpDir());
  put(path.join(outside, 'status.mjs'), `import './lib.mjs';\n`);
  put(path.join(outside, 'lib.mjs'), 'console.log(1);\n');
  put(path.join(outside, 'hooks.json'), JSON.stringify([{ title: 'After', on: 'turn.end', run: `node ${outside}/status.mjs` }]));
  const lib = path.join(outside, 'lib.mjs');
  const cases: Record<string, { patch: string; file: string }> = {
    'hooks pushed from JSON': { patch: `import { readFileSync } from 'node:fs'; const hooks: any[] = []; hooks.push(...JSON.parse(readFileSync(${JSON.stringify(path.join(outside, 'hooks.json'))}, 'utf8'))); export default { hooks };`, file: lib },
    'a whole command line with shell: true': { patch: `import { spawnSync } from 'node:child_process'; import { join } from 'node:path'; const script = join(${JSON.stringify(outside)}, 'status.mjs'); export default { panels: [{ title: 'S', items: () => spawnSync("node '" + script + "'", { shell: true }) }] };`, file: lib },
    'null through a constant': { patch: `const part = null; export default { panels: [{ title: 'S', items: ({ sh }: any) => sh('node ' + [${JSON.stringify(outside)}, part, 'status.mjs'].join('/')) }] };`, file: lib },
    'a quoted path with a space, not there yet': { patch: `import { join } from 'node:path'; export default { panels: [{ title: 'S', items: ({ sh }: any) => sh(\`node "\${join(${JSON.stringify(path.join(outside, 'My Tools'))}, 'later.mjs')}"\`) }] };`, file: path.join(outside, 'My Tools', 'later.mjs') },
  };
  for (const [name, c] of Object.entries(cases)) {
    const { runsAsCode } = await withHomePatch(c.patch);
    assert.ok(runsAsCode(c.file), name);
  }
});

test("the note leaves out a URL's password when the file is read the old way", async () => {
  const { h, thread, runsAsCode, cwd } = await withHomePatch(`import cluster from 'node:cluster'; const endpoint = 'https://alice:p4ss@example.test/api?status=open'; export default { panels: [{ title: 'S', note: String(cluster.isPrimary), url: endpoint }] };`);
  assert.equal(runsAsCode(path.join(cwd, 'README.md')), 'unsure');
  const note = h.events(thread.id).find((e) => e.type === 'system.note' && String(e.text).includes('ask before every file edit'));
  assert.ok(note);
  assert.doesNotMatch(note.text, /p4ss/);
});
