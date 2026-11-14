// A thread on another runner (the Pixel, bruce, the cloud box) that ran out
// on one login and moved to another said nothing about it on the Mac: the
// runner's "Moved to ..." note never left that runner (13 Nov 2026). Now a
// login's note goes up as a message row (role "note", backend API 78),
// another runner plays it as the system.note it was, the iPhone app and the
// website get it when they ask (API 79), and an app built before, the
// phone's search and a shared page never do.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { toMessageDoc } from '../../src/sync.ts';
import { relayEvents, type RelayRow, type RelayThreadRow } from '../../src/relay.ts';
import { call, load } from '../helpers/convex-memory.ts';
import { threeWays } from '../helpers/routes.ts';

const MOVED = 'Moved to mike@example.org: mike@example.net hit its 5-hour limit, back at 11:30 am.';
const at = (s: number) => new Date(Date.parse('2026-11-13T02:00:00Z') + s * 1000).toISOString();

test("a login's note goes up as a note row, and no other note does", () => {
  const note = (fields: Record<string, unknown>) => toMessageDoc('t1', { type: 'system.note', seq: 7, ts: at(0), threadId: 't1', ...fields } as never, (t) => t.replace('sk-secret', '[key]'));
  assert.deepEqual(note({ text: MOVED, login: true }), { threadId: 't1', seq: 7, role: 'note', text: MOVED, at: at(0) });
  assert.equal(note({ text: 'Resumed session' }), undefined);
  assert.equal(note({ text: 'Background command stopped', wake: true }), undefined);
  // Scrubbed and cut, as every row is; its turn kept when it has one.
  const long = note({ text: `Moved sk-secret ${'x'.repeat(2000)}`, login: true, turnId: 'abcdef1234' })!;
  assert.ok(long.text.startsWith('Moved [key] '));
  assert.equal(long.text.length, 500);
  assert.equal(long.turn, 'abcdef12');
  assert.equal(note({ text: '  ', login: true }), undefined);
});

test('another Mac plays a note row as the note it was, between the turns', () => {
  const rows: RelayRow[] = [
    { seq: 2, role: 'user', text: 'Land it', turn: 'aaaa1111', at: at(0) },
    { seq: 3, role: 'assistant', text: "You've hit your 5-hour limit", turn: 'aaaa1111', at: at(1) },
    { seq: 5, role: 'note', text: MOVED, at: at(2) },
    { seq: 6, role: 'user', text: 'Carry on from where you left off.', origin: 'runner', turn: 'bbbb2222', at: at(3) },
    { seq: 7, role: 'assistant', text: 'Landed.', turn: 'bbbb2222', at: at(4) },
  ];
  const idle: RelayThreadRow = { runnerId: 'pixel', status: 'idle' };
  const ev = relayEvents('t', rows, idle, [], false, 'Pixel') as Record<string, any>[];
  const types = ev.map((e) => (e.type === 'system.note' ? `note:${e.turnId ?? ''}` : `${e.type}:${e.turnId}`));
  assert.deepEqual(types, [
    'turn.started:aaaa1111',
    'user.message:aaaa1111',
    'assistant.text:aaaa1111',
    'turn.completed:aaaa1111',
    'note:',
    'turn.started:bbbb2222',
    'user.message:bbbb2222',
    'assistant.text:bbbb2222',
    'turn.completed:bbbb2222',
  ]);
  const note = ev.find((e) => e.type === 'system.note')!;
  assert.equal(note.text, MOVED);
  assert.equal(note.login, true);
  // Seqs still rise, so the app keeps the note where it happened.
  assert.deepEqual(ev.map((e) => e.seq), [...ev.map((e) => e.seq)].sort((a, b) => a - b));
});

test("a note after the last turn leaves that turn's card and its end as they were", () => {
  const rows: RelayRow[] = [
    { seq: 2, role: 'user', text: 'Land it', turn: 'aaaa1111', at: at(0) },
    { seq: 3, role: 'assistant', text: 'Half way there.', turn: 'aaaa1111', at: at(1) },
    { seq: 5, role: 'note', text: 'Every login in Claude is out of room. Carrying on at 11:30 am when mike@example.net is back.', at: at(2) },
  ];
  const done = (row: RelayThreadRow, settled = true) => (relayEvents('t', rows, row, [], false, 'Pixel', settled) as Record<string, any>[]).find((e) => e.type === 'turn.completed');
  // The done card rides on the last turn's end, note or no note.
  assert.equal(done({ runnerId: 'pixel', status: 'idle', needs: 'nothing', next: ['Ship it'] })?.needs, 'nothing');
  assert.deepEqual(done({ runnerId: 'pixel', status: 'idle', needs: 'nothing', next: ['Ship it'] })?.next, ['Ship it']);
  // A thread waiting for room failed its last turn: still said.
  assert.equal(done({ runnerId: 'pixel', status: 'error' })?.outcome, 'failed');
  // And it isn't over until that runner's row says so.
  assert.equal(done({ runnerId: 'pixel', status: 'running' }), undefined);
  assert.equal(done({ runnerId: 'pixel', status: 'idle' }, false), undefined);
});

test("a kind of row this runner doesn't know is left out, not drawn as a tool call", () => {
  const rows = [
    { seq: 2, role: 'user', text: 'Hi', turn: 'aaaa1111', at: at(0) },
    { seq: 3, role: 'later', text: 'Something new', turn: 'aaaa1111', at: at(1) },
  ] as unknown as RelayRow[];
  const ev = relayEvents('t', rows, { runnerId: 'pixel', status: 'idle' }, [], false, 'Pixel') as Record<string, any>[];
  assert.equal(ev.some((e) => e.type === 'tool.call'), false);
});

test('through the backend: the other Mac gets the note, the phone and the website when they ask, a shared page never', async () => {
  const { views, refused, backend: b, threadId } = await threeWays((p) => {
    p.say('Half way there.');
    p.endTurn({ needs: 'nothing' });
    p.loginMoved();
  });
  assert.deepEqual(refused, []);
  const moved = 'Moved to mike@example.org: mike@example.net hit its 5-hour limit, back at 11:30 am.';
  assert.deepEqual(views.here.notes, [moved]);
  assert.deepEqual(views.relay.notes, [moved]);
  assert.deepEqual(views.phone.notes, [moved], 'the phone route now reads phone:messages with notes, like the app');
  const stored = (await b.ctx.db.query('messages').collect()) as unknown as { role: string; text: string }[];
  assert.deepEqual(stored.filter((m) => m.role === 'note').map((m) => m.text), [moved], 'the row is in the backend');
  // phone:messages with tool rows on, as an iPhone app built before API 79
  // asks: it can't read the role, so it isn't given one.
  const phone = load('phone');
  const page = (await call(b.ctx, phone.messages, { token: 'phone-token', threadId, tools: true })) as { messages: { role: string; text: string }[] };
  assert.ok(page.messages.length > 0);
  assert.equal(page.messages.some((m) => m.role === 'note' || m.text.includes('mike@example')), false);
  // And as the website and a newer iPhone app ask: the note, in its place.
  const asked = (await call(b.ctx, phone.messages, { token: 'phone-token', threadId, tools: true, notes: true })) as { messages: { role: string; text: string; seq: number }[] };
  assert.deepEqual(asked.messages.filter((m) => m.role === 'note').map((m) => m.text), [moved]);
  assert.equal(asked.messages.length, page.messages.length + 1);
  assert.deepEqual(asked.messages.map((m) => m.seq), [...asked.messages.map((m) => m.seq)].sort((x, y) => x - y));
  // Asked for one row with the note on top: the row under it, not an empty
  // page saying there's more (from Codex's review).
  const one = (await call(b.ctx, phone.messages, { token: 'phone-token', threadId, tools: true, limit: 1 })) as { messages: { role: string }[]; hasMore: boolean };
  assert.equal(one.messages.length, 1);
  assert.notEqual(one.messages[0]!.role, 'note');
  assert.equal(one.hasMore, true);
  // A runner from before API 78 doesn't ask for notes and isn't given
  // them: its relay would draw one as a tool call and take the last turn's
  // card away. One that asks gets them.
  const relay = load('relay');
  for (const fn of ['remoteMessages', 'remoteTail'] as const) {
    const plain = (await call(b.ctx, relay[fn], { key: 'owner-key', threadId })) as { messages: { role: string }[]; hasMore: boolean };
    assert.ok(plain.messages.length > 0, fn);
    assert.equal(plain.messages.some((m) => m.role === 'note'), false, `${fn} without notes`);
    assert.equal(plain.hasMore, false, fn);
    const asked = (await call(b.ctx, relay[fn], { key: 'owner-key', threadId, notes: true })) as { messages: { role: string }[] };
    assert.equal(asked.messages.filter((m) => m.role === 'note').length, 1, `${fn} with notes`);
    assert.equal(asked.messages.length, plain.messages.length + 1, fn);
  }
  const after = (await call(b.ctx, relay.remoteMessages, { key: 'owner-key', threadId, afterSeq: 0, limit: 1 })) as { messages: { role: string }[]; hasMore: boolean };
  assert.equal(after.messages.length, 1);
  assert.equal(after.hasMore, true);
  // A page shared by link.
  const row = (await b.ctx.db.query('threads').collect()).find((t: any) => t.threadId === threadId) as any;
  await b.ctx.db.patch(row._id, { share: { access: 'link', slug: 'link1' } });
  const shared = (await call(b.ctx, load('share').view, { slug: 'link1', limit: 30 })) as { messages: { role: string; text: string }[] };
  assert.ok(shared.messages.length > 0);
  assert.equal(shared.messages.some((m) => m.role === 'note' || m.text.includes('mike@example')), false);
});
