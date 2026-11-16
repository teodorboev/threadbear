// Real end-to-end suite for Codex threads: starts the runner on a temp port
// and data dir and drives the real `codex app-server` on account codex-main
// with gpt-6-luna at low effort, tiny prompts and temp working folders.
// Run with `./check.sh --e2e` (TB_E2E_SKIP_CODEX=1 skips it).
//
// It uses Alex's real ~/.codex, since the login lives there, so codexArgs
// switch off everything that writes to that home or starts work by itself
// (memories, chronicle, goals and his six MCP servers). Every Codex thread
// it makes is deleted afterwards with thread/delete, with its
// generated_images folder, so nothing is left in Codex Desktop's list or
// Agent Archive. TB_E2E_CODEX_IMAGEGEN=1 adds the image-generation case.
import { describe, test, before, after } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawn, execFileSync, type ChildProcess } from 'node:child_process';
import { RunnerClient } from '../../src/client.ts';
import { AppServerProcess, fallbackCodexPath } from '../../src/adapters/codex/appserver.ts';
import { ProcessRegistry } from '../../src/proc.ts';
import { nullLogger } from '../../src/log.ts';
import { checkSequence, isTransient } from '../helpers/protocol-check.ts';
import { makePng } from '../helpers/png.ts';

const ACCOUNT = process.env.TB_E2E_CODEX_ACCOUNT ?? 'codex-main';
const MODEL = process.env.TB_E2E_CODEX_MODEL ?? 'gpt-6-luna';
const EFFORT = 'low';
const IMAGEGEN = process.env.TB_E2E_CODEX_IMAGEGEN === '1';
const IDLE_MS = 8000;
const RUNNER_DIR = path.join(import.meta.dirname, '..', '..');
const CODEX_HOME = path.join(os.homedir(), '.codex');
// Alex's MCP servers, by their config.toml table keys.
const MCP_SERVERS = ['openaiDeveloperDocs', 'node_repl', 'computer-use', 'paper', 'agent-archive', 'connectors'];
const CODEX_ARGS = [
  '--disable', 'memories',
  '--disable', 'chronicle',
  '--disable', 'goals',
  ...MCP_SERVERS.flatMap((s) => ['-c', `mcp_servers.${s}.enabled=false`]),
];

type Ev = Record<string, any>;

let root: string;
let home: string;
let work: string;
let runner: ChildProcess;
let client: RunnerClient;
let runnerOutput = '';
const events = new Map<string, Ev[]>();
const summaries = new Map<string, Ev>();
const made: string[] = []; // runner thread ids, for clean-up
const codexIdsMovedAway: string[] = []; // Codex threads a thread left for Claude, for clean-up

async function startRunner(): Promise<void> {
  runnerOutput = '';
  runner = spawn(process.execPath, ['src/main.ts'], {
    cwd: RUNNER_DIR,
    env: {
      ...process.env,
      THREADBEAR_HOME: home,
      THREADBEAR_PORT: '0',
      THREADBEAR_LOG: path.join(root, 'runner.log'),
      THREADBEAR_IDLE_MS: String(IDLE_MS),
      THREADBEAR_DEBUG: '1',
      // Never install a CLI: Codex runs from the npm copy next to Node.
      THREADBEAR_CLI_UPDATES: 'off', THREADBEAR_TOKEN_TALLY: 'off',
    },
    stdio: ['ignore', 'pipe', 'pipe'],
  });
  const port = await new Promise<number>((resolve, reject) => {
    const t = setTimeout(() => reject(new Error(`runner didn't start:\n${runnerOutput}`)), 15_000);
    runner.stdout!.on('data', (d) => {
      runnerOutput += d;
      const m = /listening on (\d+)/.exec(runnerOutput);
      if (m) {
        clearTimeout(t);
        resolve(Number(m[1]));
      }
    });
    runner.stderr!.on('data', (d) => (runnerOutput += d));
    runner.once('exit', (code) => reject(new Error(`runner exited ${code}:\n${runnerOutput}`)));
  });
  const token = fs.readFileSync(path.join(home, 'secrets', 'runner-token'), 'utf8').trim();
  client = await RunnerClient.connect(`ws://127.0.0.1:${port}/rpc`, token);
  client.onNotification((method, params) => {
    if (method === 'thread.event') {
      const list = events.get(params.threadId) ?? [];
      list.push(params.event);
      events.set(params.threadId, list);
    } else if (method === 'thread.updated') {
      summaries.set(params.thread.id, params.thread);
    }
  });
  await client.call('hello', { client: 'e2e-codex', version: '1' });
}

async function stopRunner(): Promise<void> {
  client?.close();
  if (!runner || runner.exitCode !== null) return;
  const exited = new Promise((r) => runner.once('exit', r));
  runner.kill('SIGTERM');
  await exited;
}

async function waitFor<T>(what: string, fn: () => T | undefined | false, timeoutMs = 120_000): Promise<T> {
  const deadline = Date.now() + timeoutMs;
  for (;;) {
    const v = fn();
    if (v) return v;
    if (Date.now() > deadline) throw new Error(`timed out waiting for ${what}`);
    await new Promise((r) => setTimeout(r, 100));
  }
}

function persisted(threadId: string): any[] {
  return (events.get(threadId) ?? []).filter((e) => !isTransient(e));
}

function turnEvents(threadId: string, turnId: string): any[] {
  return persisted(threadId).filter((e) => e.turnId === turnId);
}

function turnDone(threadId: string, turnId: string, timeoutMs = 180_000): Promise<Ev> {
  return waitFor(`turn ${turnId}`, () => persisted(threadId).find((e) => e.type === 'turn.completed' && e.turnId === turnId), timeoutMs);
}

function replyText(threadId: string, turnId: string): string {
  return turnEvents(threadId, turnId)
    .filter((e) => e.type === 'assistant.text')
    .map((e) => e.text)
    .join('\n');
}

function idle(threadId: string): Promise<Ev> {
  return waitFor('idle', () => summaries.get(threadId)?.status === 'idle' && summaries.get(threadId), 60_000);
}

async function createThread(params: Record<string, unknown>): Promise<{ id: string; turnId: string }> {
  const r = await client.call('threads.create', { accountId: ACCOUNT, model: MODEL, effort: EFFORT, cwd: work, ...params });
  made.push(r.thread.id);
  const sub = await client.call('threads.subscribe', { threadId: r.thread.id, afterSeq: 0 });
  const live = events.get(r.thread.id) ?? [];
  const seen = new Set(sub.events.map((e: Ev) => e.seq));
  events.set(r.thread.id, [...sub.events, ...live.filter((e) => isTransient(e) || !seen.has(e.seq))]);
  return { id: r.thread.id, turnId: r.turnId };
}

async function send(threadId: string, text: string, extra: Record<string, unknown> = {}): Promise<string> {
  const { turnId } = await client.call('threads.send', { threadId, text, ...extra });
  return turnId;
}

function assertProtocol(threadId: string): void {
  assert.deepEqual(checkSequence(events.get(threadId) ?? []), [], 'events follow docs/protocol.md');
}

// Processes whose arguments match, e.g. a test's distinctive `sleep 37`.
function running(pattern: string): boolean {
  try {
    return execFileSync('/usr/bin/pgrep', ['-f', pattern], { encoding: 'utf8' }).trim().length > 0;
  } catch {
    return false;
  }
}

// Every Codex thread the runner made for these threads, from its records.
function nativeIds(): string[] {
  const ids = new Set<string>(codexIdsMovedAway);
  for (const id of made) {
    const s = summaries.get(id);
    if (s?.nativeSessionId) ids.add(s.nativeSessionId);
    try {
      const rec = JSON.parse(fs.readFileSync(path.join(home, 'threads', id, 'thread.json'), 'utf8'));
      if (rec.nativeSessionId) ids.add(rec.nativeSessionId);
    } catch {
      // never written
    }
  }
  return [...ids];
}

// thread/delete for each, in a short-lived app-server of our own. An
// archived thread is unarchived first, in case delete doesn't find it.
async function deleteCodexThreads(ids: string[]): Promise<void> {
  if (!ids.length) return;
  const reg = fs.mkdtempSync(path.join(os.tmpdir(), 'tb-e2e-cx-del-'));
  const p = new AppServerProcess({
    binary: fallbackCodexPath(),
    args: CODEX_ARGS,
    account: { id: ACCOUNT, provider: 'codex', title: ACCOUNT, configDir: CODEX_HOME },
    cwd: os.tmpdir(),
    registry: new ProcessRegistry(reg, nullLogger),
    log: nullLogger,
    label: 'e2e-cleanup',
  });
  try {
    await p.start();
    for (const id of ids) {
      try {
        await p.request('thread/delete', { threadId: id }, 30_000);
      } catch (e) {
        const msg = (e as Error).message;
        if (/no rollout found/i.test(msg)) continue;
        try {
          await p.request('thread/unarchive', { threadId: id }, 30_000);
          await p.request('thread/delete', { threadId: id }, 30_000);
        } catch (e2) {
          console.error(`could not delete Codex thread ${id}: ${msg}; ${(e2 as Error).message}`);
        }
      }
      fs.rmSync(path.join(CODEX_HOME, 'generated_images', id), { recursive: true, force: true });
    }
  } finally {
    await p.close();
    fs.rmSync(reg, { recursive: true, force: true });
  }
}

// Files under ~/.codex's session folders that name any of these threads.
function leftovers(ids: string[]): string[] {
  const out: string[] = [];
  const walk = (dir: string) => {
    let names: fs.Dirent[] = [];
    try {
      names = fs.readdirSync(dir, { withFileTypes: true });
    } catch {
      return;
    }
    for (const d of names) {
      const p = path.join(dir, d.name);
      if (d.isDirectory()) walk(p);
      else if (ids.some((id) => d.name.includes(id))) out.push(p);
    }
  };
  walk(path.join(CODEX_HOME, 'sessions'));
  walk(path.join(CODEX_HOME, 'archived_sessions'));
  for (const id of ids) if (fs.existsSync(path.join(CODEX_HOME, 'generated_images', id))) out.push(path.join(CODEX_HOME, 'generated_images', id));
  return out;
}

const hasCodex = fs.existsSync(path.join(CODEX_HOME, 'auth.json')) && fs.existsSync(fallbackCodexPath());

describe('Codex threads end to end (real codex app-server)', { concurrency: false, skip: !hasCodex && 'no Codex login or binary' }, () => {
  before(async () => {
    root = fs.mkdtempSync(path.join('/tmp', 'tb-e2e-codex-'));
    home = path.join(root, 'home');
    work = path.join(root, 'work');
    fs.mkdirSync(home, { recursive: true });
    fs.mkdirSync(work, { recursive: true });
    assert.ok(!home.includes('Application Support/Threadbear'));
    fs.writeFileSync(
      path.join(home, 'runner.json'),
      JSON.stringify({ codexArgs: CODEX_ARGS, defaults: { cwd: work, models: { codex: MODEL }, efforts: { codex: EFFORT } } }, null, 2),
    );
    await startRunner();
  });

  after(async () => {
    await stopRunner();
    const ids = nativeIds();
    await deleteCodexThreads(ids);
    const left = leftovers(ids);
    if (left.length) console.error(`left in ~/.codex after clean-up:\n${left.join('\n')}`);
    else console.log(`deleted ${ids.length} Codex threads; nothing left in sessions, archived_sessions or generated_images`);
    if (process.env.TB_E2E_KEEP) console.log(`kept ${root}`);
    else fs.rmSync(root, { recursive: true, force: true });
  });

  let main: { id: string; turnId: string };

  test('hello lists the Codex account with its models', async () => {
    const hello = await client.call('hello', { client: 'e2e-codex', version: '1' });
    const acct = hello.accounts.find((a: Ev) => a.id === ACCOUNT);
    assert.ok(acct, 'codex-main is listed');
    assert.equal(acct.provider, 'codex');
  });

  test('a text turn', async () => {
    main = await createThread({ text: 'Reply with exactly these words and nothing else: hello from codex' });
    const done = await turnDone(main.id, main.turnId);
    assert.equal(done.outcome, 'completed', done.error);
    assert.equal(done.costUsd, undefined, 'Codex reports no cost');
    assert.match(replyText(main.id, main.turnId).toLowerCase(), /hello from codex/);
    const t = await idle(main.id);
    assert.equal(t.provider, 'codex');
    assert.ok(t.nativeSessionId);
    assert.equal(t.modelInUse ?? t.model, MODEL);
    assert.ok(t.contextTokens > 0);
    assertProtocol(main.id);
  });

  test('a command and a new file, with edit.diff', async () => {
    const turnId = await send(
      main.id,
      'First run the shell command `echo hi`. Then use the apply_patch tool (not the shell) to create a file named c.txt containing the single line x. Reply with only: done',
    );
    const done = await turnDone(main.id, turnId);
    assert.equal(done.outcome, 'completed', done.error);
    const evs = turnEvents(main.id, turnId);
    const cmd = evs.find((e) => e.type === 'tool.call' && e.name === 'shell' && /echo hi/.test(e.title));
    assert.ok(cmd, 'a shell call for echo hi');
    const cmdResult = evs.find((e) => e.type === 'tool.result' && e.itemId === cmd.itemId);
    assert.equal(cmdResult.ok, true);
    assert.match(cmdResult.text, /hi/);
    const patch = evs.find((e) => e.type === 'tool.call' && e.name === 'apply_patch');
    assert.ok(patch, 'an apply_patch call');
    const diff = evs.find((e) => e.type === 'edit.diff' && e.itemId === patch.itemId);
    assert.ok(diff && /c\.txt$/.test(diff.path), 'an edit.diff for c.txt');
    assert.equal(fs.readFileSync(path.join(work, 'c.txt'), 'utf8').trim(), 'x');
    assertProtocol(main.id);
  });

  test('one apply_patch touching two files: two paths on one item', async () => {
    const turnId = await send(
      main.id,
      'Use the apply_patch tool once (not the shell), with a single patch that adds two files: a.txt containing one and b.txt containing two. Reply with only: done',
    );
    const done = await turnDone(main.id, turnId);
    assert.equal(done.outcome, 'completed', done.error);
    const evs = turnEvents(main.id, turnId);
    const patches = evs.filter((e) => e.type === 'tool.call' && e.name === 'apply_patch');
    const both = patches.find((p) => {
      const paths = new Set(evs.filter((e) => e.type === 'edit.diff' && e.itemId === p.itemId).map((e) => path.basename(e.path)));
      return paths.has('a.txt') && paths.has('b.txt');
    });
    assert.ok(both, `one apply_patch with both paths (got ${patches.length} calls)`);
    assert.equal(fs.readFileSync(path.join(work, 'a.txt'), 'utf8').trim(), 'one');
    assert.equal(fs.readFileSync(path.join(work, 'b.txt'), 'utf8').trim(), 'two');
  });

  test('a message sent during a turn is steered into it', async () => {
    const turnId = await send(main.id, 'Run the shell command `sleep 6` and wait for it to finish. Then reply in one short sentence, following any later message from me too.');
    await waitFor('the sleep starts', () => turnEvents(main.id, turnId).find((e) => e.type === 'tool.call' && /sleep 6/.test(e.title)));
    const steered = await send(main.id, 'When you reply after the sleep, include the word kiwi.');
    assert.equal(steered, turnId, 'the message joined the running turn');
    const done = await turnDone(main.id, turnId);
    assert.equal(done.outcome, 'completed', done.error);
    assert.match(replyText(main.id, turnId).toLowerCase(), /kiwi/);
    assert.equal(persisted(main.id).filter((e) => e.type === 'turn.completed' && e.turnId === turnId).length, 1);
    assert.equal(turnEvents(main.id, turnId).filter((e) => e.type === 'user.message').length, 2);
  });

  test('interrupting sleep 37 closes its row and the sleep is gone within 5 s', async () => {
    const turnId = await send(main.id, 'Run the shell command `sleep 37` and wait for it to finish. Then reply with only: woke');
    const call = await waitFor('the sleep starts', () => turnEvents(main.id, turnId).find((e) => e.type === 'tool.call' && /sleep 37/.test(e.title)));
    await waitFor('sleep running', () => running('sleep 37'), 15_000);
    await client.call('threads.interrupt', { threadId: main.id });
    const done = await turnDone(main.id, turnId, 30_000);
    assert.equal(done.outcome, 'interrupted');
    const result = turnEvents(main.id, turnId).find((e) => e.type === 'tool.result' && e.itemId === call.itemId);
    assert.ok(result, 'the command row is closed');
    assert.equal(result.ok, false);
    const t0 = Date.now();
    await waitFor('sleep gone', () => !running('sleep 37'), 5000);
    console.log(`sleep 37 gone ${Date.now() - t0} ms after the turn ended`);
    await idle(main.id);
  });

  test('ask mode: one command allowed, one denied', async () => {
    const dir = path.join(root, 'ask');
    fs.mkdirSync(dir);
    const t = await createThread({ cwd: dir, permissionMode: 'ask', text: 'You must actually run the shell command `touch allowed.txt` yourself, with your shell tool; an approval prompt is expected and fine. Then reply with just: done' });
    const req = await waitFor('approval', () => persisted(t.id).find((e) => e.type === 'approval.requested'));
    assert.equal(req.name, 'shell');
    assert.match(req.title, /touch allowed\.txt/);
    assert.equal(fs.existsSync(path.join(dir, 'allowed.txt')), false, 'nothing written before approval');
    await client.call('approvals.respond', { threadId: t.id, approvalId: req.approvalId, decision: 'allow' });
    const done = await turnDone(t.id, t.turnId);
    assert.equal(done.outcome, 'completed', done.error);
    assert.ok(fs.existsSync(path.join(dir, 'allowed.txt')));
    assertProtocol(t.id);
    // The deny in a thread of its own: on a second turn luna sometimes
    // decides the folder is read-only and doesn't try (or ask) at all.
    const d = await createThread({ cwd: dir, permissionMode: 'ask', text: 'You must actually run the shell command `touch denied.txt` yourself, with your shell tool; an approval prompt is expected and fine. Then reply with just: done' });
    const req2 = await waitFor('second approval', () => persisted(d.id).find((e) => e.type === 'approval.requested'));
    assert.match(req2.title, /touch denied\.txt/);
    await client.call('approvals.respond', { threadId: d.id, approvalId: req2.approvalId, decision: 'deny' });
    const done2 = await turnDone(d.id, d.turnId);
    assert.equal(done2.outcome, 'completed', done2.error);
    assert.equal(fs.existsSync(path.join(dir, 'denied.txt')), false);
    const declined = turnEvents(d.id, d.turnId).find((e) => e.type === 'tool.result' && e.itemId === req2.itemId);
    if (declined) assert.equal(declined.ok, false);
    assertProtocol(d.id);
  });

  test('a red PNG sent in comes back as red', async () => {
    const src = path.join(root, 'red.png');
    fs.writeFileSync(src, makePng(32, 32, [220, 20, 20]));
    const { image } = await client.call('media.import', { path: src });
    const turnId = await send(main.id, 'What colour is the attached image? Answer in one word.', { images: [image] });
    const done = await turnDone(main.id, turnId);
    assert.equal(done.outcome, 'completed', done.error);
    const um = turnEvents(main.id, turnId).find((e) => e.type === 'user.message');
    assert.ok(um.images[0].path.startsWith(path.join(home, 'threads', main.id, 'media')));
    assert.match(replyText(main.id, turnId).toLowerCase(), /red/);
  });

  let words: { id: string; turnId: string };

  test('a Threadbear tool call, then resume after an idle release remembers a word', async () => {
    words = await createThread({ text: 'Remember the word pineapple. Then call the threadbear list_threads tool and reply with only: ok' });
    const done = await turnDone(words.id, words.turnId);
    assert.equal(done.outcome, 'completed', done.error);
    const call = turnEvents(words.id, words.turnId).find((e) => e.type === 'tool.call' && e.name === 'mcp__threadbear__list_threads');
    assert.ok(call, 'list_threads was called');
    const result = turnEvents(words.id, words.turnId).find((e) => e.type === 'tool.result' && e.itemId === call.itemId);
    assert.equal(result.ok, true);
    const before = await idle(words.id);
    await waitFor('idle release', () => summaries.get(words.id)?.live === false, IDLE_MS + 30_000);
    await new Promise((r) => setTimeout(r, 1000));
    const turnId = await send(words.id, 'Call the threadbear list_threads tool again, then tell me the word I asked you to remember. Reply with the word only.');
    const done2 = await turnDone(words.id, turnId);
    assert.equal(done2.outcome, 'completed', done2.error);
    assert.match(replyText(words.id, turnId).toLowerCase(), /pineapple/);
    assert.ok(turnEvents(words.id, turnId).some((e) => e.type === 'tool.call' && e.name === 'mcp__threadbear__list_threads'), 'the dynamic tool survives the resume');
    const after = await idle(words.id);
    assert.equal(after.nativeSessionId, before.nativeSessionId, 'the same Codex thread, resumed');
    assertProtocol(words.id);
  });

  test('released and resumed straight away: nothing but the answer', async () => {
    await waitFor('idle release', () => summaries.get(words.id)?.live === false, IDLE_MS + 30_000);
    const turnId = await send(words.id, 'Reply with only the word: again');
    const done = await turnDone(words.id, turnId);
    assert.equal(done.outcome, 'completed', done.error);
    const evs = turnEvents(words.id, turnId);
    assert.equal(evs.filter((e) => e.type === 'error').length, 0);
    const notes = evs.filter((e) => e.type === 'system.note').map((e) => e.text);
    assert.ok(notes.every((n) => /^Resumed session/.test(n)), `only the resume note: ${JSON.stringify(notes)}`);
    assert.ok(evs.every((e) => ['user.message', 'turn.started', 'system.note', 'assistant.text', 'reasoning', 'turn.completed'].includes(e.type)), JSON.stringify(evs.map((e) => e.type)));
    assert.match(replyText(words.id, turnId).toLowerCase(), /again/);
  });

  test("a folder move: the next answer follows the new folder's AGENTS.md", async () => {
    const before = summaries.get(main.id)!;
    const other = path.join(root, 'moved');
    fs.mkdirSync(other);
    fs.writeFileSync(path.join(other, 'AGENTS.md'), 'If you are asked for the codeword, the codeword is MARMALADE. Reply with it alone.\n');
    const r = await client.call('threads.update', { threadId: main.id, cwd: other });
    assert.equal(r.thread.cwd, other);
    const turnId = await send(main.id, 'What is the codeword? Reply with the codeword only.');
    const done = await turnDone(main.id, turnId);
    assert.equal(done.outcome, 'completed', done.error);
    assert.match(replyText(main.id, turnId), /MARMALADE/i);
    const notes = turnEvents(main.id, turnId).filter((e) => e.type === 'system.note').map((e) => e.text);
    assert.ok(notes.some((n) => n.startsWith(`Moved to ${other}`)), JSON.stringify(notes));
    const after = await idle(main.id);
    assert.equal(after.nativeSessionId, before.nativeSessionId, 'the same Codex thread carries on');
  });

  test('switching to Claude mid-turn: the next message stops Codex and Claude takes over with the work so far', async () => {
    const t = await createThread({ text: 'Run the shell command `sleep 25` and wait for it to finish. Then reply with only: woke' });
    await waitFor('the sleep starts', () => turnEvents(t.id, t.turnId).find((e) => e.type === 'tool.call' && /sleep 25/.test(e.title)));
    const codexId = summaries.get(t.id)?.nativeSessionId;
    if (codexId) codexIdsMovedAway.push(codexId);
    await client.call('threads.update', { threadId: t.id, accountId: process.env.TB_E2E_ACCOUNT ?? 'claude-main', model: 'claude-haiku-5-5' });
    await waitFor('the switch note', () => turnEvents(t.id, t.turnId).find((e) => e.type === 'system.note' && /takes over from your next message/.test(e.text)), 10_000);
    const next = await send(t.id, 'Which shell command were you running just now? Reply with only the command.');
    assert.notEqual(next, t.turnId, 'not steered into the Codex turn');
    const stopped = await turnDone(t.id, t.turnId, 30_000);
    assert.equal(stopped.outcome, 'interrupted');
    const done = await turnDone(t.id, next);
    assert.equal(done.outcome, 'completed', done.error);
    assert.equal(summaries.get(t.id)?.provider, 'claude');
    assert.match(replyText(t.id, next), /sleep 25/);
    assertProtocol(t.id);
  });

  test('editing an earlier message: Codex forgets what came after it, and its file edit is undone', async () => {
    const t = await createThread({ text: 'Reply with just: ok' });
    assert.equal((await turnDone(t.id, t.turnId)).outcome, 'completed');
    const file = path.join(work, 'edit-e2e.txt');
    fs.writeFileSync(file, 'one\n');
    const second = (
      await client.call('threads.send', {
        threadId: t.id,
        text: 'My secret word is PELICAN. Use apply_patch to change edit-e2e.txt so it says two instead of one. Then reply with just: done',
      })
    ).turnId;
    assert.equal((await turnDone(t.id, second)).outcome, 'completed');
    assert.equal(fs.readFileSync(file, 'utf8').trim(), 'two', 'the agent edited the file');
    const seq = persisted(t.id).find((e) => e.type === 'user.message' && e.turnId === second).seq;
    const r = await client.call('threads.edit', {
      threadId: t.id,
      seq,
      text: 'What secret word did I tell you, if any? Answer with one word, or NONE. Do not use any tools.',
    });
    assert.equal(r.rewound.fromSeq, seq);
    assert.equal(r.rewound.files.how, 'diffs');
    assert.deepEqual(r.rewound.files.failed, []);
    assert.equal(fs.readFileSync(file, 'utf8'), 'one\n', 'the edit was undone');
    const done = await turnDone(t.id, r.turnId);
    assert.equal(done.outcome, 'completed', done.error);
    const reply = replyText(t.id, r.turnId);
    assert.match(reply, /NONE/i);
    assert.doesNotMatch(reply, /PELICAN/i);
    const edited = persisted(t.id).find((e) => e.type === 'user.message' && e.turnId === r.turnId);
    assert.equal(edited.editOf, seq);
    assertProtocol(t.id);
    fs.rmSync(file, { force: true });
  });

  test('archiving a thread archives it in Codex too', async () => {
    const native = summaries.get(words.id)!.nativeSessionId;
    await client.call('threads.update', { threadId: words.id, archived: true });
    await waitFor('archived in Codex', () => leftovers([native]).some((p) => p.includes('archived_sessions')), 60_000);
  });

  test('a generated image lands in the media folder, with no base64 in the log', { skip: !IMAGEGEN && 'set TB_E2E_CODEX_IMAGEGEN=1' }, async () => {
    const t = await createThread({ text: 'Use your image generation tool to make one image: a plain blue square. Make exactly one image, then reply with only: done' });
    const done = await turnDone(t.id, t.turnId, 300_000);
    assert.equal(done.outcome, 'completed', done.error);
    const result = turnEvents(t.id, t.turnId).find((e) => e.type === 'tool.result' && e.images?.length);
    assert.ok(result, 'a tool.result with an image');
    const img = result.images[0];
    assert.equal(img.mediaType, 'image/png');
    assert.ok(img.path.startsWith(path.join(home, 'threads', t.id, 'media')), img.path);
    assert.ok(fs.readFileSync(img.path).subarray(0, 8).equals(Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a])));
    const log = fs.readFileSync(path.join(home, 'threads', t.id, 'events.ndjson'), 'utf8');
    assert.equal(/[A-Za-z0-9+/]{4000,}/.test(log), false, 'no base64 in events.ndjson');
  });
});
