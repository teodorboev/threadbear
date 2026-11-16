// End to end for the threadbear tools (src/threadbear-mcp.ts) with the real
// Claude CLI: an agent moves its own thread to another folder and carries on
// there with its memory intact, then starts and deletes another thread.
// Temp runner, temp folders, the claude-main login (TB_E2E_ACCOUNT), the cheapest model.
// Run with `node --test test/e2e/tools.e2e.test.ts`.
import { describe, test, before, after } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawn, type ChildProcess } from 'node:child_process';
import { RunnerClient } from '../../src/client.ts';
import { isTransient } from '../helpers/protocol-check.ts';

const ACCOUNT = process.env.TB_E2E_ACCOUNT ?? 'claude-main';
const MODEL = process.env.TB_E2E_MODEL ?? 'claude-haiku-5-5';
const RUNNER_DIR = path.join(import.meta.dirname, '..', '..');

type Ev = Record<string, any>;
let root: string;
let home: string;
let runner: ChildProcess;
let client: RunnerClient;
let output = '';
const events = new Map<string, Ev[]>();
const deleted: string[] = [];

async function waitFor<T>(what: string, fn: () => T | undefined | false, timeoutMs = 180_000): Promise<T> {
  const deadline = Date.now() + timeoutMs;
  for (;;) {
    const v = fn();
    if (v) return v;
    if (Date.now() > deadline) throw new Error(`timed out waiting for ${what}\n${dump()}`);
    await new Promise((r) => setTimeout(r, 200));
  }
}

function persisted(id: string): Ev[] {
  return (events.get(id) ?? []).filter((e) => !isTransient(e));
}

function dump(): string {
  return [...events.entries()]
    .map(([id, evs]) => `--- ${id}\n` + evs.filter((e) => !isTransient(e)).map((e) => `${e.seq} ${e.type} ${(e.title ?? e.text ?? e.name ?? '').toString().slice(0, 160)}`).join('\n'))
    .join('\n');
}

describe('threadbear tools end to end (real Claude CLI)', { concurrency: false }, () => {
  let a: string; // folder the thread starts in
  let b: string; // folder it moves to

  before(async () => {
    root = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'tb-tools-e2e-')));
    home = path.join(root, 'home');
    a = path.join(root, 'start-here');
    b = path.join(root, 'moved-here');
    fs.mkdirSync(a, { recursive: true });
    fs.mkdirSync(b, { recursive: true });
    fs.writeFileSync(path.join(b, 'marker.txt'), 'you are in the new folder\n');
    runner = spawn(process.execPath, ['src/main.ts'], {
      cwd: RUNNER_DIR,
      env: { ...process.env, THREADBEAR_HOME: home, THREADBEAR_PORT: '0', THREADBEAR_LOG: path.join(root, 'runner.log'), THREADBEAR_DEBUG: '1', THREADBEAR_CLI_UPDATES: 'off', THREADBEAR_TOKEN_TALLY: 'off' },
      stdio: ['ignore', 'pipe', 'pipe'],
    });
    const port = await new Promise<number>((resolve, reject) => {
      runner.stdout!.on('data', (d) => {
        output += d;
        const m = /listening on (\d+)/.exec(output);
        if (m) resolve(Number(m[1]));
      });
      runner.stderr!.on('data', (d) => (output += d));
      runner.once('exit', (code) => reject(new Error(`runner exited ${code}:\n${output}`)));
    });
    const token = fs.readFileSync(path.join(home, 'secrets', 'runner-token'), 'utf8').trim();
    client = await RunnerClient.connect(`ws://127.0.0.1:${port}/rpc`, token);
    client.onNotification((method, params) => {
      if (method === 'thread.event') {
        const list = events.get(params.threadId) ?? [];
        list.push(params.event);
        events.set(params.threadId, list);
      } else if (method === 'thread.deleted') deleted.push(params.threadId);
    });
    await client.call('hello', { client: 'e2e-tools', version: '1' });
  });

  after(async () => {
    client?.close();
    if (runner && runner.exitCode === null) {
      const exited = new Promise((r) => runner.once('exit', r));
      runner.kill('SIGTERM');
      await exited;
    }
    if (process.env.TB_KEEP) console.log(`kept ${root}`);
    else fs.rmSync(root, { recursive: true, force: true });
  });

  test('move_thread: the thread carries on in the new folder and remembers', async () => {
    const r = await client.call('threads.create', {
      accountId: ACCOUNT,
      model: MODEL,
      cwd: a,
      text:
        `Remember this code word: PINEAPPLE-42.\n\n` +
        `Now call the mcp__threadbear__move_thread tool once, with exactly these arguments: ` +
        `${JSON.stringify({ folder: b, then: 'Run pwd and cat marker.txt, then tell me the code word I gave you.' })}. ` +
        `After the tool returns, reply with just "moving" and end your turn. Don't run any other tools in this turn.`,
    });
    const id = r.thread.id;
    await client.call('threads.subscribe', { threadId: id, afterSeq: 0 }).then((s: Ev) => {
      const live = events.get(id) ?? [];
      const seen = new Set(s.events.map((e: Ev) => e.seq));
      events.set(id, [...s.events, ...live.filter((e) => isTransient(e) || !seen.has(e.seq))]);
    });
    // Two turns: the one Alex started, and the one the move queued.
    await waitFor('two finished turns', () => persisted(id).filter((e) => e.type === 'turn.completed').length >= 2 && true);
    const evs = persisted(id);
    const call = evs.find((e) => e.type === 'tool.call' && e.name === 'mcp__threadbear__move_thread');
    assert.ok(call, `the agent called move_thread\n${dump()}`);
    assert.equal(call.title, `Move this thread to ${b}`);
    const res = evs.find((e) => e.type === 'tool.result' && e.itemId === call.itemId);
    assert.ok(res?.ok, `move_thread succeeded: ${res?.text}`);
    assert.ok(evs.some((e) => e.type === 'system.note' && e.text.startsWith(`Moved to ${b}`)), `transcript carried\n${dump()}`);
    const second = evs.filter((e) => e.type === 'user.message')[1];
    assert.match(second.text, /^\[Sent by the agent before moving to /);
    assert.ok(!second.text.includes('[Threadbear handoff]'));
    const after = evs.filter((e) => e.seq > second.seq);
    const results = after.filter((e) => e.type === 'tool.result').map((e) => e.text).join('\n');
    assert.match(results, new RegExp(b.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')), 'pwd shows the new folder');
    assert.match(results, /you are in the new folder/);
    const reply = after.filter((e) => e.type === 'assistant.text').map((e) => e.text).join('\n');
    assert.match(reply, /PINEAPPLE-42/, 'it still knows what it was told before the move');
    const done = after.filter((e) => e.type === 'turn.completed').at(-1);
    assert.equal(done?.outcome, 'completed');
    const t = (await client.call('threads.list', {})).threads.find((x: Ev) => x.id === id);
    assert.equal(t.cwd, b);
  });

  test('end_turn reminder gets a complete answer before the turn finishes', async () => {
    const r = await client.call('threads.create', {
      accountId: ACCOUNT,
      model: MODEL,
      cwd: a,
      text:
        'Test the end_turn reminder. First call mcp__threadbear__end_turn with needs "nothing", before writing any reply. ' +
        'After its result, follow the reminder and answer this question in one sentence: ' +
        'A box has 3 red balls and 2 blue balls. How many balls are there, and what counts support the total? ' +
        'Then call end_turn again with needs "nothing".',
    });
    const id = r.thread.id;
    await client.call('threads.subscribe', { threadId: id, afterSeq: 0 }).then((s: Ev) => {
      const live = events.get(id) ?? [];
      const seen = new Set(s.events.map((e: Ev) => e.seq));
      events.set(id, [...s.events, ...live.filter((e) => isTransient(e) || !seen.has(e.seq))]);
    });
    const done = await waitFor('the reply after the end_turn reminder', () => persisted(id).find((e) => e.type === 'turn.completed'));
    const evs = persisted(id);
    assert.equal(done.outcome, 'completed', dump());
    const calls = evs.filter((e) => e.type === 'tool.call' && e.name === 'mcp__threadbear__end_turn');
    assert.equal(calls.length, 2, dump());
    const refused = evs.find((e) => e.type === 'tool.result' && e.itemId === calls[0].itemId);
    assert.equal(refused?.ok, false, dump());
    assert.match(refused.text, /result and any supporting evidence/);
    const reply = evs.filter((e) => e.type === 'assistant.text' && e.seq > refused.seq && e.seq < calls[1].seq).map((e) => e.text).join('\n');
    assert.match(reply, /5|five/i);
    assert.match(reply, /3|three/i);
    assert.match(reply, /2|two/i);
    assert.match(reply, /red/i);
    assert.match(reply, /blue/i);
    assert.ok(evs.find((e) => e.type === 'tool.result' && e.itemId === calls[1].itemId)?.ok, dump());
  });

  test('create_thread, list_threads and delete_thread (which asks first)', async () => {
    const r = await client.call('threads.create', {
      accountId: ACCOUNT,
      model: MODEL,
      cwd: a,
      text:
        `Use the threadbear tools. 1) Call mcp__threadbear__create_thread with title "Scratch child" and text "Reply with just OK." ` +
        `2) Call mcp__threadbear__delete_thread on the id it returned. 3) Call mcp__threadbear__list_threads and tell me how many threads there are. Keep it short.`,
    });
    const id = r.thread.id;
    await client.call('threads.subscribe', { threadId: id, afterSeq: 0 });
    // delete_thread asks even though the thread has full access.
    const ask = await waitFor('the delete approval', () => persisted(id).find((e) => e.type === 'approval.requested'));
    assert.equal(ask.name, 'mcp__threadbear__delete_thread');
    assert.equal(ask.title, 'Delete the thread "Scratch child"');
    assert.deepEqual(ask.options, ['allow', 'deny'], 'no "Always allow" for a tool that always asks');
    const child = (await client.call('threads.list', {})).threads.find((x: Ev) => x.title === 'Scratch child');
    assert.ok(child, 'the child thread was created');
    assert.equal(child.cwd, a, "in the caller's folder");
    await client.call('approvals.respond', { threadId: id, approvalId: ask.approvalId, decision: 'allow' });
    await waitFor('the turn', () => persisted(id).find((e) => e.type === 'turn.completed'));
    assert.deepEqual(deleted, [child.id]);
    assert.ok(fs.readdirSync(path.join(home, 'trash')).some((n) => n.startsWith(child.id)));
    const list = (await client.call('threads.list', { include: ['inbox', 'snoozed', 'archived'] })).threads;
    assert.ok(!list.some((x: Ev) => x.id === child.id));
    const evs = persisted(id);
    for (const name of ['create_thread', 'delete_thread', 'list_threads']) {
      const c = evs.find((e) => e.type === 'tool.call' && e.name === `mcp__threadbear__${name}`);
      assert.ok(c, `${name} was called\n${dump()}`);
      assert.ok(evs.find((e) => e.type === 'tool.result' && e.itemId === c.itemId)?.ok, `${name} succeeded`);
    }
  });
});
