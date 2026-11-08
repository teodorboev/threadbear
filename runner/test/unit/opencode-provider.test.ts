// The OpenCode provider outside its adapter: its login, its slash commands
// and its name.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { discoverAccounts, readOpencodeModel } from '../../src/accounts.ts';
import { parseCommand, planCommand } from '../../src/slash-commands.ts';
import { providerTitle } from '../../src/protocol.ts';
import { ACCOUNTS, FakeAccounts, FakeAdapter, makeHarness, tmpDir } from '../helpers/fakes.ts';
import type { Account } from '../../src/protocol.ts';

function dirs() {
  const root = tmpDir('tb-opencode-acc-');
  return { claudeMainDir: path.join(root, 'no-claude'), claudeMainJson: path.join(root, 'no.json'), tokenStatsAccountsDir: path.join(root, 'none'), codexMainDir: path.join(root, 'no-codex') };
}

test('there is one OpenCode login when its binary is known, and none without', () => {
  assert.deepEqual(discoverAccounts(dirs()), []);
  const found = discoverAccounts({ ...dirs(), opencodePath: '/opt/homebrew/bin/opencode' });
  assert.equal(found.length, 1);
  assert.equal(found[0].id, 'opencode-main');
  assert.equal(found[0].provider, 'opencode');
  assert.equal(found[0].title, 'OpenCode');
});

test("opencode.json's model is read, and nothing else", () => {
  const dir = tmpDir('tb-opencode-conf-');
  const file = path.join(dir, 'opencode.json');
  fs.writeFileSync(file, JSON.stringify({ model: 'opencode/claude-sonnet-5-5', provider: { secret: 'x' } }));
  assert.equal(readOpencodeModel(file), 'opencode/claude-sonnet-5-5');
  fs.writeFileSync(file, JSON.stringify({ model: 'sonnet' }));
  assert.equal(readOpencodeModel(file), undefined);
  assert.equal(readOpencodeModel(path.join(dir, 'missing.json')), undefined);
});

test("OpenCode's slash commands go to it, apart from Threadbear's own", () => {
  const plan = (text: string) => planCommand('opencode', parseCommand(text)!);
  assert.deepEqual(plan('/review'), { kind: 'cli' });
  assert.deepEqual(plan('/init'), { kind: 'cli' });
  assert.deepEqual(plan('/clear'), { kind: 'clear' });
  assert.deepEqual(plan('/status'), { kind: 'status' });
  assert.equal(plan('/model x').kind, 'model');
});

test('provider names', () => {
  assert.equal(providerTitle('opencode'), 'OpenCode');
  assert.equal(providerTitle('codex'), 'Codex');
  assert.equal(providerTitle('claude'), 'Claude');
  assert.equal(providerTitle('something-new'), 'Claude');
});

// ------------------------------------------------ threads on the OpenCode login

const OPENCODE: Account = { id: 'opencode-main', provider: 'opencode', title: 'OpenCode', configDir: '/tmp/opencode', models: [{ id: 'opencode/big-pickle', model: 'opencode/big-pickle', title: 'Big Pickle' }] };

function opencodeHarness() {
  const claude = new FakeAdapter();
  const opencode = new FakeAdapter('opencode', { wakesAfterTasks: false });
  const h = makeHarness(undefined, {}, { accounts: new FakeAccounts([...ACCOUNTS, OPENCODE]), adapters: { claude, opencode } });
  return { ...h, claude, opencode };
}

const notes = (h: { events(id: string): any[] }, id: string): string[] => h.events(id).filter((e) => e.type === 'system.note').map((e) => e.text);

test('a thread on the OpenCode login runs on its adapter, with a model it lists', () => {
  const h = opencodeHarness();
  const { thread } = h.runner.create({ text: 'hi', accountId: 'opencode-main', model: 'opencode/big-pickle' });
  assert.equal(thread.provider, 'opencode');
  assert.equal(thread.model, 'opencode/big-pickle');
  assert.equal(h.claude.sessions.length, 0);
  assert.equal(h.opencode.sessions.at(-1)!.opts.model, 'opencode/big-pickle');
  assert.deepEqual(notes(h, thread.id), []);
});

test("a Claude model or an effort on an OpenCode thread gives way, with a note", () => {
  const h = opencodeHarness();
  const { thread } = h.runner.create({ text: 'hi', accountId: 'opencode-main', model: 'sonnet', effort: 'high' });
  assert.equal(thread.model, undefined);
  assert.equal(thread.effort, undefined);
  const said = notes(h, thread.id);
  assert.ok(said.some((n) => n.startsWith("sonnet isn't an OpenCode model")), said.join(' | '));
  assert.ok(said.some((n) => n.startsWith("OpenCode threads don't take an effort level")), said.join(' | '));
});

test('an OpenCode id on a Claude login gives way to the Claude default', () => {
  const h = opencodeHarness();
  const { thread } = h.runner.create({ text: 'hi', accountId: ACCOUNTS[0].id, model: 'ollama-cloud/glm-5.3' });
  assert.equal(thread.provider, 'claude');
  assert.notEqual(thread.model, 'ollama-cloud/glm-5.3');
});
