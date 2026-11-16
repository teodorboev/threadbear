// ask_secret and secrets.answer (docs/protocol.md, "Secrets").
import { test } from 'node:test';
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { execFileSync } from 'node:child_process';
import { makeHarness, tick, tmpDir } from '../helpers/fakes.ts';
import { SecretStore, SECRET_TTL_MS, openSealed, seal } from '../../src/secrets.ts';
import { buildThreadbearTools } from '../../src/threadbear-mcp.ts';
import { WROTE_INSTEAD } from '../../src/runner.ts';

const VALUE = 'hunter2 ✓';

// The phone (CryptoKit) side is tested against this same vector in
// ios/ThreadbearKit, so the two can't drift apart.
const VECTOR = {
  recipientPrivate: 'AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE=',
  recipientPublic: 'pOCSkrZRwni5dyxWn1+puxPZBrRqtoyd+dwrRAn4ogk=',
  epk: 'zo060cy2M+x7cMF4FKXHbs0CloUFDTRHRboFhw5YfVk=',
  nonce: 'AwMDAwMDAwMDAwMD',
  approvalId: 'approval-123',
  box: '818iYL0/xbv1amlCprX8NVHr59l6QxFyCAWu',
};
const privFromRaw = (b64: string) =>
  crypto.createPrivateKey({ key: Buffer.concat([Buffer.from('302e020100300506032b656e04220420', 'hex'), Buffer.from(b64, 'base64')]), format: 'der', type: 'pkcs8' });

test('the fixed vector opens, and only for its own approval', () => {
  const key = privFromRaw(VECTOR.recipientPrivate);
  const sealed = { epk: VECTOR.epk, nonce: VECTOR.nonce, box: VECTOR.box };
  assert.equal(openSealed(key, sealed, VECTOR.approvalId), VALUE);
  assert.throws(() => openSealed(key, sealed, 'another-approval'), (e: Error) => !e.message.includes(VALUE) && /couldn't be opened/.test(e.message));
  const tampered = { ...sealed, box: Buffer.from(Buffer.from(sealed.box, 'base64').map((b, i) => (i === 0 ? b ^ 1 : b))).toString('base64') };
  assert.throws(() => openSealed(key, tampered, VECTOR.approvalId), /couldn't be opened/);
  assert.throws(() => openSealed(key, { epk: 'x', nonce: 'y', box: 'z' }, VECTOR.approvalId), /couldn't be opened/);
});

test('the store makes one key pair, keeps it, and opens what is sealed to it', () => {
  const dir = tmpDir();
  const a = new SecretStore(dir);
  const pub = a.publicKey();
  assert.equal(Buffer.from(pub, 'base64').length, 32);
  assert.equal(fs.statSync(path.join(dir, 'secret-box.key')).mode & 0o777, 0o600);
  const b = new SecretStore(dir);
  assert.equal(b.publicKey(), pub, 'a second start uses the same key');
  assert.equal(b.open(seal(pub, VALUE, 'ap1'), 'ap1'), VALUE);
});

test('an answer goes to an owner-only file, with an askpass helper that prints it', () => {
  const dir = tmpDir();
  const store = new SecretStore(dir);
  const { file, askpass } = store.write(VALUE);
  assert.equal(path.dirname(file), store.dir);
  assert.equal(fs.statSync(store.dir).mode & 0o777, 0o700);
  assert.equal(fs.statSync(file).mode & 0o777, 0o600);
  assert.equal(fs.readFileSync(file, 'utf8'), VALUE);
  assert.equal(execFileSync(askpass, { encoding: 'utf8' }), VALUE);
  const second = store.write('other');
  assert.notEqual(second.file, file, 'each answer gets a file of its own');
});

test('the sweep removes files older than a day and nothing else', () => {
  const dir = tmpDir();
  let now = Date.now();
  const store = new SecretStore(dir, () => now);
  const old = store.write('old');
  const stranger = path.join(store.dir, 'notes.txt');
  fs.writeFileSync(stranger, 'not ours');
  const past = new Date(now - SECRET_TTL_MS - 60_000);
  for (const p of [old.file, old.askpass, stranger]) fs.utimesSync(p, past, past);
  const fresh = store.write('fresh');
  assert.equal(store.sweep(), 2);
  assert.ok(!fs.existsSync(old.file) && !fs.existsSync(old.askpass));
  assert.ok(fs.existsSync(fresh.file) && fs.existsSync(stranger));
  now += SECRET_TTL_MS + 60_000;
  assert.equal(store.sweep(), 2);
});

function askTool(h: ReturnType<typeof makeHarness>, selfId: string) {
  const list = buildThreadbearTools(h.runner, selfId, Date.now);
  const t = list.find((x) => x.name === 'ask_secret');
  assert.ok(t, 'ask_secret is offered');
  return async (args: Record<string, unknown>) => {
    const r = (await t.handler(args as never, {})) as { content: { text: string }[]; isError?: boolean };
    return r.content.map((c) => c.text).join('\n');
  };
}

function startThread(h: ReturnType<typeof makeHarness>) {
  const { thread } = h.runner.create({ text: 'Install the rule', accountId: 'claude-3' });
  h.adapter.last.init();
  return thread;
}

test('ask_secret: a card goes up, the answer goes to a file, and the value is nowhere else', async () => {
  const h = makeHarness();
  const thread = startThread(h);
  const reply = askTool(h, thread.id)({ label: 'Mac administrator password', reason: 'Install the Keep Awake rule.', placeholder: 'Your login password' });
  await tick();
  const req = h.events(thread.id).find((e) => e.type === 'secret.requested');
  assert.deepEqual({ label: req.label, reason: req.reason, placeholder: req.placeholder }, { label: 'Mac administrator password', reason: 'Install the Keep Awake rule.', placeholder: 'Your login password' });
  assert.equal(h.runner.get(thread.id).status, 'waiting');
  assert.ok(h.runner.get(thread.id).needsYou);
  // approvals.respond can't answer it: the value would go into its message.
  assert.throws(() => h.runner.respondApproval({ threadId: thread.id, approvalId: req.approvalId, decision: 'answered', message: VALUE }), /secrets.answer/);
  assert.throws(() => h.runner.respondApproval({ threadId: thread.id, approvalId: req.approvalId, decision: 'allow' }), /update the Threadbear app/);
  assert.throws(() => h.runner.answerSecret({ threadId: thread.id, approvalId: req.approvalId, secret: '   ' }), /Type the secret/);
  h.runner.answerSecret({ threadId: thread.id, approvalId: req.approvalId, secret: `  ${VALUE}\n` });
  const text = await reply;
  const file = /in '([^']+)' \(only you/.exec(text)?.[1];
  assert.ok(file, text);
  assert.equal(fs.readFileSync(file, 'utf8'), VALUE, 'trimmed');
  assert.match(text, /SUDO_ASKPASS='[^']+\.askpass' sudo -A/);
  assert.ok(!text.includes(VALUE));
  const resolved = h.events(thread.id).find((e) => e.type === 'approval.resolved' && e.approvalId === req.approvalId);
  assert.equal(resolved.decision, 'answered');
  assert.equal(resolved.message, undefined);
  assert.equal(h.runner.get(thread.id).status, 'running');
  // Not in any event, the thread's files on disk or what clients were sent.
  assert.ok(!JSON.stringify(h.notes).includes('hunter2'));
  const threadDir = path.join(h.home, 'threads', thread.id);
  for (const name of fs.readdirSync(threadDir)) {
    const p = path.join(threadDir, name);
    if (fs.statSync(p).isFile()) assert.ok(!fs.readFileSync(p, 'utf8').includes('hunter2'), name);
  }
});

test('ask_secret: declined, a message from Alex instead, and no answer in time', async () => {
  const h = makeHarness();
  const thread = startThread(h);
  const ask = askTool(h, thread.id);
  const pendingId = () => h.events(thread.id).filter((e) => e.type === 'secret.requested').at(-1).approvalId;

  const declined = ask({ label: 'Token', reason: 'For the deploy.' });
  await tick();
  h.runner.answerSecret({ threadId: thread.id, approvalId: pendingId(), decline: true });
  assert.match(await declined, /declined/);

  // An older phone's Deny on a card it drew as Allow/Deny.
  const oldPhone = ask({ label: 'Token', reason: 'For the deploy.' });
  await tick();
  h.runner.respondApproval({ threadId: thread.id, approvalId: pendingId(), decision: 'deny' });
  assert.match(await oldPhone, /declined/);

  const wrote = ask({ label: 'Token', reason: 'For the deploy.' });
  await tick();
  h.runner.send(thread.id, 'Use the one in 1Password instead');
  assert.match(await wrote, new RegExp(WROTE_INSTEAD.slice(0, 20).replace(/[.*+?^${}()|[\]\\]/g, '\\$&')));

  const slow = ask({ label: 'Token', reason: 'For the deploy.', timeoutMinutes: 2 });
  await tick();
  h.clock.advance(2 * 60_000);
  assert.match(await slow, /No answer: No answer in time/);
  const decisions = h.events(thread.id).filter((e) => e.type === 'approval.resolved').map((e) => e.decision);
  assert.deepEqual(decisions, ['deny', 'deny', 'deny', 'expired']);
  // The answered card can't be answered again.
  assert.throws(() => h.runner.answerSecret({ threadId: thread.id, approvalId: pendingId(), secret: VALUE }), /no longer waiting/);
});

test('ask_secret: interrupting the turn ends the request', async () => {
  const h = makeHarness();
  const thread = startThread(h);
  const reply = askTool(h, thread.id)({ label: 'Token', reason: 'For the deploy.' });
  await tick();
  h.runner.interrupt(thread.id);
  assert.match(await reply, /No answer/);
});
