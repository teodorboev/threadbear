// Real end-to-end check for pools (docs/protocol.md, "Pools"): does the
// Claude CLI resume a session another login wrote, thinking signatures and
// all? A thread is told a code word on one login (TB_E2E_ACCOUNT), moved to
// another (TB_E2E_OTHER_ACCOUNT, claude-1) with threads.update, and asked for the word back. It
// must come back through the resumed session (the transcript copied into
// the new login's config dir), not through a handoff note. Cheapest model,
// tiny prompts, a temp working folder and a temp runner data dir. Run with
// `npm run test:e2e`.
import { describe, test, before, after } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawn, type ChildProcess } from 'node:child_process';
import { RunnerClient } from '../../src/client.ts';
import { checkSequence, isTransient } from '../helpers/protocol-check.ts';
import { projectDirName } from '../../src/transcripts.ts';

const FROM = process.env.TB_E2E_ACCOUNT ?? 'claude-main';
const TO = process.env.TB_E2E_OTHER_ACCOUNT ?? 'claude-1';
const MODEL = process.env.TB_E2E_MODEL ?? 'claude-haiku-5-5';
const RUNNER_DIR = path.join(import.meta.dirname, '..', '..');
// Made up for this run, so the model can't guess it.
const WORD = `PELICAN-${Math.floor(1000 + Math.random() * 9000)}`;

type Ev = Record<string, any>;

let root: string;
let home: string;
let cwd: string;
let runner: ChildProcess;
let client: RunnerClient;
let runnerOutput = '';
const events = new Map<string, Ev[]>();
let accounts: Ev[] = [];

async function startRunner(): Promise<void> {
  runner = spawn(process.execPath, ['src/main.ts'], {
    cwd: RUNNER_DIR,
    env: {
      ...process.env,
      THREADBEAR_HOME: home,
      THREADBEAR_PORT: '0',
      THREADBEAR_LOG: path.join(root, 'runner.log'),
      THREADBEAR_DEBUG: '1',
      THREADBEAR_CLI_UPDATES: 'off', THREADBEAR_TOKEN_TALLY: 'off',
      THREADBEAR_FOLLOW_MAIN: '0',
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
    if (method !== 'thread.event') return;
    const list = events.get(params.threadId) ?? [];
    list.push(params.event);
    events.set(params.threadId, list);
  });
  accounts = (await client.call('hello', { client: 'e2e', version: '1' })).accounts;
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

const persisted = (id: string) => (events.get(id) ?? []).filter((e) => !isTransient(e));
const turnDone = (id: string, turnId: string) => waitFor(`turn ${turnId}`, () => persisted(id).find((e) => e.type === 'turn.completed' && e.turnId === turnId));
const turnEvents = (id: string, turnId: string) => persisted(id).filter((e) => e.turnId === turnId);

describe('pools end to end: a thread moved to another Claude login resumes its session there', { concurrency: false }, () => {
  before(async () => {
    root = fs.mkdtempSync(path.join(os.tmpdir(), 'tb-e2e-pools-'));
    home = path.join(root, 'home');
    cwd = path.join(root, 'work');
    fs.mkdirSync(cwd, { recursive: true });
    assert.ok(!home.includes('Application Support/Threadbear'));
    await startRunner();
  });

  after(async () => {
    client?.close();
    if (runner && runner.exitCode === null) {
      const exited = new Promise((r) => runner.once('exit', r));
      runner.kill('SIGTERM');
      await exited;
    }
    // The transcripts this run left in both logins' config dirs, under the
    // temp folder's own name, and nothing else.
    const name = projectDirName(fs.realpathSync(cwd));
    for (const a of accounts.filter((x) => x.id === FROM || x.id === TO)) {
      if (name.includes('tb-e2e-pools-')) fs.rmSync(path.join(a.configDir, 'projects', name), { recursive: true, force: true });
    }
    if (process.env.TB_E2E_KEEP) console.log(`kept ${root}`);
    else fs.rmSync(root, { recursive: true, force: true });
  });

  test('tell it a word on one login, move it to another, and it still knows the word', async () => {
    assert.ok(accounts.some((a) => a.id === FROM), `${FROM} is a login here`);
    assert.ok(accounts.some((a) => a.id === TO), `${TO} is a login here`);
    const toTitle = accounts.find((a) => a.id === TO)!.title;

    // High effort, so the transcript has a thinking block with a signature
    // when the model thinks; the other login has to accept it.
    const r = await client.call('threads.create', {
      text: `Remember this code word for later: ${WORD}. Reply with just OK.`,
      accountId: FROM,
      model: MODEL,
      effort: 'high',
      cwd,
    });
    const id: string = r.thread.id;
    const sub = await client.call('threads.subscribe', { threadId: id, afterSeq: 0 });
    // Live events may have landed before the subscribe reply; keep one copy.
    const live = events.get(id) ?? [];
    const seen = new Set(sub.events.map((e: Ev) => e.seq));
    events.set(id, [...sub.events, ...live.filter((e) => isTransient(e) || !seen.has(e.seq))]);
    const first = await turnDone(id, r.turnId);
    assert.equal(first.outcome, 'completed', first.error);
    const session = (await client.call('threads.list', {})).threads.find((t: Ev) => t.id === id).nativeSessionId;
    assert.ok(session);

    // To the other login (pinned there, as Alex picking one by hand).
    const moved = (await client.call('threads.update', { threadId: id, accountId: TO })).thread;
    assert.equal(moved.accountId, TO);

    const { turnId } = await client.call('threads.send', { threadId: id, text: 'What was the code word? Reply with just the word.' });
    const done = await turnDone(id, turnId);
    assert.equal(done.outcome, 'completed', done.error);
    const evs = turnEvents(id, turnId);
    const notes = evs.filter((e) => e.type === 'system.note').map((e) => e.text);
    assert.ok(notes.includes(`Resumed the session on ${toTitle}, with the whole conversation.`), `resumed, not handed off: ${JSON.stringify(notes)}`);
    assert.ok(!notes.some((n) => /Started a new session|Couldn't resume/.test(n)), JSON.stringify(notes));
    const reply = evs.filter((e) => e.type === 'assistant.text').map((e) => e.text).join('\n');
    assert.match(reply, new RegExp(WORD), 'remembered through the resumed session');
    const after = (await client.call('threads.list', {})).threads.find((t: Ev) => t.id === id);
    assert.equal(after.nativeSessionId, session, 'the same session, now on the other login');
    // The transcript is in the new login's config dir.
    const to = accounts.find((a) => a.id === TO)!;
    assert.ok(fs.existsSync(path.join(to.configDir, 'projects', projectDirName(fs.realpathSync(cwd)), `${session}.jsonl`)));
    assert.deepEqual(checkSequence(events.get(id) ?? []), []);
  });
});
