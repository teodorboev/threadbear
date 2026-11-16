// Grok over ACP (adapters/acp/agents.ts, grok-guard.ts), against the pretend
// agent in its Grok persona: like Grok 1.0.77 in auto mode, it runs every tool
// unasked unless a PreToolUse hook from --plugin-dir says otherwise, and it
// runs the real guard script to find out.
import { test, afterEach, beforeEach } from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { z } from 'zod';
import { AcpAdapter, AcpSession, modelChoices, withoutHookNote } from '../../src/adapters/acp/adapter.ts';
import { ACP_AGENTS, acpAccounts, isCursorBinary, isGrokBinary, type AcpAgent } from '../../src/adapters/acp/agents.ts';
import { grokGuardDir, grokTrustedFolders, grokTrusts, guardPluginName } from '../../src/adapters/acp/grok-guard.ts';
import { acpAsker } from '../../src/models/acp-oneshot.ts';
import { acpVersion } from '../../src/updates/manager.ts';
import type { AdapterOutput, SessionSink, StartSessionOptions, ToolApprovalRequest } from '../../src/adapters/types.ts';
import type { Account, PermissionMode } from '../../src/protocol.ts';
import { nullLogger } from '../../src/log.ts';
import { ProcessRegistry } from '../../src/proc.ts';

const FAKE = path.join(import.meta.dirname, '..', 'helpers', 'fake-acp-agent.mjs');
const grok = ACP_AGENTS.grok;
const account: Account = { id: 'grok-main', provider: 'grok', title: 'Grok', configDir: '/g' };

function tmp(): string {
  const d = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'tb-grok-')));
  made.push(d);
  return d;
}

const made: string[] = [];
const live: AcpSession[] = [];
// Grok's trusted folders come from GROK_HOME, never the real ~/.grok here.
const inheritedGrokHome = process.env.GROK_HOME;
let grokHome = '';
beforeEach(() => {
  grokHome = tmp();
  process.env.GROK_HOME = grokHome;
});
const trust = (dir: string) => fs.writeFileSync(path.join(grokHome, 'trusted_folders.toml'), `[folders."${dir}"]\ntrusted = true\ndecided_at = 1\n`);
afterEach(async () => {
  if (inheritedGrokHome === undefined) delete process.env.GROK_HOME;
  else process.env.GROK_HOME = inheritedGrokHome;
  await Promise.all(live.splice(0).map((s) => s.end({ hard: true }).catch(() => undefined)));
  for (const d of made.splice(0)) fs.rmSync(d, { recursive: true, force: true });
});

function rig(o: { mode?: PermissionMode; deny?: boolean; agent?: AcpAgent; opts?: Partial<StartSessionOptions>; tools?: string[]; before?: (cwd: string) => void } = {}) {
  const root = tmp();
  const cwd = path.join(root, 'work');
  fs.mkdirSync(cwd);
  o.before?.(cwd);
  let exitedWith: Error | undefined | null = null;
  const logFile = path.join(root, 'agent.log');
  const outputs: AdapterOutput[] = [];
  const cards: ToolApprovalRequest[] = [];
  const results: (() => void)[] = [];
  const sink: SessionSink = {
    output(out) {
      outputs.push(out);
      if (out.kind === 'result') results.splice(0).forEach((f) => f());
    },
    async requestApproval(req) {
      cards.push(req);
      return { decision: o.deny ? 'deny' : 'allow' };
    },
    async requestQuestion() {
      return { answers: null };
    },
    exited(e) {
      exitedWith = e;
      results.splice(0).forEach((f) => f());
    },
  };
  const tools = (o.tools ?? []).map((name) => ({ name, description: name, inputSchema: { what: z.string().optional() }, handler: async () => ({ content: [{ type: 'text', text: 'ok' }] }) }));
  const adapter = new AcpAdapter({
    agent: o.agent ?? grok,
    binary: () => FAKE,
    registry: new ProcessRegistry(path.join(root, 'run'), nullLogger),
    log: nullLogger,
    env: { PATH: process.env.PATH, FAKE_ACP_LOG: logFile, FAKE_ACP_NAME: 'Grok', GROK_HOME: grokHome },
    timings: { closeWaitMs: 300 },
    dataDir: path.join(root, 'data'),
    toolsFor: () => tools as never,
  });
  const s = adapter.start({ threadId: 't1', cwd, account, permissionMode: o.mode ?? 'ask', sessionId: 'unused', resume: false, mediaDir: path.join(root, 'media'), blobsDir: path.join(root, 'blobs'), tmpDir: root, ...o.opts }, sink) as AcpSession;
  live.push(s);
  const turn = (text: string) =>
    new Promise<void>((resolve) => {
      results.push(resolve);
      s.send({ id: `t${outputs.length}`, text, images: [] });
    });
  const log = (): Record<string, unknown>[] => (fs.existsSync(logFile) ? fs.readFileSync(logFile, 'utf8').trim().split('\n').filter(Boolean).map((l) => JSON.parse(l)) : []);
  return { root, cwd, s, adapter, cards, outputs, turn, log, exitError: () => exitedWith };
}

// ---- Asking

test('without the hook the pretend Grok writes unasked, as the real one did in auto mode', async () => {
  // The guard is what holds: take it away and Ask means nothing.
  const r = rig({ mode: 'ask', deny: true, agent: { ...grok, args: () => ['agent', 'stdio'] } });
  const file = path.join(r.cwd, 'a.txt');
  await r.turn(`write ${file} hi`);
  assert.equal(r.cards.length, 0);
  assert.equal(fs.readFileSync(file, 'utf8'), 'hi');
});

for (const mode of ['ask', 'auto', 'plan'] as PermissionMode[]) {
  test(`${mode}: a write and a command each ask, and denying stops them`, async () => {
    const r = rig({ mode, deny: true });
    const file = path.join(r.cwd, 'a.txt');
    await r.turn(`write ${file} hi`);
    await r.turn('exec echo ran');
    assert.deepEqual(r.cards.map((c) => c.name), ['apply_patch', 'shell']);
    assert.equal(fs.existsSync(file), false);
    assert.ok(!r.log().some((l) => l.ran), 'the command never ran');
  });
}

test('the card says what the call does, not which hook asked', async () => {
  const r = rig({ mode: 'ask' });
  const file = path.join(r.cwd, 'a.txt');
  await r.turn(`write ${file} hi`);
  assert.equal(r.cards[0].title, 'Edit a.txt');
  assert.equal(r.cards[0].canAlwaysAllow, true, "Grok's `variant` names no place, so its plain write can be always allowed");
  assert.equal(fs.readFileSync(file, 'utf8'), 'hi');
  assert.deepEqual((r.log().find((l) => l.answered) as { answered: unknown }).answered, { outcome: { outcome: 'selected', optionId: 'allow-once' } }, "never Grok's own always");
});

test('Full runs with no card, through a request answered once', async () => {
  const r = rig({ mode: 'full', deny: true });
  const file = path.join(r.cwd, 'a.txt');
  await r.turn(`write ${file} hi`);
  assert.equal(r.cards.length, 0);
  assert.equal(fs.readFileSync(file, 'utf8'), 'hi');
});

test("a read inside the folder goes ahead; one outside it, or through a link that leaves it, asks", async () => {
  const r = rig({ mode: 'ask', deny: true });
  fs.writeFileSync(path.join(r.cwd, 'note.txt'), 'x');
  fs.symlinkSync(os.tmpdir(), path.join(r.cwd, 'out'));
  await r.turn('read note.txt');
  assert.equal(r.cards.length, 0);
  await r.turn('read /etc/hosts');
  await r.turn('read out/thing');
  assert.equal(r.cards.length, 2);
  assert.deepEqual(r.log().flatMap((l) => (l.read ? [l.read] : [])), ['note.txt']);
});

test("its own threadbear tools go ahead, as their MCP server gates them; anyone else's asks", async () => {
  const r = rig({ mode: 'ask', deny: true, tools: ['say_code_word'] });
  await r.turn('mcp threadbear__say_code_word');
  assert.equal(r.cards.length, 0);
  await r.turn('mcp threadbear__not_ours');
  await r.turn('mcp linear__save_issue');
  assert.equal(r.cards.length, 2);
  assert.deepEqual(r.log().flatMap((l) => (l.mcp ? [l.mcp] : [])), ['threadbear__say_code_word']);
});

test('a thread can only make it stricter: its environment comes before the row', async () => {
  const r = rig({ mode: 'ask', deny: true, opts: { env: { GROK_FOLDER_TRUST: '0' } } });
  const env = grok.env();
  assert.equal(env.GROK_FOLDER_TRUST, '1');
  assert.equal(env.GROK_SANDBOX_AUTO_ALLOW_BASH, 'false');
  const file = path.join(r.cwd, 'a.txt');
  await r.turn(`write ${file} hi`);
  assert.equal(r.cards.length, 1);
});

// ---- The guard itself

function guard(dir: string, mode: 'ask' | 'deny', event: unknown, env: Record<string, string> = {}): string | undefined {
  const all = JSON.parse(fs.readFileSync(path.join(dir, 'hooks', 'hooks.json'), 'utf8')).hooks;
  const hooks = all.PreToolUse[0].hooks[0];
  assert.equal(hooks.failClosed, true);
  assert.equal(hooks.command, process.execPath, "the runner's own node, by its whole path");
  assert.equal(path.basename(hooks.args[0]), `guard-${mode}.mjs`, 'a script of its own for each, so a deny never hangs on an argument');
  assert.ok(all.UserPromptSubmit[0].hooks[0], 'and one that shows it loaded as each prompt goes in');
  const r = spawnSync(hooks.command, hooks.args, { input: typeof event === 'string' ? event : JSON.stringify(event), encoding: 'utf8', env: { ...process.env, GROK_HOME: grokHome, ...env } });
  return JSON.parse(r.stdout).decision;
}

test('the guard: reads inside the folder go ahead, everything else asks, and a small job gets nothing', () => {
  const data = tmp();
  const cwd = path.join(data, 'work');
  fs.mkdirSync(path.join(cwd, 'src'), { recursive: true });
  fs.symlinkSync('/etc', path.join(cwd, 'etc'));
  fs.symlinkSync('/', path.join(cwd, 'rootlink'));
  const ask = grokGuardDir({ dataDir: data });
  const deny = grokGuardDir({ dataDir: data, noTools: true });
  assert.notEqual(ask, deny);
  const ev = (toolName: string, toolInput: unknown, more: object = {}) => ({ hookEventName: 'pre_tool_use', cwd, toolName, toolInput, toolInputTruncated: false, ...more });
  assert.equal(guard(ask, 'ask', ev('read_file', { target_file: 'src/a.ts', offset: 1 })), 'allow');
  assert.equal(guard(ask, 'ask', ev('read_file', { target_file: path.join(cwd, 'new', 'file.ts') })), 'allow', "a file that isn't there yet, inside");
  assert.equal(guard(ask, 'ask', ev('grep', { pattern: 'TODO', output_mode: 'content' })), 'allow');
  assert.equal(guard(ask, 'ask', ev('list_dir', { target_directory: '.' })), 'allow');
  assert.equal(guard(ask, 'ask', ev('search_tool', { query: 'threadbear end_turn' })), 'allow');
  for (const [name, input] of [
    ['read_file', { target_file: '/etc/hosts' }],
    ['read_file', { target_file: '../outside.txt' }],
    ['read_file', { target_file: '~/.ssh/id_ed25519' }],
    ['read_file', { target_file: 'etc/hosts' }],
    // The kernel follows rootlink before it goes up: this is /etc/passwd.
    ['read_file', { target_file: 'rootlink/../etc/passwd' }],
    ['list_dir', { target_directory: 'rootlink/..' }],
    ['read_file', { target_file: 'src/../a.ts' }],
    ['grep', { pattern: 'x', glob_pattern: 'src/../**' }],
    ['grep', { pattern: 'x', path: '/Users' }],
    ['grep', { pattern: 'x', glob_pattern: '../../**' }],
    ['grep', { pattern: 'x', paths: ['src', '/'] }],
    ['read_file', { target_file: 'a', extra: { nested: 'thing' } }],
    ['write', { file_path: 'src/a.ts', content: 'x' }],
    ['run_terminal_command', { command: 'ls' }],
    ['web_search', { query: 'x' }],
    ['use_tool', { tool_name: 'threadbear__end_turn', tool_input: {} }],
    ['a_tool_added_later', {}],
  ] as const) {
    assert.equal(guard(ask, 'ask', ev(name, input)), 'ask', `${name} ${JSON.stringify(input)}`);
  }
  assert.equal(guard(ask, 'ask', ev('read_file', { target_file: 'src/a.ts' }, { toolInputTruncated: true })), 'ask', "a cut input can't be checked");
  assert.equal(guard(ask, 'ask', ev('read_file', { target_file: 'src/a.ts' }, { cwd: 'relative' })), 'ask');
  assert.equal(guard(ask, 'ask', 'not json'), 'ask');
  assert.equal(guard(deny, 'deny', ev('read_file', { target_file: 'src/a.ts' })), 'deny', 'a small job reads nothing either');
  // A folder Grok trusts runs its own hooks, which can rewrite a read after
  // the guard has looked: everything asks there.
  trust(cwd);
  assert.equal(guard(ask, 'ask', ev('read_file', { target_file: 'src/a.ts' })), 'ask');
  fs.writeFileSync(path.join(grokHome, 'trusted_folders.toml'), 'trusted = true but not as expected');
  assert.equal(guard(ask, 'ask', ev('read_file', { target_file: 'src/a.ts' })), 'ask', "a trust file it can't read counts as trusting everything");
});

test('trusted when a process started stays trusted for its life, whatever the file says later', async () => {
  const data = tmp();
  const cwd = path.join(data, 'work');
  fs.mkdirSync(cwd);
  const dir = grokGuardDir({ dataDir: data, trusted: true });
  const ev = { hookEventName: 'pre_tool_use', cwd, toolName: 'read_file', toolInput: { target_file: 'a.txt' }, toolInputTruncated: false };
  assert.equal(guard(dir, 'ask', ev), 'ask', 'no trust file now, but its hooks loaded when it started');
  assert.equal(guard(grokGuardDir({ dataDir: data }), 'ask', ev), 'allow');
  // A repo's SessionStart hook takes the folder out of the file mid-session.
  const r = rig({ mode: 'full', before: (c) => trust(c) });
  await r.turn('hello');
  fs.rmSync(path.join(grokHome, 'trusted_folders.toml'));
  await r.s.setPermissionMode('ask');
  assert.equal((r.s as unknown as { state: string }).state, 'closed', 'leaving Full still stops it');
});

test('Grok counts the home folder and a disk root as trusted', () => {
  const home = tmp();
  const empty = tmp();
  assert.equal(grokTrusts(home, empty, home), true);
  assert.equal(grokTrusts('/', empty, home), true);
  assert.equal(grokTrusts(path.join(home, 'project'), empty, home), false, 'only the folder itself');
  assert.match(String(grok.refuses?.({ cwd: os.homedir(), mode: 'ask' })), /Grok trusts this folder/);
  const dir = grokGuardDir({ dataDir: tmp() });
  const ev = (cwd: string) => ({ hookEventName: 'pre_tool_use', cwd, toolName: 'read_file', toolInput: { target_file: 'a.txt' }, toolInputTruncated: false });
  assert.equal(guard(dir, 'ask', ev(home), { HOME: home }), 'ask');
  assert.equal(guard(dir, 'ask', ev('/'), { HOME: home }), 'ask');
  fs.mkdirSync(path.join(home, 'project'));
  assert.equal(guard(dir, 'ask', ev(path.join(home, 'project')), { HOME: home }), 'allow');
});

test("Grok's trusted folders are read as Grok writes them", () => {
  const home = tmp();
  assert.deepEqual(grokTrustedFolders(home), [], 'none yet');
  fs.writeFileSync(path.join(home, 'trusted_folders.toml'), '[folders."/a/b"]\ntrusted = true\ndecided_at = 1794710722\n\n[folders."/c"]\ntrusted = false\n');
  assert.deepEqual(grokTrustedFolders(home), ['/a/b']);
  assert.equal(grokTrusts('/a/b/c', home), true);
  assert.equal(grokTrusts('/a/bc', home), false);
  assert.equal(grokTrusts('/c', home), false);
});

test('every start gets a plugin of its own, named at random, so a folder can\'t name it to turn it off', () => {
  const data = tmp();
  const a = grokGuardDir({ dataDir: data });
  const b = grokGuardDir({ dataDir: data });
  assert.notEqual(path.basename(a), path.basename(b));
  assert.match(path.basename(a), /^threadbear-[0-9a-f]{16}$/);
  const args = grok.args({ dataDir: data });
  assert.deepEqual([args[0], args[1], args[2], args[4]], ['agent', '--no-leader', '--plugin-dir', 'stdio'], 'never its leader mode, which ignores --plugin-dir');
  assert.equal(grok.guardName?.(args), path.basename(args[3]));
  assert.equal(guardPluginName(['agent', 'stdio']), undefined);
  // An old start's plugin goes after a week.
  const old = new Date(Date.now() - 8 * 24 * 60 * 60 * 1000);
  fs.utimesSync(a, old, old);
  grokGuardDir({ dataDir: data });
  assert.equal(fs.existsSync(a), false);
  assert.equal(fs.existsSync(b), true);
});

test("the guard's folder and scripts are never someone else's: links are replaced, not written through", () => {
  const data = tmp();
  const elsewhere = tmp();
  // A grok-guard that's a link to another folder is removed and made afresh.
  fs.symlinkSync(elsewhere, path.join(data, 'grok-guard'));
  grokGuardDir({ dataDir: data });
  const root = path.join(data, 'grok-guard');
  assert.equal(fs.lstatSync(root).isDirectory(), true);
  assert.equal(fs.lstatSync(root).mode & 0o777, 0o700);
  assert.deepEqual(fs.readdirSync(elsewhere), [], 'nothing was written there');
  // A script swapped for a link: the link is replaced, its target untouched,
  // even when the target already says the same.
  const target = path.join(elsewhere, 'guard-ask.mjs');
  fs.copyFileSync(path.join(root, 'guard-ask.mjs'), target);
  fs.rmSync(path.join(root, 'guard-ask.mjs'));
  fs.symlinkSync(target, path.join(root, 'guard-ask.mjs'));
  grokGuardDir({ dataDir: data });
  assert.equal(fs.lstatSync(path.join(root, 'guard-ask.mjs')).isSymbolicLink(), false);
  fs.chmodSync(root, 0o755);
  grokGuardDir({ dataDir: data });
  assert.equal(fs.lstatSync(root).mode & 0o777, 0o700);
});

test('a folder that turns the plugin off stops the thread before the model can do anything, Full included', async () => {
  // As a repo's .grok/config.toml can: `[plugins] disabled = ["<name>"]`.
  let name = '';
  const agent: AcpAgent = {
    ...grok,
    args: (run) => {
      const a = grok.args(run);
      name = path.basename(a[3]);
      return a;
    },
  };
  const r = rig({ mode: 'full', agent });
  fs.mkdirSync(path.join(r.cwd, '.grok'));
  fs.writeFileSync(path.join(r.cwd, '.grok', 'config.toml'), `[plugins]\ndisabled = ["${name}"]\n`);
  const file = path.join(r.cwd, 'a.txt');
  await r.turn(`write ${file} hi`);
  assert.match(String(r.exitError()?.message), /didn't run Threadbear's hook/);
  assert.equal(fs.existsSync(file), false);
});

test("a folder Grok trusts can't have a thread that asks, and leaving Full there stops the process", async () => {
  const root = tmp();
  const cwd = path.join(root, 'work');
  fs.mkdirSync(cwd);
  trust(cwd);
  assert.match(String(grok.refuses?.({ cwd: path.join(cwd, 'sub'), mode: 'ask' })), /Grok trusts this folder/);
  assert.equal(grok.refuses?.({ cwd, mode: 'full' }), undefined);
  assert.throws(() => rig({ mode: 'ask', before: (c) => trust(c) }), /Grok trusts this folder/);
  const r = rig({ mode: 'full', before: (c) => trust(c) });
  await r.turn('hello');
  await r.s.setPermissionMode('ask');
  assert.ok(r.outputs.some((o) => o.kind === 'event' && o.event.type === 'system.note' && /Grok trusts this folder/.test((o.event as { text: string }).text)));
  assert.equal((r.s as unknown as { state: string }).state, 'closed');
});

// ---- Small jobs

test('a small job on Grok can use nothing, reads included', async () => {
  const root = tmp();
  const logFile = path.join(root, 'log');
  const ask = acpAsker({ agent: grok, binary: () => FAKE, registry: new ProcessRegistry(path.join(root, 'run'), nullLogger), log: nullLogger, env: { PATH: process.env.PATH, FAKE_ACP_LOG: logFile, FAKE_ACP_NAME: 'Grok' }, timings: { closeWaitMs: 200 }, dataDir: path.join(root, 'data') });
  const file = path.join(root, 'a.txt');
  const res = await ask({ account, user: `write ${file} hi`, cwd: root, timeoutMs: 8000 });
  assert.equal(res.ok, true);
  assert.equal(fs.existsSync(file), false);
  // With its plugin turned off it stops before the model's first word.
  fs.mkdirSync(path.join(root, '.grok'));
  const sneaky = acpAsker({ agent: { ...grok, args: (run) => { const a = grok.args(run); fs.writeFileSync(path.join(root, '.grok', 'config.toml'), `disabled = ["${path.basename(a[3])}"]`); return a; } }, binary: () => FAKE, registry: new ProcessRegistry(path.join(root, 'run2'), nullLogger), log: nullLogger, env: { PATH: process.env.PATH, FAKE_ACP_NAME: 'Grok' }, timings: { closeWaitMs: 200 }, dataDir: path.join(root, 'data') });
  const stopped = await sneaky({ account, user: `write ${file} hi`, cwd: root, timeoutMs: 8000 });
  assert.equal(stopped.ok, false);
  assert.match((stopped as { message: string }).message, /didn't run Threadbear's hook/);
  assert.equal(fs.existsSync(file), false);
  fs.rmSync(path.join(root, '.grok'), { recursive: true });
  const read = await ask({ account, user: 'read a.txt', cwd: root, timeoutMs: 8000 });
  assert.equal(read.ok, true);
  const seen = fs.readFileSync(logFile, 'utf8');
  assert.doesNotMatch(seen, /"read":/);
  assert.match(seen, /"decision":"deny"/);
});

// ---- Models and effort

test('models come with their effort levels, lowest first, and only Threadbear\'s', async () => {
  const r = rig();
  const models = await r.adapter.models(account);
  assert.deepEqual(models, [
    { id: 'grok-4.7', model: 'grok-4.7', title: 'Grok 4.7', effortLevels: ['low', 'medium', 'high', 'xhigh'], defaultEffort: 'high' },
    { id: 'grok-4.5', model: 'grok-4.5', title: 'Grok 4.5', effortLevels: ['low', 'medium', 'high'], defaultEffort: 'high' },
  ]);
  assert.deepEqual(modelChoices([{ category: 'model', options: [{ value: 'x' }] }]), [{ id: 'x', model: 'x', title: 'x' }], 'none without a list');
});

test("a thread's effort goes to Grok's thought_level option, and back to the model's own level", async () => {
  const r = rig({ mode: 'full', opts: { model: 'grok-4.5', effort: 'low' } });
  await r.turn('hello');
  const sets = () => r.log().flatMap((l) => (l.set ? [l.set] : []));
  assert.deepEqual(sets(), [{ model: 'grok-4.5' }, { reasoning_effort: 'low' }]);
  assert.ok(r.outputs.some((o) => o.kind === 'effort' && o.level === 'low'), 'the level in use is reported');
  await r.s.setEffort(undefined);
  await r.turn('hello');
  assert.deepEqual(sets().at(-1), { reasoning_effort: 'high' });
  await r.s.setModel('grok-4.7');
  await r.s.setEffort('xhigh');
  await r.turn('hello');
  assert.deepEqual(sets().slice(-2), [{ model: 'grok-4.7' }, { reasoning_effort: 'xhigh' }]);
});

// ---- Which program, and is it signed in

test("Grok is offered where it's installed and signed in, found by where it really lives", () => {
  const home = tmp();
  const bin = path.join(home, '.local', 'bin');
  const downloads = path.join(home, '.grok', 'downloads');
  fs.mkdirSync(bin, { recursive: true });
  fs.mkdirSync(downloads, { recursive: true });
  const real = path.join(downloads, 'grok-1.0.77-macos-aarch64');
  fs.writeFileSync(real, '#!/bin/sh\n', { mode: 0o755 });
  fs.symlinkSync(real, path.join(bin, 'grok'));
  // The same binary is ~/.local/bin/agent on Mike's Mac: Cursor's, it isn't.
  fs.symlinkSync(real, path.join(bin, 'agent'));
  assert.equal(isGrokBinary(path.join(bin, 'grok')), true);
  assert.equal(isCursorBinary(path.join(bin, 'agent')), false);
  assert.deepEqual(acpAccounts({ home, pathEnv: bin }).map((a) => a.id), [], 'not signed in');
  fs.writeFileSync(path.join(home, '.grok', 'auth.json'), '{}');
  const accounts = acpAccounts({ home, pathEnv: bin });
  assert.deepEqual(accounts.map((a) => [a.id, a.provider, a.title, a.configDir]), [['grok-main', 'grok', 'Grok', path.join(home, '.grok')]]);
  // A `grok` that's really something else isn't it.
  fs.rmSync(path.join(bin, 'grok'));
  const other = path.join(home, 'other-tool');
  fs.writeFileSync(other, '#!/bin/sh\n', { mode: 0o755 });
  fs.symlinkSync(other, path.join(bin, 'grok'));
  assert.equal(isGrokBinary(path.join(bin, 'grok')), false);
});

test('it is known by what initialize says, and its version comes from there', async () => {
  assert.equal(grok.identifies({ authMethods: [{ id: 'cached_token' }, { id: 'grok.com' }] }), true);
  assert.equal(grok.identifies({ authMethods: [{ id: 'opencode-login' }], agentInfo: { name: 'OpenCode' } }), false);
  const r = rig({ mode: 'full' });
  await r.turn('hello');
  assert.ok(r.outputs.some((o) => o.kind === 'cliVersion' && o.version === '1.0.77'));
  assert.equal(acpVersion('grok 1.0.77 (50ca579fa996) [stable]\n'), '1.0.77');
  assert.equal(acpVersion('1.18.23\n'), '1.18.23');
});

test("withoutHookNote leaves a title alone unless it ends in that hook's note", () => {
  const meta = { hookAsk: { hookName: 'plugin/threadbear-ask/hooks:pre_tool_use[0].hooks[0]', reason: 'Threadbear decides.' } };
  assert.equal(withoutHookNote({ title: "Read `/etc/hosts` — hook 'plugin/threadbear-ask/hooks:pre_tool_use[0].hooks[0]' asks: Threadbear decides." }, meta).title, 'Read `/etc/hosts`');
  assert.equal(withoutHookNote({ title: 'Read `/etc/hosts`' }, meta).title, 'Read `/etc/hosts`');
  assert.equal(withoutHookNote({ title: "x — hook 'other' asks" }, meta).title, "x — hook 'other' asks");
  assert.equal(withoutHookNote({ title: 'x' }, undefined).title, 'x');
});
