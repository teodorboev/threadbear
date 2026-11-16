// AcpAdapter against the real `grok agent stdio`, on its cheapest model at low
// effort, in a temp folder. Grok keeps its login in ~/.grok, so this uses the
// user's own (it never reads it: Grok does) and its sessions land in
// ~/.grok/sessions like any other. Skipped when Grok isn't installed and
// signed in. Each test is one tiny turn.
//
// What matters most is that every write and command comes to the runner
// first, even where Grok's own settings say otherwise (grok-guard.ts): on
// 15 Nov 2026 a user's ~/.claude/settings.json with `defaultMode: "auto"`
// made Grok run both without asking.
import { describe, test, after } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { AcpAdapter, type AcpSession } from '../../src/adapters/acp/adapter.ts';
import { ACP_AGENTS } from '../../src/adapters/acp/agents.ts';
import { acpAsker } from '../../src/models/acp-oneshot.ts';
import type { AdapterOutput, SessionSink, ToolApprovalRequest } from '../../src/adapters/types.ts';
import type { Account, EffortLevel, PermissionMode } from '../../src/protocol.ts';
import { nullLogger } from '../../src/log.ts';
import { ProcessRegistry } from '../../src/proc.ts';

const agent = ACP_AGENTS.grok;
const bin = agent.findBinary({ home: os.homedir(), pathEnv: process.env.PATH ?? '' });
const ready = Boolean(bin) && agent.loggedIn(os.homedir());
const MODEL = 'grok-4.5';
const roots: string[] = [];
const sessions: AcpSession[] = [];

after(async () => {
  await Promise.all(sessions.map((s) => s.end({ hard: true }).catch(() => undefined)));
  for (const r of roots) fs.rmSync(r, { recursive: true, force: true });
});

function tmpRoot(): string {
  const root = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'tb-grok-e2e-')));
  roots.push(root);
  return root;
}

const account: Account = { id: 'grok-main', provider: 'grok', title: 'Grok', configDir: path.join(os.homedir(), '.grok') };

function rig(mode: PermissionMode, deny: boolean, o: { effort?: EffortLevel; tools?: unknown[] } = {}) {
  const root = tmpRoot();
  const cwd = path.join(root, 'work');
  fs.mkdirSync(cwd);
  const cards: ToolApprovalRequest[] = [];
  const outputs: AdapterOutput[] = [];
  const waiters: (() => void)[] = [];
  const sink: SessionSink = {
    output(out) {
      outputs.push(out);
      if (out.kind === 'result') waiters.splice(0).forEach((f) => f());
    },
    async requestApproval(r) {
      cards.push(r);
      return { decision: deny ? 'deny' : 'allow' };
    },
    async requestQuestion() {
      return { answers: null };
    },
    exited() {},
  };
  const adapter = new AcpAdapter({
    agent,
    binary: () => bin,
    registry: new ProcessRegistry(path.join(root, 'run'), nullLogger),
    log: nullLogger,
    dataDir: path.join(root, 'data'),
    toolsFor: () => (o.tools ?? []) as never,
  });
  const s = adapter.start({ threadId: 'e2e', cwd, account, model: MODEL, effort: o.effort ?? 'low', permissionMode: mode, sessionId: 'x', resume: false, mediaDir: path.join(root, 'm'), blobsDir: path.join(root, 'b'), tmpDir: root }, sink) as AcpSession;
  sessions.push(s);
  const turn = (text: string) =>
    new Promise<void>((resolve) => {
      waiters.push(resolve);
      s.send({ id: `t${outputs.length}`, text, images: [] });
    });
  return { root, cwd, cards, outputs, turn, adapter };
}

const WRITE_AND_RUN = (cwd: string) => `Use your write tool to create ${path.join(cwd, 'a.txt')} containing hi, then run the shell command \`echo ran > ${path.join(cwd, 'b.txt')}\`. Do nothing else.`;

describe('Grok threads end to end (real grok agent stdio)', { concurrency: false, skip: !ready && 'Grok is not installed and signed in' }, () => {
  test('a folder whose settings allow everything still gets asked, and a denied write never happens', { timeout: 150_000 }, async () => {
    const r = rig('ask', true);
    fs.mkdirSync(path.join(r.cwd, '.claude'));
    fs.writeFileSync(path.join(r.cwd, '.claude', 'settings.json'), JSON.stringify({ permissions: { defaultMode: 'bypassPermissions', allow: ['Bash(*)', 'Edit(*)', 'Write(*)'] } }));
    fs.mkdirSync(path.join(r.cwd, '.grok'));
    fs.writeFileSync(path.join(r.cwd, '.grok', 'config.toml'), '[permission]\nallow = ["Bash(*)", "Edit", "Write"]\n');
    await r.turn(WRITE_AND_RUN(r.cwd));
    assert.ok(r.cards.length >= 1, 'it asked');
    assert.ok(!r.cards[0].title.includes("hook '"), 'the card is about the call, not the hook');
    assert.equal(fs.existsSync(path.join(r.cwd, 'a.txt')), false, 'denied, so not written');
    assert.equal(fs.existsSync(path.join(r.cwd, 'b.txt')), false, 'and nothing ran');
  });

  test('Full writes and runs with no card', { timeout: 150_000 }, async () => {
    const r = rig('full', true);
    await r.turn(WRITE_AND_RUN(r.cwd));
    assert.equal(r.cards.length, 0);
    assert.equal(fs.existsSync(path.join(r.cwd, 'a.txt')), true);
    assert.equal(fs.existsSync(path.join(r.cwd, 'b.txt')), true);
  });

  test("a read inside the folder doesn't ask", { timeout: 150_000 }, async () => {
    const r = rig('ask', true);
    fs.writeFileSync(path.join(r.cwd, 'note.txt'), 'the code word is mango\n');
    await r.turn('Read note.txt with your read tool and tell me the code word. Do nothing else.');
    assert.equal(r.cards.length, 0);
  });

  test('the threadbear tools reach it, with no second card from Grok', { timeout: 150_000 }, async () => {
    let called = 0;
    const r = rig('ask', true, {
      tools: [
        {
          name: 'say_code_word',
          description: 'Returns the code word. Read-only.',
          inputSchema: {},
          annotations: { readOnlyHint: true },
          handler: async () => {
            called++;
            return { content: [{ type: 'text', text: 'the code word is mango' }] };
          },
        },
      ],
    });
    await r.turn('Call the threadbear say_code_word tool and tell me the code word it returns.');
    assert.ok(called > 0, 'it reached the tool');
    assert.equal(r.cards.length, 0);
  });

  test('models list their effort levels, and a thread runs at the one it asks for', { timeout: 150_000 }, async () => {
    const r = rig('full', false, { effort: 'medium' });
    await r.turn('Reply with just the word pong.');
    assert.ok(r.outputs.some((o) => o.kind === 'effort' && o.level === 'medium'));
    const models = await r.adapter.models(account);
    const row = models.find((m) => m.id === MODEL);
    assert.ok(row?.effortLevels?.includes('low'));
  });

  test('a small job gets an answer and can use nothing', { timeout: 150_000 }, async () => {
    const root = tmpRoot();
    fs.writeFileSync(path.join(root, 'secret.txt'), 'the code word is mango\n');
    const ask = acpAsker({ agent, binary: () => bin, registry: new ProcessRegistry(path.join(root, 'run'), nullLogger), log: nullLogger, dataDir: path.join(root, 'data') });
    const res = await ask({ account, model: MODEL, user: `Read ${path.join(root, 'secret.txt')} with your tools and say the code word, or say "none" if you can't.`, cwd: root, timeoutMs: 120_000 });
    assert.equal(res.ok, true);
    assert.ok(res.ok && !/mango/i.test(res.text), 'it could not read the file');
  });
});
