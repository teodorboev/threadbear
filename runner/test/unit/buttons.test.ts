import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { makeHarness, tick } from '../helpers/fakes.ts';
import { SessionNotFoundError } from '../../src/adapters/types.ts';
import { buildThreadbearTools } from '../../src/threadbear-mcp.ts';
import { AGENT_MESSAGE_PREFIX } from '../../src/protocol.ts';

function fixture(mode?: 'ask' | 'full') {
  const h = makeHarness();
  const { thread } = h.runner.create({ text: 'Daily brief', accountId: 'claude-3', ...(mode ? { permissionMode: mode } : {}) });
  h.adapter.last.init();
  const folder = path.join(h.home, 'work');
  fs.mkdirSync(folder, { recursive: true });
  return { h, thread, folder };
}

test('a start button starts its thread once, however many times it is pressed', () => {
  const { h, thread, folder } = fixture();
  const { button } = h.runner.createButton(thread.id, { title: 'Fill in the form', start: { text: 'Fill in the Pebble form', cwd: folder } });
  assert.match(button.id, /^[a-z0-9]{8}$/);
  assert.equal(button.start?.permissionMode, 'full');
  const before = h.runner.list(['inbox']).length;
  const first = h.runner.pressButton(thread.id, button.id).button;
  assert.ok(first.pressed);
  const again = h.runner.pressButton(thread.id, button.id).button;
  assert.deepEqual(again.pressed, first.pressed);
  assert.equal(h.runner.list(['inbox']).length, before + 1, 'one new thread, not two');
  const started = h.runner.get(first.pressed!.threadId);
  assert.equal(started.cwd, folder);
  assert.equal(started.startedBy?.label, 'Button');
  assert.equal(started.startedBy?.threadId, thread.id);
  const msg = h.runner.events(started.id).events.find((e) => e.type === 'user.message');
  assert.ok(msg && msg.type === 'user.message' && msg.text.startsWith(AGENT_MESSAGE_PREFIX) && msg.text.endsWith('Fill in the Pebble form'));
  const updates = h.runner.events(thread.id).events.filter((e) => e.type === 'button.updated');
  assert.equal(updates.length, 2, 'made, then pressed');
  // Survives a restart, still pressed.
  const back = h.restart();
  assert.deepEqual(back.runner.listButtons(thread.id).buttons[0].pressed, first.pressed);
});

test('a send button messages its thread once', () => {
  const { h, thread } = fixture();
  const other = h.runner.create({ text: 'Meetup', accountId: 'claude-3' }).thread;
  const { button } = h.runner.createButton(thread.id, { title: 'Ask it', send: { threadId: other.id, text: 'Post the meetup message' } });
  assert.equal(button.send?.threadTitle, h.runner.get(other.id).title);
  const count = () => h.runner.events(other.id).events.filter((e) => e.type === 'user.message').length + h.store.load(other.id)!.queue.length;
  const was = count();
  h.runner.pressButton(thread.id, button.id);
  h.runner.pressButton(thread.id, button.id);
  assert.equal(count(), was + 1);
});

test('buttons are checked when made', () => {
  const { h, thread, folder } = fixture();
  const n = h.runner.events(thread.id).events.length;
  assert.throws(() => h.runner.createButton(thread.id, { title: '', start: { text: 'x', cwd: folder } }), /title/);
  assert.throws(() => h.runner.createButton(thread.id, { title: 'x'.repeat(61), start: { text: 'x', cwd: folder } }), /60/);
  assert.throws(() => h.runner.createButton(thread.id, { title: 'Go', start: { text: 'x', cwd: path.join(folder, 'nope') } }), /Folder not found/);
  assert.throws(() => h.runner.createButton(thread.id, { title: 'Go', start: { text: 'x', cwd: folder, accountId: 'nobody' } }), /No account/);
  assert.throws(() => h.runner.createButton(thread.id, { title: 'Go', start: { text: 'x', cwd: folder, poolId: 'nopool' } }), /No pool/);
  assert.throws(() => h.runner.createButton(thread.id, { title: 'Go', send: { threadId: 'missing', text: 'x' } }), /No thread/);
  assert.throws(() => h.runner.createButton(thread.id, { title: 'Go', send: { threadId: thread.id, text: 'x' } }), /own thread/);
  assert.throws(() => h.runner.createButton(thread.id, { title: 'Go' }), /either start/);
  assert.throws(() => h.runner.createButton(thread.id, { title: 'Go', start: { text: 'x', cwd: folder }, send: { threadId: thread.id, text: 'x' } }), /not both/);
  assert.equal(h.runner.events(thread.id).events.length, n, 'nothing saved');
  assert.throws(() => h.runner.pressButton(thread.id, 'zzzzzzzz'), /not found/);
});

test("a restricted thread's buttons can't reach past its own access", () => {
  const { h, thread, folder } = fixture('ask');
  const made = h.runner.createButton(thread.id, { title: 'Go', start: { text: 'x', cwd: folder } }).button;
  assert.equal(made.start?.permissionMode, 'ask', "the maker's mode by default");
  assert.throws(() => h.runner.createButton(thread.id, { title: 'Go', start: { text: 'x', cwd: folder, permissionMode: 'full' } }), /can't start a thread with Full access/);
  const full = h.runner.create({ text: 'Other', accountId: 'claude-3' }).thread;
  assert.throws(() => h.runner.createButton(thread.id, { title: 'Go', send: { threadId: full.id, text: 'rm -rf' } }), /send_message asks/);
});

test('a press that fails is saved as failed and can be pressed again', () => {
  const { h, thread, folder } = fixture();
  const sub = path.join(folder, 'gone');
  fs.mkdirSync(sub);
  const { button } = h.runner.createButton(thread.id, { title: 'Go', start: { text: 'x', cwd: sub } });
  fs.rmdirSync(sub);
  assert.throws(() => h.runner.pressButton(thread.id, button.id), /Folder not found/);
  const failed = h.runner.listButtons(thread.id).buttons[0];
  assert.ok(failed.failed && !failed.pressed);
  fs.mkdirSync(sub);
  const ok = h.runner.pressButton(thread.id, button.id).button;
  assert.ok(ok.pressed && !ok.failed);
});

test('add_button is handed to every thread, and returns the link', async () => {
  const { h, thread } = fixture();
  const tool = buildThreadbearTools(h.runner, thread.id).find((t) => t.name === 'add_button');
  assert.ok(tool);
  const r = await tool.handler({ title: 'Look into it', start: { text: 'Look at the camera panning', folder: '.' } } as any, {} as any);
  assert.equal(r.isError, undefined, JSON.stringify(r));
  const id = h.runner.listButtons(thread.id).buttons[0].id;
  assert.match(JSON.stringify(r.content), new RegExp(`\\[Look into it\\]\\(button:${id}\\)`));
});

test('access is checked again when pressed', () => {
  const { h, thread, folder } = fixture();
  const { button } = h.runner.createButton(thread.id, { title: 'Go', start: { text: 'x', cwd: folder } });
  assert.equal(button.start?.permissionMode, 'full');
  h.runner.update({ threadId: thread.id, permissionMode: 'ask' });
  // A running turn keeps the access it started with; once it's over, the new mode counts.
  h.adapter.last.result();
  assert.throws(() => h.runner.pressButton(thread.id, button.id), /now has Ask before edits/);
  assert.ok(h.runner.listButtons(thread.id).buttons[0].failed);
});

// Run on the card was taken out (14 Nov 2026). A thread from while it was
// on can still have one of its buttons, with a command and no start or send.
test("a left-over Run from a card's command does nothing when pressed", () => {
  const { h, thread } = fixture();
  const st = (h.runner as any).threads.get(thread.id);
  h.runner.persistButton(st, { id: 'abcd1234', title: 'Run', createdAt: new Date().toISOString(), run: { command: 'touch ran.txt', cwd: st.rec.cwd } } as any);
  assert.throws(() => h.runner.pressButton(thread.id, 'abcd1234'), /no longer does anything/);
  assert.ok(!fs.existsSync(path.join(st.rec.cwd, 'ran.txt')));
});

// The Daily brief, 8 Nov: Mike pressed "RSVP yes", then told the brief to go
// ahead in his reply, and its agent (which hadn't seen the press) sent the
// same job again with send_message.
test("the agent hears which of its buttons were pressed, before the user's next message", () => {
  const { h, thread, folder } = fixture();
  const other = h.runner.create({ text: 'SF trip', accountId: 'claude-3' }).thread;
  const rsvp = h.runner.createButton(thread.id, { title: 'RSVP yes', send: { threadId: other.id, text: 'RSVP yes to the party' } }).button;
  const scope = h.runner.createButton(thread.id, { title: 'Scope the video', start: { text: 'Scope it', cwd: folder } }).button;
  h.runner.createButton(thread.id, { title: 'Not pressed', start: { text: 'x', cwd: folder } });
  const session = h.adapter.sessions.find((s) => s.opts.threadId === thread.id)!;
  session.result();
  h.runner.pressButton(thread.id, rsvp.id);
  const started = h.runner.pressButton(thread.id, scope.id).button.pressed!.threadId;
  h.runner.send(thread.id, 'Yeah go ahead with the RSVP');
  const text = session.sent.at(-1)!.text;
  assert.match(text, /pressed/);
  assert.match(text, /"RSVP yes"/);
  assert.match(text, new RegExp(other.id.slice(0, 8)));
  assert.match(text, new RegExp(`"Scope the video".*${started.slice(0, 8)}`));
  assert.doesNotMatch(text, /Not pressed/);
  assert.ok(text.endsWith('Yeah go ahead with the RSVP'));
  // Told once: the message after doesn't say it again.
  session.result();
  h.runner.send(thread.id, 'thanks');
  assert.equal(session.sent.at(-1)!.text, 'thanks');
});

test("a press told in a turn that didn't complete is told again", () => {
  const { h, thread } = fixture();
  const other = h.runner.create({ text: 'SF trip', accountId: 'claude-3' }).thread;
  const rsvp = h.runner.createButton(thread.id, { title: 'RSVP yes', send: { threadId: other.id, text: 'RSVP yes' } }).button;
  const session = h.adapter.sessions.find((s) => s.opts.threadId === thread.id)!;
  session.result();
  h.runner.pressButton(thread.id, rsvp.id);
  h.runner.send(thread.id, 'go');
  assert.match(session.sent.at(-1)!.text, /"RSVP yes"/);
  session.result('failed', undefined, 'overloaded');
  h.runner.send(thread.id, 'try again');
  assert.match(session.sent.at(-1)!.text, /"RSVP yes"/);
});

// Codex's review of the first try at the above.
function pressed() {
  const { h, thread, folder } = fixture();
  const other = h.runner.create({ text: 'SF trip', accountId: 'claude-3' }).thread;
  const rsvp = h.runner.createButton(thread.id, { title: 'RSVP yes', send: { threadId: other.id, text: 'RSVP yes to the party for 11 Dec' } }).button;
  const session = h.adapter.sessions.find((s) => s.opts.threadId === thread.id)!;
  session.result();
  h.runner.pressButton(thread.id, rsvp.id);
  return { h, thread, folder, session };
}
const allSent = (h: ReturnType<typeof makeHarness>, id: string) => h.adapter.sessions.filter((s) => s.opts.threadId === id).flatMap((s) => s.sent.map((m) => m.text));

test('the line says what the button sent, and outlives the button', () => {
  const { h, thread, session } = pressed();
  // As a move to another runner does.
  (h.runner as any).threads.get(thread.id).rec.buttons = [];
  h.runner.send(thread.id, 'go');
  assert.match(session.sent.at(-1)!.text, /"RSVP yes" messaged "[^"]*" \([0-9a-f]{8}\): RSVP yes to the party for 11 Dec/);
  assert.match(session.sent.at(-1)!.text, /something different/);
});

test('a restart part way through the turn that told it tells it again', () => {
  const { h, thread } = pressed();
  h.runner.send(thread.id, 'go');
  const back = h.restart();
  back.runner.send(thread.id, 'still there?');
  const texts = allSent(back, thread.id);
  assert.ok(texts.some((t) => /"RSVP yes"/.test(t)), JSON.stringify(texts));
});

test('a completed turn that told it, then a restart, does not tell it again', () => {
  const { h, thread, session } = pressed();
  h.runner.send(thread.id, 'go');
  session.result();
  const back = h.restart();
  back.runner.send(thread.id, 'thanks');
  assert.ok(!allSent(back, thread.id).some((t) => /"RSVP yes"/.test(t)));
});

test('editing back past the message that told it tells it again', async () => {
  const { h, thread, session } = pressed();
  h.adapter.rewind = async () => ({ resumeAt: 'entry-1' });
  const { turnId } = h.runner.send(thread.id, 'go ahead');
  session.result();
  const seq = h.store.log(thread.id).read(1, h.store.log(thread.id).lastSeq).find((e) => e.type === 'user.message' && e.turnId === turnId)!.seq;
  await h.runner.editMessage(thread.id, seq, 'go ahead, and book the gym');
  const texts = allSent(h, thread.id);
  assert.match(texts.at(-1)!, /"RSVP yes"[\s\S]*go ahead, and book the gym$/);
});

test('a fresh session that replays the message keeps the line', async () => {
  const { h, thread } = pressed();
  const back = h.restart();
  back.runner.send(thread.id, 'go');
  const resumed = back.adapter.last;
  assert.equal(resumed.opts.resume, true);
  assert.match(resumed.sent.at(-1)!.text, /"RSVP yes"/);
  resumed.sink.exited(new SessionNotFoundError('gone'));
  await tick();
  const fresh = back.adapter.last;
  assert.notEqual(fresh, resumed);
  assert.match(fresh.sent[0].text, /"RSVP yes"[\s\S]*go$/);
});

// Codex's second look.
test('a crash after a failed turn ended on disk tells it again', () => {
  const { h, thread, session } = pressed();
  h.runner.send(thread.id, 'go');
  (h.runner as any).flush();
  const file = path.join(h.store.threadDir(thread.id), 'thread.json');
  const saved = fs.readFileSync(file, 'utf8');
  session.result('failed', undefined, 'overloaded');
  // thread.json as it was before the turn ended: the crash.
  fs.writeFileSync(file, saved);
  const back = h.restart();
  back.runner.send(thread.id, 'still there?');
  assert.ok(allSent(back, thread.id).some((t) => /"RSVP yes"/.test(t)));
});

test('a fresh session hears presses told in earlier turns', async () => {
  const { h, thread, session } = pressed();
  h.runner.send(thread.id, 'go');
  session.result();
  const back = h.restart();
  back.runner.send(thread.id, 'finish the RSVP if it has not gone');
  const resumed = back.adapter.last;
  assert.doesNotMatch(resumed.sent.at(-1)!.text, /"RSVP yes"/);
  resumed.sink.exited(new SessionNotFoundError('gone'));
  await tick();
  assert.match(back.adapter.last.sent[0].text, /"RSVP yes"/);
});

test('no press goes untold however many there are', () => {
  const { h, thread } = fixture();
  const other = h.runner.create({ text: 'Jobs', accountId: 'claude-3' }).thread;
  const session = h.adapter.sessions.find((s) => s.opts.threadId === thread.id)!;
  session.result();
  for (let i = 0; i < 25; i++) {
    const b = h.runner.createButton(thread.id, { title: `Job ${i}`, send: { threadId: other.id, text: `Do job ${i}` } }).button;
    h.runner.pressButton(thread.id, b.id);
  }
  h.runner.send(thread.id, 'go ahead with those');
  const text = allSent(h, thread.id).at(-1)!;
  assert.match(text, /go ahead with those$/);
  for (let i = 0; i < 25; i++) assert.match(text, new RegExp(`"Job ${i}"`));
});

// Codex's third look: a press mid-turn doesn't push out ones still in flight.
test('presses in flight survive more presses, and a failed turn tells them all again', () => {
  const { h, thread } = fixture();
  const other = h.runner.create({ text: 'Jobs', accountId: 'claude-3' }).thread;
  const session = h.adapter.sessions.find((s) => s.opts.threadId === thread.id)!;
  session.result();
  const press = (i: number) => h.runner.pressButton(thread.id, h.runner.createButton(thread.id, { title: `Job ${i}`, send: { threadId: other.id, text: `Do job ${i}` } }).button.id);
  for (let i = 0; i < 25; i++) press(i);
  h.runner.send(thread.id, 'go ahead');
  press(25);
  session.result('failed', undefined, 'overloaded');
  h.runner.send(thread.id, 'again');
  const text = allSent(h, thread.id).at(-1)!;
  for (let i = 0; i <= 25; i++) assert.match(text, new RegExp(`"Job ${i}"`));
});
