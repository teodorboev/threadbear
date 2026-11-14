// Other runners' threads through the backend (relay.ts): rows become
// transcript events in relay seqs, a tail merges without holes, and actions
// go to the thread's runner as commands, with nothing published to the
// runner's own listeners (sync would mirror them as this runner's).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { MAX_CARDS, MAX_HELD, Relay, SCALE, mergeTail, relayEvents, type RelayRow, type RelayThreadRow } from '../../src/relay.ts';
import { FakeClock } from '../../src/clock.ts';
import { nullLogger } from '../../src/log.ts';
import type { RemoteThreads } from '../../src/thread-index.ts';
import { patiently } from '../helpers/patience.ts';

const T = 'thread-on-bruce';
const at = (s: number) => new Date(Date.parse('2026-11-05T10:00:00Z') + s * 1000).toISOString();
const rows: RelayRow[] = [
  { seq: 3, role: 'user', text: 'Fix it', turn: 'aaaa1111', at: at(0) },
  { seq: 5, role: 'tool', text: 'Read ~/x.ts', toolKind: 'read', turn: 'aaaa1111', at: at(2) },
  { seq: 9, role: 'assistant', text: 'Done', summary: true, turn: 'aaaa1111', at: at(10) },
  { seq: 12, role: 'user', text: 'Again', turn: 'bbbb2222', at: at(20) },
  { seq: 14, role: 'tool', text: 'Ran tests', toolKind: 'command', turn: 'bbbb2222', at: at(21) },
];
const idle: RelayThreadRow = { runnerId: 'bruce', status: 'idle', needs: 'nothing' };

test('rows become events eight seqs apart, with turns, results and ends made up beside them', () => {
  const ev = relayEvents(T, rows, idle, [], false, 'bruce');
  const types = ev.map((e) => `${e.seq}:${e.type}`);
  assert.deepEqual(types, [
    '24:turn.started',
    '25:user.message',
    '41:tool.call',
    '42:tool.result',
    '73:assistant.text',
    '79:turn.completed',
    '96:turn.started',
    '97:user.message',
    '113:tool.call',
    '114:tool.result',
    '119:turn.completed',
  ]);
  assert.equal(ev.find((e) => e.type === 'tool.result')!.text, 'The output is kept on bruce.');
  assert.deepEqual(ev.find((e) => e.type === 'tool.result')!.heldBack, { seq: 5 });
  assert.equal(ev.find((e) => e.seq === 79)!.durationMs, 10_000);
  assert.equal(ev.find((e) => e.seq === 73)!.summary, true);
});

test('a running turn has no end, and its last tool is still running', () => {
  const ev = relayEvents(T, rows, { runnerId: 'bruce', status: 'running' }, [], false, 'bruce');
  assert.equal(ev.some((e) => e.seq === 114), false);
  assert.equal(ev.some((e) => e.seq === 119), false);
  // The earlier turn still ended, at its next row.
  assert.ok(ev.some((e) => e.seq === 79 && e.type === 'turn.completed'));
  // Nor does one whose row has only just said it stopped.
  assert.equal(relayEvents(T, rows, idle, [], false, 'bruce', false).some((e) => e.seq === 119), false);
});

test('no turn.started for the first row held when older rows may still be its turn', () => {
  const ev = relayEvents(T, rows.slice(1), idle, [], true, 'bruce');
  assert.equal(ev[0]!.type, 'tool.call');
});

test("the open card is the last turn's end, and an approval sits at its own seq", () => {
  const asksRow: RelayThreadRow = { runnerId: 'bruce', status: 'idle', needs: 'you', asks: [{ action: 'Pick one', options: ['A', 'B'], ticked: true }] };
  const ev = relayEvents(T, rows, asksRow, [{ slot: 13 * SCALE + 1, pending: { approvalId: 'ap1', kind: 'approval', name: 'Bash', title: 'Run it', options: ['allow', 'deny'], inputSummary: 'npm test' } }], false, 'bruce');
  const end = ev.find((e) => e.seq === 119)!;
  assert.equal(end.needs, 'you');
  assert.deepEqual(end.asks, [{ action: 'Pick one', options: ['A', 'B'], ticked: true }]);
  // What each answer does comes through, so the Mac draws rows as it does
  // for its own runner's threads, not plain buttons (8 Nov).
  const noted = relayEvents(T, rows, { ...asksRow, asks: [{ action: 'Pick one', options: ['A', 'B'], optionNotes: ['Does A', 'Does B'] }] }, [], false, 'bruce');
  const notedEnd = noted.filter((e) => e.type === 'turn.completed').at(-1) as { asks?: unknown };
  assert.deepEqual(notedEnd.asks, [{ action: 'Pick one', options: ['A', 'B'], optionNotes: ['Does A', 'Does B'] }]);
  const ap = ev.find((e) => e.type === 'approval.requested')!;
  assert.equal(ap.seq, 13 * SCALE + 1);
  assert.equal(ap.turnId, 'bbbb2222');
});

test('a tail past what is held asks for the gap; one that overlaps just merges', () => {
  const held = new Map(rows.map((r) => [r.seq, r]));
  assert.deepEqual(mergeTail(held, { messages: [{ seq: 14, role: 'tool', text: 'x', at: at(1) }, { seq: 15, role: 'assistant', text: 'y', at: at(2) }], hasMore: true }), {});
  assert.deepEqual(mergeTail(held, { messages: [{ seq: 40, role: 'assistant', text: 'z', at: at(3) }], hasMore: true }), { gapAfter: 15 });
  assert.ok(held.has(40));
  // A tail that's the whole thread has no gap.
  assert.deepEqual(mergeTail(new Map([[1, rows[0]!]]), { messages: [{ seq: 50, role: 'user', text: 'q', at: at(4) }], hasMore: false }), {});
});

type Call = { kind: string; fn: string; args: Record<string, unknown> };
type HarnessOpts = { paged?: boolean; online?: boolean; row?: () => RelayThreadRow & { share?: unknown }; apiVersion?: number; queueFails?: boolean; uploadMs?: number; rows?: RelayRow[]; readMessages?: (args: Record<string, unknown>) => unknown };
function harness(opts: HarnessOpts = {}) {
  const calls: Call[] = [];
  const subs = new Map<string, { args: Record<string, unknown>; onUpdate: (v: unknown) => void; closed: boolean }[]>();
  const published: unknown[] = [];
  const subscribed = new Set<string>();
  const clock = new FakeClock();
  const remote: RemoteThreads = {
    runners: [{ id: 'bruce', name: 'bruce', online: opts.online ?? true, lastSeenAt: at(0) }],
    threads: [{ runnerId: 'bruce', threadId: T, title: 'A thread', cwd: '/Users/b/x', accountId: 'claude-main', provider: 'claude', status: 'idle', needsYou: false, unread: true, preview: '', createdAt: at(0), lastActivityAt: at(0) }],
  };
  let commandN = 0;
  let uploadN = 0;
  // What was uploaded to Convex, as [upload URL, bytes, type].
  const uploads: [string, string, string][] = [];
  const relay = new Relay({
    source: () => ({
      key: 'owner',
      apiVersion: opts.apiVersion ?? 68,
      upload: async (url, bytes, mediaType) => {
        if (opts.uploadMs) clock.advance(opts.uploadMs);
        uploads.push([url, Buffer.from(bytes).toString(), mediaType]);
        return `st${uploads.length}`;
      },
      post: async (kind, fn, args) => {
        calls.push({ kind, fn, args });
        if (fn === 'relay:remoteThread') return opts.row?.() ?? idle;
        if (fn === 'relay:remoteMessages' && opts.readMessages) return opts.readMessages(args);
        if (fn === 'relay:remoteMessages' && opts.paged) {
          // As convex/relay.ts pages: ascending after afterSeq, else the newest page.
          const all = opts.rows ?? rows;
          const limit = Number(args.limit ?? 50);
          if (typeof args.afterSeq === 'number') {
            const from = all.findIndex((r) => r.seq > (args.afterSeq as number));
            const page = from < 0 ? [] : all.slice(from, from + limit);
            return { messages: page, hasMore: from >= 0 && from + limit < all.length };
          }
          const before = typeof args.beforeSeq === 'number' ? all.filter((r) => r.seq < (args.beforeSeq as number)) : all;
          return { messages: before.slice(-limit), hasMore: before.length > limit };
        }
        if (fn === 'relay:remoteMessages') return { messages: opts.rows ?? rows, hasMore: false };
        if (fn === 'relay:queueCommand') {
          if (opts.queueFails) throw new Error('Convex is away');
          return `cmd${++commandN}`;
        }
        if (fn === 'relay:pictureUploadUrl') return `https://upload.example/${++uploadN}`;
        if (fn === 'relay:remoteAccounts') return [{ runnerId: 'bruce', id: 'claude-main', provider: 'claude', title: 'Claude' }];
        return null;
      },
      subscribe: (fn, args, onUpdate) => {
        const list = subs.get(fn) ?? [];
        const s = { args, onUpdate, closed: false };
        list.push(s);
        subs.set(fn, list);
        return () => {
          s.closed = true;
        };
      },
    }),
    remote: () => remote,
    // This runner's inbox: what the app's media.import made.
    readPicture: (file) => (file.startsWith('/inbox/') ? { bytes: Buffer.from(`bytes of ${file}`), mediaType: 'image/png' } : { skip: "it isn't in this runner's inbox" }),
    subscribed: (id) => subscribed.has(id),
    publish: (n) => published.push(n),
    clock,
    log: nullLogger,
  });
  return { relay, calls, subs, published, subscribed, clock, remote, uploads };
}

test('it answers only for threads other runners have, and pages in relay seqs', async () => {
  const h = harness();
  assert.equal(h.relay.owns(T), true);
  assert.equal(h.relay.owns('mine'), false);
  const page = (await h.relay.call(T, 'threads.events', { threadId: T, limit: 4 })) as { events: { seq: number }[]; relay: { atStart?: boolean } };
  assert.deepEqual(page.events.map((e) => e.seq), [97, 113, 114, 119]);
  assert.equal(page.relay.atStart, undefined);
  const older = (await h.relay.call(T, 'threads.events', { threadId: T, beforeSeq: 97, limit: 200 })) as { events: { seq: number }[]; relay: { atStart?: boolean } };
  assert.deepEqual(older.events.map((e) => e.seq), [24, 25, 41, 42, 73, 79, 96]);
  assert.equal(older.relay.atStart, true);
});

test('a subscribe watches a tail and the row, and new rows reach clients only', async () => {
  const h = harness();
  await h.relay.call(T, 'threads.events', { threadId: T });
  const back = (await h.relay.call(T, 'threads.subscribe', { threadId: T, afterSeq: 119 })) as { events: { seq: number }[] };
  // Nothing new; only what could have come late, again (the app ignores seqs it has).
  assert.ok(back.events.every((e) => e.seq <= 119 && [0, 2, 3, 4, 7].includes(e.seq % SCALE)));
  const tail = h.subs.get('relay:remoteTail')![0]!;
  assert.equal(tail.args.limit, 15);
  assert.ok(h.subs.get('relay:remoteThread'));
  // The next turn arrives while the thread runs.
  h.subs.get('relay:remoteThread')![0]!.onUpdate({ runnerId: 'bruce', status: 'running' });
  tail.onUpdate({ messages: [...rows.slice(-2), { seq: 20, role: 'user', text: 'More', turn: 'cccc3333', at: at(30) }], hasMore: true });
  await new Promise((r) => setImmediate(r));
  const seqs = h.published.map((n) => (n as { params: { event: { seq: number; type: string } } }).params.event).map((e) => `${e.seq}:${e.type}`);
  assert.deepEqual(seqs, ['160:turn.started', '161:user.message']);
  // An approval comes and is answered from here: its card, then its answer, both late-safe.
  h.subs.get('relay:remoteThread')![0]!.onUpdate({ runnerId: 'bruce', status: 'waiting', pendingApproval: { approvalId: 'ap9', kind: 'approval', seq: 22, name: 'Bash', title: 'Run', options: ['allow', 'deny'] } });
  const respond = h.relay.call(T, 'approvals.respond', { threadId: T, approvalId: 'ap9', decision: 'deny' });
  await new Promise((r) => setImmediate(r));
  const cmd = h.calls.find((c) => c.fn === 'relay:queueCommand')!;
  assert.deepEqual(cmd.args, { key: 'owner', runnerId: 'bruce', threadId: T, kind: 'respond', args: { approvalId: 'ap9', decision: 'deny' } });
  h.subs.get('relay:commandState')![0]!.onUpdate({ state: 'done' });
  await respond;
  const last = h.published.map((n) => (n as { params: { event: { seq: number; type: string; decision?: string } } }).params.event).slice(-2);
  assert.deepEqual(last.map((e) => `${e.seq}:${e.type}`), ['177:approval.requested', '178:approval.resolved']);
  assert.equal(last[1]!.decision, 'deny');
});

test('a subscriber receives only its missing late events, independent of another reader', async () => {
  const h = harness();
  await h.relay.call(T, 'threads.events', { threadId: T });
  // This reader has only the last page. Another reader loaded the earlier
  // turn, whose results must not appear in this reader's reply.
  const seen = [[96, 97], [113, 114], [119, 119]];
  const same = await h.relay.call(T, 'threads.subscribe', { afterSeq: 119, relaySeen: seen }) as { events: { seq: number }[] };
  assert.deepEqual(same.events, []);
  // A hole below afterSeq is real: the last tool result arrived late.
  const missing = await h.relay.call(T, 'threads.subscribe', { afterSeq: 119, relaySeen: [[96, 97], [113, 113], [119, 119]] }) as { events: { seq: number }[] };
  assert.deepEqual(missing.events.map((e) => e.seq), [114]);
  const fresh = await h.relay.call(T, 'threads.subscribe', { afterSeq: 0, relaySeen: [] }) as { events: { seq: number }[] };
  assert.ok(fresh.events.some((e) => e.seq === 25));
});

test('invalid relay seen ranges are refused rather than hiding events', async () => {
  const h = harness();
  for (const relaySeen of [null, [1], [[0, 1]], [[2, 1]], [[1, 120]], [[1.5, 2]], [[1, 3], [3, 4]], [[5, 6], [1, 2]], Array(20_001).fill([1, 1])]) {
    await assert.rejects(h.relay.call(T, 'threads.subscribe', { afterSeq: 119, relaySeen }), /relaySeen/);
  }
});

test('a page or subscription read does not consume live events owed to another subscriber', async () => {
  for (const method of ['threads.events', 'threads.subscribe']) {
    let release!: (value: unknown) => void;
    const gap = new Promise((resolve) => { release = resolve; });
    const h = harness({ readMessages: (args) => args.afterSeq === undefined ? { messages: rows, hasMore: false } : gap });
    await h.relay.call(T, 'threads.events', { threadId: T });
    await h.relay.call(T, 'threads.subscribe', { afterSeq: 119 });
    h.subscribed.add(T);
    h.subs.get('relay:remoteThread')![0]!.onUpdate({ runnerId: 'bruce', status: 'running' });
    h.subs.get('relay:remoteTail')![0]!.onUpdate({ messages: [{ seq: 20, role: 'user', text: 'More', turn: 'cccc3333', at: at(30) }], hasMore: true });
    // The tail merged seq 20 but is waiting for the missing rows 15..19.
    const reading = h.relay.call(T, method, { afterSeq: 119, relaySeen: [[96, 97], [113, 114], [119, 119]] });
    release({ messages: [], hasMore: false });
    await reading;
    await new Promise((r) => setImmediate(r));
    const got = h.published.map((n) => (n as { params: { event?: { seq: number } } }).params.event?.seq);
    assert.equal(got.filter((seq) => seq === 161).length, 1, `${method}: the already-subscribed reader still gets it once`);
  }
});

test('a gap fill cannot publish an invented early turn end to a reader or live listener', async () => {
  let release!: (value: unknown) => void;
  const gap = new Promise((resolve) => { release = resolve; });
  const h = harness({ row: () => ({ runnerId: 'bruce', status: 'running' }), readMessages: (args) => args.afterSeq === undefined ? { messages: rows, hasMore: false } : gap });
  await h.relay.call(T, 'threads.subscribe', { afterSeq: 0 });
  h.subscribed.add(T);
  h.subs.get('relay:remoteTail')![0]!.onUpdate({ messages: [{ seq: 20, role: 'user', text: 'Next turn', turn: 'cccc3333', at: at(30) }], hasMore: true });
  const page = await h.relay.call(T, 'threads.events', {}) as { events: { seq: number; type: string }[] };
  await new Promise((r) => setImmediate(r));
  assert.ok(page.events.some((e) => e.seq === 161), 'the known messages show without waiting for the fill');
  assert.ok(!page.events.some((e) => e.seq === 119 || e.seq === 160), 'the two turn boundaries are not known yet');
  release({ messages: [{ seq: 18, role: 'assistant', text: 'The real end', turn: 'bbbb2222', at: at(29) }], hasMore: false });
  await new Promise((r) => setImmediate(r));
  const events = h.published.map((n) => (n as { params: { event: { seq: number; type: string } } }).params.event);
  assert.ok(events.some((e) => e.seq === 151 && e.type === 'turn.completed'));
  assert.ok(!events.some((e) => e.seq === 119 && e.type === 'turn.completed'));
});

test('a stale final gap page cannot erase a newer tail hole', async () => {
  for (const empty of [false, true]) {
    let release!: (value: unknown) => void;
    const gap = new Promise((resolve) => { release = resolve; });
    const readAfter: number[] = [];
    const h = harness({ row: () => ({ runnerId: 'bruce', status: 'running' }), readMessages: (args) => {
      if (args.afterSeq === undefined) return { messages: rows, hasMore: false };
      readAfter.push(Number(args.afterSeq));
      if (readAfter.length === 1) return gap;
      return { messages: [{ seq: 45, role: 'assistant', text: 'Must not disappear', turn: 'c', at: at(45) }], hasMore: false };
    } });
    await h.relay.call(T, 'threads.subscribe', { afterSeq: 0 });
    const tail = h.subs.get('relay:remoteTail')![0]!;
    tail.onUpdate({ messages: [{ seq: 20, role: 'user', text: 'Second turn', turn: 'c', at: at(30) }], hasMore: true });
    tail.onUpdate({ messages: [{ seq: 50, role: 'user', text: 'Third turn', turn: 'd', at: at(50) }], hasMore: true });
    release({ messages: empty ? [] : [{ seq: 30, role: 'assistant', text: 'Earlier page', turn: 'c', at: at(40) }], hasMore: false });
    await new Promise((r) => setImmediate(r));
    assert.deepEqual(readAfter, [14, empty ? 14 : 30], 'read again after the new tail, even though the old answer said final');
    const page = await h.relay.call(T, 'threads.events', {}) as { events: { text?: string }[] };
    assert.ok(page.events.some((e) => e.text === 'Must not disappear'));
  }
});

test('a failed gap fill leaves the thread readable and a read retries it without a new tail', async () => {
  let reads = 0;
  const h = harness({ row: () => ({ runnerId: 'bruce', status: 'running' }), readMessages: (args) => {
    if (args.afterSeq === undefined) return { messages: rows, hasMore: false };
    if (++reads === 1) throw new Error('network still waking up');
    return { messages: [{ seq: 18, role: 'assistant', text: 'The real end', turn: 'bbbb2222', at: at(29) }], hasMore: false };
  } });
  await h.relay.call(T, 'threads.subscribe', { afterSeq: 0 });
  h.subs.get('relay:remoteTail')![0]!.onUpdate({ messages: [{ seq: 20, role: 'user', text: 'Next turn', turn: 'cccc3333', at: at(30) }], hasMore: true });
  await new Promise((r) => setImmediate(r));
  const page = await h.relay.call(T, 'threads.events', {}) as { events: { seq: number }[] };
  assert.ok(page.events.some((e) => e.seq === 161));
  await new Promise((r) => setImmediate(r));
  assert.equal(reads, 2, 'the page retried it without another tail update');
  const got = h.published.map((n) => (n as { params: { event: { seq: number } } }).params.event.seq);
  assert.ok(got.includes(151));
  assert.ok(!got.includes(119));
});

test('another tail during a gap fill cannot publish boundaries inside the merged holes', async () => {
  let release!: (value: unknown) => void;
  const gap = new Promise((resolve) => { release = resolve; });
  const h = harness({ row: () => ({ runnerId: 'bruce', status: 'running' }), readMessages: (args) => args.afterSeq === undefined ? { messages: rows, hasMore: false } : gap });
  await h.relay.call(T, 'threads.subscribe', { afterSeq: 0 });
  const tail = h.subs.get('relay:remoteTail')![0]!;
  tail.onUpdate({ messages: [{ seq: 20, role: 'user', text: 'Second turn', turn: 'c', at: at(30) }], hasMore: true });
  tail.onUpdate({ messages: [{ seq: 50, role: 'user', text: 'Third turn', turn: 'd', at: at(50) }], hasMore: true });
  const page = await h.relay.call(T, 'threads.events', {}) as { events: { seq: number; type: string }[] };
  assert.ok(page.events.some((e) => e.seq === 401));
  assert.ok(!page.events.some((e) => e.seq >= 112 && e.seq <= 400 && ['turn.started', 'turn.completed'].includes(e.type)));
  release({ messages: [{ seq: 18, role: 'assistant', text: 'End', turn: 'bbbb2222', at: at(29) }, { seq: 30, role: 'assistant', text: 'End', turn: 'c', at: at(49) }], hasMore: false });
  await new Promise((r) => setImmediate(r));
  const events = h.published.map((n) => (n as { params: { event: { seq: number; type: string } } }).params.event);
  assert.ok(!events.some((e) => e.seq === 119 || e.seq === 167));
  assert.ok(events.some((e) => e.seq === 247));
});

test('an idle subscribed thread retries a failed gap read without any app read or tail change', async () => {
  let reads = 0;
  const h = harness({ row: () => ({ runnerId: 'bruce', status: 'running' }), readMessages: (args) => {
    if (args.afterSeq === undefined) return { messages: rows, hasMore: false };
    if (++reads === 1) throw new Error('network still waking up');
    return { messages: [{ seq: 18, role: 'assistant', text: 'End', turn: 'bbbb2222', at: at(29) }], hasMore: false };
  } });
  await h.relay.call(T, 'threads.subscribe', { afterSeq: 0 });
  h.subscribed.add(T);
  h.subs.get('relay:remoteTail')![0]!.onUpdate({ messages: [{ seq: 20, role: 'user', text: 'Next', turn: 'c', at: at(30) }], hasMore: true });
  await new Promise((r) => setImmediate(r));
  h.clock.advance(15_000);
  await new Promise((r) => setImmediate(r));
  assert.equal(reads, 2);
  assert.ok(h.published.some((n) => (n as { params: { event: { seq: number } } }).params.event.seq === 151));
});

test('an older client gets turn starts that a gap delayed below its last seq', async () => {
  let release!: (value: unknown) => void;
  const gap = new Promise((resolve) => { release = resolve; });
  const h = harness({ row: () => ({ runnerId: 'bruce', status: 'running' }), readMessages: (args) => args.afterSeq === undefined ? { messages: rows, hasMore: false } : gap });
  await h.relay.call(T, 'threads.subscribe', { afterSeq: 0 });
  h.subs.get('relay:remoteTail')![0]!.onUpdate({ messages: [{ seq: 20, role: 'user', text: 'Next', turn: 'c', at: at(30) }], hasMore: true });
  const page = await h.relay.call(T, 'threads.events', {}) as { events: { seq: number }[] };
  assert.ok(page.events.some((e) => e.seq === 161));
  assert.ok(!page.events.some((e) => e.seq === 160));
  release({ messages: [{ seq: 18, role: 'assistant', text: 'End', turn: 'bbbb2222', at: at(29) }], hasMore: false });
  await new Promise((r) => setImmediate(r));
  const sub = await h.relay.call(T, 'threads.subscribe', { afterSeq: 161 }) as { events: { seq: number }[] };
  assert.ok(sub.events.some((e) => e.seq === 160), 'no relaySeen in an older client');
});

test("a turn ends a moment after the row says it stopped, so its last messages come first", async () => {
  const h = harness();
  await h.relay.call(T, 'threads.events', { threadId: T });
  await h.relay.call(T, 'threads.subscribe', { threadId: T, afterSeq: 119 });
  const row = h.subs.get('relay:remoteThread')![0]!;
  const tail = h.subs.get('relay:remoteTail')![0]!;
  row.onUpdate({ runnerId: 'bruce', status: 'running' });
  tail.onUpdate({ messages: [{ seq: 20, role: 'user', text: 'More', turn: 'cccc3333', at: at(30) }], hasMore: true });
  await new Promise((r) => setImmediate(r));
  row.onUpdate({ runnerId: 'bruce', status: 'idle', needs: 'nothing' });
  const kinds = () => h.published.map((n) => (n as { params: { event: { seq: number; type: string } } }).params.event).map((e) => `${e.seq}:${e.type}`);
  assert.equal(kinds().includes('167:turn.completed'), false);
  // Its reply lands after the row said idle.
  tail.onUpdate({ messages: [{ seq: 20, role: 'user', text: 'More', turn: 'cccc3333', at: at(30) }, { seq: 23, role: 'assistant', text: 'Done', turn: 'cccc3333', at: at(31) }], hasMore: true });
  await new Promise((r) => setImmediate(r));
  h.clock.advance(4000);
  assert.deepEqual(kinds().slice(-2), ['185:assistant.text', '191:turn.completed']);
});

test('a thread made from here opens before the sidebar lists it', async () => {
  const h = harness();
  const made = h.relay.create('bruce', { text: 'Start', permissionMode: 'ask' });
  await new Promise((r) => setImmediate(r));
  h.subs.get('relay:commandState')![0]!.onUpdate({ state: 'done', threadId: 'fresh' });
  await made;
  assert.equal(h.calls.at(-1)!.args.args && (h.calls.at(-1)!.args.args as { permissionMode?: string }).permissionMode, 'ask');
  assert.equal(h.relay.owns('fresh'), true);
});

test('the watch stops a minute after nobody is subscribed', async () => {
  const h = harness();
  await h.relay.call(T, 'threads.subscribe', { threadId: T, afterSeq: 0 });
  h.subscribed.add(T);
  h.clock.advance(30_000);
  assert.equal(h.subs.get('relay:remoteTail')![0]!.closed, false);
  h.subscribed.delete(T);
  for (let i = 0; i < 6; i++) h.clock.advance(15_000);
  assert.equal(h.subs.get('relay:remoteTail')![0]!.closed, true);
  assert.equal(h.subs.get('relay:remoteThread')![0]!.closed, true);
});

test('sends, stops and changes go to the thread runner; settings that only work there are refused', async () => {
  const h = harness();
  const send = h.relay.call(T, 'threads.send', { threadId: T, text: 'hi', answers: [{ action: 'a', answer: 'b' }] });
  await new Promise((r) => setImmediate(r));
  h.subs.get('relay:commandState')![0]!.onUpdate({ state: 'taken' });
  assert.deepEqual(await send, { turnId: '' });
  assert.deepEqual(h.calls.at(-1)!.args.args, { text: 'hi', answers: [{ action: 'a', answer: 'b' }] });
  const stop = h.relay.call(T, 'threads.interrupt', { threadId: T });
  await new Promise((r) => setImmediate(r));
  h.subs.get('relay:commandState')![1]!.onUpdate({ state: 'done' });
  await stop;
  assert.equal(h.calls.at(-1)!.args.kind, 'interrupt');
  await assert.rejects(h.relay.call(T, 'threads.update', { threadId: T, cwd: '/elsewhere' }), /isn't possible through Convex yet/);
});

// 13 Nov: Continue now on for a Pixel thread at its limit was refused as
// "Changing its login, model or settings isn't possible through Convex yet".
test("a login or model change goes to the thread's runner as one update, once the backend takes it, and shows what it applied", async () => {
  const old = harness();
  await assert.rejects(old.relay.call(T, 'threads.update', { threadId: T, accountId: 'claude-2' }), /once your Convex backend is updated/);
  assert.ok(!old.calls.some((c) => c.fn === 'relay:queueCommand'), 'nothing queued for a backend that would refuse it');
  const h = harness({ apiVersion: 77 });
  const moved = h.relay.call(T, 'threads.update', { threadId: T, accountId: 'claude-2', model: 'opus', carryOn: true });
  await new Promise((r) => setImmediate(r));
  // That runner put its own default in place of a model its login can't run.
  h.subs.get('relay:commandState')![0]!.onUpdate({ state: 'done', result: { accountId: 'claude-2', model: 'claude-sonnet-5-5' } });
  const { thread } = (await moved) as { thread: { accountId: string; model?: string } };
  const queued = h.calls.find((c) => c.fn === 'relay:queueCommand')!;
  assert.equal(queued.args.kind, 'update');
  assert.deepEqual(queued.args.args, { accountId: 'claude-2', model: 'opus', carryOn: true });
  assert.equal(thread.accountId, 'claude-2', 'the reply has the new login at once');
  assert.equal(thread.model, 'claude-sonnet-5-5', 'and the model that runner applied, not the one asked for');
  // An agent's goes as one, so that runner keeps a sandboxed thread in.
  const agent = h.relay.call(T, 'threads.update', { threadId: T, model: 'haiku', byAgent: true });
  await new Promise((r) => setImmediate(r));
  h.subs.get('relay:commandState')![1]!.onUpdate({ state: 'done' });
  await agent;
  assert.deepEqual(h.calls.filter((c) => c.fn === 'relay:queueCommand').at(-1)!.args.args, { model: 'haiku', by: 'agent' });
  // No row carries effort, so this Mac couldn't show what it became.
  await assert.rejects(h.relay.call(T, 'threads.update', { threadId: T, effort: 'high' }), /isn't possible through Convex yet/);
  await assert.rejects(h.relay.call(T, 'threads.update', { threadId: T, accountId: 'claude-2', title: 'x' }), /on its own/);
  await assert.rejects(h.relay.call(T, 'threads.update', { threadId: T, model: 'opus', carryOn: true }), /carryOn goes with accountId/);
});

// 8 Nov: a picture pasted for a thread on Hetzner Cloud was refused with
// "Pictures can't go to Hetzner Cloud from this Mac yet." Each is uploaded
// from this runner's inbox and named in the command, in order.
test('pictures in a send to another runner are uploaded and go with the command, in order', async () => {
  const h = harness();
  const send = h.relay.call(T, 'threads.send', { threadId: T, text: 'this [Picture 1] like [Picture 2]', images: [{ path: '/inbox/a.png', mediaType: 'image/png' }, { path: '/inbox/b.png', mediaType: 'image/png' }] });
  await new Promise((r) => setTimeout(r, 5));
  h.subs.get('relay:commandState')![0]!.onUpdate({ state: 'taken' });
  h.subs.get('relay:commandState')![0]!.onUpdate({ state: 'done' });
  assert.deepEqual(await send, { turnId: '' });
  assert.deepEqual(h.uploads, [
    ['https://upload.example/1', 'bytes of /inbox/a.png', 'image/png'],
    ['https://upload.example/2', 'bytes of /inbox/b.png', 'image/png'],
  ]);
  assert.deepEqual(h.calls.find((c) => c.fn === 'relay:queueCommand')!.args.args, { text: 'this [Picture 1] like [Picture 2]', images: ['st1', 'st2'] });
});

test('a new thread there takes pictures, words or not', async () => {
  const h = harness();
  const made = h.relay.create('bruce', { text: '', cwd: '/Users/b/x', runnerId: 'bruce', images: [{ path: '/inbox/a.png', mediaType: 'image/png' }] });
  await new Promise((r) => setTimeout(r, 5));
  h.subs.get('relay:commandState')![0]!.onUpdate({ state: 'done', threadId: 'new1' });
  assert.equal(((await made) as { thread: { id: string } }).thread.id, 'new1');
  assert.deepEqual(h.calls.find((c) => c.fn === 'relay:queueCommand')!.args.args, { text: '', cwd: '/Users/b/x', images: ['st1'] });
});

test("pictures that can't go say why, and nothing is queued or uploaded", async () => {
  // An older backend would drop them and send the words alone.
  const old = harness({ apiVersion: 62 });
  await assert.rejects(old.relay.call(T, 'threads.send', { threadId: T, text: 'x', images: [{ path: '/inbox/a.png' }] }), /backend is updated/);
  assert.equal(old.calls.length, 0);
  // Only this runner's own imports: a client naming any other file on this Mac doesn't get it posted to Convex.
  const h = harness();
  await assert.rejects(h.relay.call(T, 'threads.send', { threadId: T, text: 'x', images: [{ path: '/Users/m/.ssh/id_ed25519' }] }), /isn't in this runner's inbox/);
  await assert.rejects(h.relay.call(T, 'threads.send', { threadId: T, text: 'x', images: [{ nope: 1 }] }), /picture/);
  await assert.rejects(h.relay.call(T, 'threads.send', { threadId: T, text: 'x', images: Array.from({ length: 7 }, (_, i) => ({ path: `/inbox/${i}.png` })) }), /At most 6/);
  assert.equal(h.uploads.length, 0);
  assert.equal(h.calls.filter((c) => c.fn === 'relay:queueCommand').length, 0);
  // Offline: said at once, before uploading anything.
  const off = harness({ online: false });
  await assert.rejects(off.relay.call(T, 'threads.send', { threadId: T, text: 'x', images: [{ path: '/inbox/a.png' }] }), /bruce is offline/);
  assert.equal(off.uploads.length, 0);
});

// Codex's review: that runner marks a send taken before it fetches the
// pictures, so a fetch failing afterwards was a send the app thought went.
test("a send with pictures waits past taken for done, so a fetch that fails there says so", async () => {
  const h = harness();
  const pic = [{ path: '/inbox/a.png' }];
  const failing = h.relay.call(T, 'threads.send', { threadId: T, text: 'x', images: pic });
  await new Promise((r) => setTimeout(r, 5));
  const sub = h.subs.get('relay:commandState')![0]!;
  sub.onUpdate({ state: 'taken' });
  sub.onUpdate({ state: 'failed', note: 'HTTP 404' });
  await assert.rejects(failing, /HTTP 404/);
  // Taken and never finished: a yes at 100 s, before the app gives up at
  // 120 s and puts back a draft that may still go.
  const slow = h.relay.call(T, 'threads.send', { threadId: T, text: 'x', images: pic });
  await new Promise((r) => setTimeout(r, 5));
  h.subs.get('relay:commandState')![1]!.onUpdate({ state: 'taken' });
  let settled = false;
  void slow.then(() => (settled = true));
  for (const ms of [20_000, 40_000, 39_000]) {
    h.clock.advance(ms);
    await new Promise((r) => setImmediate(r));
    assert.equal(settled, false, 'still waiting for the pictures');
  }
  h.clock.advance(1_000);
  assert.deepEqual(await slow, { turnId: '' });
  // Not taken in 20 seconds: queued, as any send.
  const away = h.relay.call(T, 'threads.send', { threadId: T, text: 'x', images: pic });
  await new Promise((r) => setTimeout(r, 5));
  h.clock.advance(20_000);
  assert.deepEqual(await away, { turnId: '', queued: true });
});

test('uploads that take past a minute send nothing, so the answer still beats the app giving up', async () => {
  const h = harness({ uploadMs: 25_000 });
  const pics = [1, 2, 3, 4].map((i) => ({ path: `/inbox/${i}.png` }));
  await assert.rejects(h.relay.call(T, 'threads.send', { threadId: T, text: 'x', images: pics }), /took too long/);
  assert.equal(h.uploads.length, 3);
  assert.equal(h.calls.filter((c) => c.fn === 'relay:queueCommand').length, 0, 'nothing queued');
  assert.deepEqual(h.calls.find((c) => c.fn === 'relay:discardPictures')!.args.storageIds, ['st1', 'st2', 'st3']);
});

test('a last upload that ends past 75 s sends nothing, and a slow queue still answers by 100 s', async () => {
  // Three uploads of 26 s: the third starts at 52 s, inside the minute, and ends at 78 s.
  const late = harness({ uploadMs: 26_000 });
  await assert.rejects(late.relay.call(T, 'threads.send', { threadId: T, text: 'x', images: [1, 2, 3].map((i) => ({ path: `/inbox/${i}.png` })) }), /took too long/);
  assert.equal(late.calls.filter((c) => c.fn === 'relay:queueCommand').length, 0);
  // Uploaded by 72 s and queued: a new thread there waits only what's left of the 100 s.
  const h = harness({ uploadMs: 24_000 });
  const made = h.relay.create('bruce', { text: '', runnerId: 'bruce', images: [1, 2, 3].map((i) => ({ path: `/inbox/${i}.png` })) });
  await new Promise((r) => setTimeout(r, 5));
  h.subs.get('relay:commandState')![0]!.onUpdate({ state: 'taken' });
  let over = false;
  void made.catch(() => (over = true));
  h.clock.advance(27_000);
  await new Promise((r) => setImmediate(r));
  assert.equal(over, false);
  h.clock.advance(1_000);
  await assert.rejects(made, /hasn't answered yet/);
});

test("pictures uploaded for a command that couldn't be queued are discarded", async () => {
  const h = harness({ queueFails: true });
  await assert.rejects(h.relay.call(T, 'threads.send', { threadId: T, text: 'x', images: [{ path: '/inbox/a.png' }, { path: '/inbox/b.png' }] }), /Convex is away/);
  assert.deepEqual(h.calls.find((c) => c.fn === 'relay:discardPictures')!.args, { key: 'owner', storageIds: ['st1', 'st2'] });
});

test("a change answers with the thread, as the app decodes any runner's threads.update", async () => {
  // It answered {} and the app's archive failed with "Key 'thread' not found".
  const h = harness();
  const settle = (i: number) => h.subs.get('relay:commandState')![i]!.onUpdate({ state: 'done' });
  const archive = h.relay.call(T, 'threads.update', { threadId: T, archived: true, verdict: 'worked' });
  await new Promise((r) => setImmediate(r));
  settle(0);
  const { thread } = (await archive) as { thread: Record<string, unknown> };
  assert.equal(thread.id, T);
  assert.equal(thread.runnerId, 'bruce');
  assert.equal(thread.title, 'A thread');
  assert.equal(typeof thread.archivedAt, 'string');
  assert.equal(thread.lastSeq, 1, 'still unread');
  const snooze = h.relay.call(T, 'threads.update', { threadId: T, snoozedUntil: 'restart', starred: true, title: 'Renamed' });
  await new Promise((r) => setImmediate(r));
  settle(1);
  await new Promise((r) => setImmediate(r));
  settle(2);
  await new Promise((r) => setImmediate(r));
  settle(3);
  const after = ((await snooze) as { thread: Record<string, unknown> }).thread;
  assert.equal(after.title, 'Renamed');
  assert.equal(after.snoozedUntilRestart, true);
  assert.equal(typeof after.starredAt, 'string');
  assert.equal(after.archivedAt, thread.archivedAt, "the backend's row isn't archived yet, but the archive was done: a later reply mustn't undo it in the app");
  const markSeen = h.relay.call(T, 'threads.markSeen', { threadId: T, seq: 9 });
  await new Promise((r) => setImmediate(r));
  settle(4);
  const seen = ((await markSeen) as { thread: Record<string, unknown> }).thread;
  assert.equal(seen.lastSeq, 0);
  assert.equal(typeof seen.archivedAt, 'string', 'still archived');
  // A minute on, the row has caught up and is the truth again.
  h.clock.advance(61_000);
  const again = h.relay.call(T, 'threads.markSeen', { threadId: T, seq: 9 });
  await new Promise((r) => setImmediate(r));
  settle(5);
  const later = ((await again) as { thread: Record<string, unknown> }).thread;
  assert.equal(later.archivedAt, undefined);
  assert.equal(later.title, 'A thread');
});

test("a change from elsewhere wins once the row shows ours, and one that went through is kept when the next fails", async () => {
  const h = harness();
  let n = 0;
  const answer = async (state: 'done' | 'failed') => {
    await new Promise((r) => setImmediate(r));
    h.subs.get('relay:commandState')![n++]!.onUpdate(state === 'done' ? { state } : { state, note: 'bad time' });
  };
  // Renamed to A from here; the row catches up, then the phone renames it B.
  const rename = h.relay.call(T, 'threads.update', { threadId: T, title: 'A' });
  await answer('done');
  assert.equal(((await rename) as { thread: { title: string } }).thread.title, 'A');
  h.remote.threads[0]!.title = 'A';
  h.remote.threads[0]!.unread = false;
  assert.equal(((await h.relay.call(T, 'threads.markSeen', { threadId: T, seq: 1 })) as { thread: { title: string } }).thread.title, 'A');
  h.remote.threads[0]!.title = 'B';
  assert.equal(((await h.relay.call(T, 'threads.markSeen', { threadId: T, seq: 1 })) as { thread: { title: string } }).thread.title, 'B', "the phone's rename isn't hidden");
  // A rename that goes through, then a snooze that fails: the rename stays.
  const both = h.relay.call(T, 'threads.update', { threadId: T, title: 'C', snoozedUntil: 'not a time' });
  await answer('done');
  await answer('failed');
  await assert.rejects(both, /bad time/);
  assert.equal(((await h.relay.call(T, 'threads.markSeen', { threadId: T, seq: 1 })) as { thread: { title: string } }).thread.title, 'C');
});

test("the sidebar's list keeps an archive made here until the backend's row shows it", async () => {
  // A list sent for some other thread's change, before this row caught up,
  // put the thread just archived back in the inbox.
  const h = harness();
  assert.equal(h.relay.overlay(h.remote), h.remote, 'nothing changed here: the list as it is');
  const archive = h.relay.call(T, 'threads.update', { threadId: T, archived: true });
  await new Promise((r) => setImmediate(r));
  h.subs.get('relay:commandState')![0]!.onUpdate({ state: 'done' });
  await archive;
  assert.equal(typeof h.relay.overlay(h.remote).threads[0]!.archivedAt, 'string');
  assert.equal(h.remote.threads[0]!.archivedAt, undefined, "the backend's own list isn't changed");
  // The row catches up; then the phone unarchives it, and that shows.
  h.remote.threads[0]!.archivedAt = at(5);
  assert.equal(h.relay.overlay(h.remote).threads[0]!.archivedAt, at(5));
  delete h.remote.threads[0]!.archivedAt;
  assert.equal(h.relay.overlay(h.remote).threads[0]!.archivedAt, undefined);
});

test("sharing answers with the link, read from that runner's row once it shows it", async () => {
  let share: { access: string; slug: string } | undefined;
  const h = harness({ row: () => ({ ...idle, ...(share ? { share } : {}) }) });
  const done = async (i: number) => {
    await new Promise((r) => setImmediate(r));
    h.subs.get('relay:commandState')![i]!.onUpdate({ state: 'done' });
  };
  const on = h.relay.call(T, 'threads.share', { threadId: T, access: 'link' });
  await done(0);
  // Its row hasn't caught up the first time it's read: read again a moment later.
  await new Promise((r) => setImmediate(r));
  share = { access: 'link', slug: 'abc' };
  h.clock.advance(1000);
  assert.deepEqual(((await on) as { thread: { share: unknown } }).thread.share, { access: 'link', slug: 'abc' });
  assert.deepEqual(h.calls.filter((c) => c.fn === 'relay:queueCommand').at(-1)!.args, { key: 'owner', runnerId: 'bruce', threadId: T, kind: 'share', args: { access: 'link' } });
  // A new link: not done until the slug isn't the old one.
  const fresh = h.relay.call(T, 'threads.share', { threadId: T, access: 'link', newLink: true });
  await done(1);
  await new Promise((r) => setImmediate(r));
  share = { access: 'link', slug: 'def' };
  h.clock.advance(1000);
  assert.equal(((await fresh) as { thread: { share: { slug: string } } }).thread.share.slug, 'def');
  // Off keeps the slug, so sharing again brings the same link back.
  share = { access: 'off', slug: 'def' };
  const off = h.relay.call(T, 'threads.share', { threadId: T, access: 'off' });
  await done(2);
  assert.deepEqual(((await off) as { thread: { share: unknown } }).thread.share, { access: 'off', slug: 'def' });
  await assert.rejects(h.relay.call(T, 'threads.share', { threadId: T, access: 'people' }), /access must be/);
});

test("a link that never reaches the row says so, rather than showing none", async () => {
  const h = harness();
  const on = h.relay.call(T, 'threads.share', { threadId: T, access: 'link' });
  await new Promise((r) => setImmediate(r));
  h.subs.get('relay:commandState')![0]!.onUpdate({ state: 'done' });
  const said = assert.rejects(on, /has shared it, but its link hasn't reached Convex yet/);
  for (let i = 0; i < 5; i++) {
    await new Promise((r) => setImmediate(r));
    h.clock.advance(1000);
  }
  await said;
});

test("an open thread's share changing there (the website's New link, Stop sharing) is said to the app", async () => {
  const h = harness({ row: () => ({ ...idle, share: { access: 'link', slug: 'abc' } }) });
  await h.relay.call(T, 'threads.subscribe', { threadId: T, afterSeq: 0 });
  const said = () => h.published.filter((n) => (n as { method: string }).method === 'thread.updated').map((n) => (n as { params: { thread: { id: string; runnerId: string; share?: unknown } } }).params.thread);
  // Opened, and it's shared: the app hears so, as the sidebar's rows never say.
  assert.deepEqual(said().map((t) => [t.id, t.runnerId, t.share]), [[T, 'bruce', { access: 'link', slug: 'abc' }]]);
  const row = h.subs.get('relay:remoteThread')![0]!;
  row.onUpdate({ ...idle, status: 'running', share: { access: 'link', slug: 'abc' } });
  assert.equal(said().length, 1, 'nothing new about the share, nothing said');
  row.onUpdate({ ...idle, share: { access: 'link', slug: 'def' } });
  row.onUpdate({ ...idle, share: { access: 'people', slug: 'def' } });
  assert.deepEqual(said().slice(1).map((t) => t.share), [{ access: 'link', slug: 'def' }, { access: 'off', slug: 'def' }]);
  // An app that subscribes again (relaunched) hears it again, as it may not know.
  await h.relay.call(T, 'threads.subscribe', { threadId: T, afterSeq: 0 });
  assert.equal(said().length, 4);
  // A thread never shared says nothing on opening.
  const plain = harness();
  await plain.relay.call(T, 'threads.subscribe', { threadId: T, afterSeq: 0 });
  assert.equal(plain.published.filter((n) => (n as { method: string }).method === 'thread.updated').length, 0);
});

test("stopping a share that Convex doesn't show yet says so, rather than seeming done", async () => {
  const h = harness({ row: () => ({ ...idle, share: { access: 'link', slug: 'abc' } }) });
  const off = h.relay.call(T, 'threads.share', { threadId: T, access: 'off' });
  await new Promise((r) => setImmediate(r));
  h.subs.get('relay:commandState')![0]!.onUpdate({ state: 'done' });
  const said = assert.rejects(off, /has stopped sharing it, but Convex doesn't show that yet/);
  for (let i = 0; i < 5; i++) {
    await new Promise((r) => setImmediate(r));
    h.clock.advance(1000);
  }
  await said;
});

test('a failed command says why, and an offline runner is said at once', async () => {
  const h = harness();
  const p = h.relay.call(T, 'threads.update', { threadId: T, archived: true });
  await new Promise((r) => setImmediate(r));
  h.subs.get('relay:commandState')![0]!.onUpdate({ state: 'failed', note: 'No thread' });
  await assert.rejects(p, /No thread/);
  const off = harness({ online: false });
  await assert.rejects(off.relay.call(T, 'threads.send', { threadId: T, text: 'x' }), /bruce is offline/);
  assert.equal(off.calls.length, 0);
});

test("a new thread there, a folder there and that runner's logins", async () => {
  const h = harness();
  const made = h.relay.create('bruce', { text: 'Start', cwd: '/Users/b/x', runnerId: 'bruce' });
  await new Promise((r) => setImmediate(r));
  h.subs.get('relay:commandState')![0]!.onUpdate({ state: 'done', threadId: 'new1' });
  assert.equal(((await made) as { thread: { id: string } }).thread.id, 'new1');
  assert.deepEqual(h.calls.at(-1)!.args, { key: 'owner', runnerId: 'bruce', kind: 'create', args: { text: 'Start', cwd: '/Users/b/x' } });
  const list = h.relay.listFolder('bruce', '/Users/b');
  await new Promise((r) => setImmediate(r));
  h.subs.get('relay:commandState')![1]!.onUpdate({ state: 'done', result: { path: '/Users/b', entries: [], truncated: false } });
  assert.deepEqual(await list, { path: '/Users/b', entries: [], truncated: false });
  assert.deepEqual(h.calls.at(-1)!.args.args, { method: 'files.list', params: { path: '/Users/b' } });
  const logins = await h.relay.logins('bruce');
  assert.equal(logins.defaults.cwd, '/Users/b/x');
  assert.deepEqual(logins.accounts, [{ id: 'claude-main', provider: 'claude', title: 'Claude', configDir: '' }]);
});

test('an opened tool fills in from its body: its real call, result and diffs, in seqs of their own', () => {
  const bodies = new Map([[14, { state: 'text' as const, text: 'all passed', ok: true, input: JSON.stringify({ command: 'npm test' }), diffs: JSON.stringify([{ path: 'a.ts', hunks: [] }, { path: 'b.ts', hunks: [] }, { path: 'c.ts', hunks: [] }]), images: 2 }]]);
  const ev = relayEvents(T, rows, idle, [], false, 'bruce', true, bodies);
  const of = (n: number) => ev.find((e) => e.seq === 14 * SCALE + n) as Record<string, unknown> | undefined;
  assert.equal(of(2)!.type, 'tool.result', 'the stub stays, and is replaced by itemId');
  assert.deepEqual(of(5)!.input, { command: 'npm test' });
  assert.equal(of(5)!.itemId, of(2)!.itemId);
  assert.equal(of(3)!.path, 'a.ts');
  assert.equal(of(4)!.path, 'b.ts');
  assert.match(String(of(6)!.text), /all passed[\s\S]*Also changed: c\.ts[\s\S]*2 pictures are kept on bruce/);
  assert.equal(of(6)!.heldBack, undefined, 'nothing more to ask for');
  assert.equal(new Set(ev.map((e) => e.seq)).size, ev.length, 'every seq once');
  assert.deepEqual(ev.map((e) => e.seq), [...ev.map((e) => e.seq)].sort((a, b) => a - b));
});

test("a body that's withheld says so, and one that's missing changes nothing", () => {
  const withheld = relayEvents(T, rows, idle, [], false, 'bruce', true, new Map([[14, { state: 'withheld' as const, reason: 'environment' }]]));
  assert.match(String((withheld.find((e) => e.seq === 14 * SCALE + 6) as { text?: string }).text), /can hold secrets/);
  const plain = relayEvents(T, rows, idle, [], false, 'bruce');
  for (const body of [null, { state: 'expired' as const }]) assert.deepEqual(relayEvents(T, rows, idle, [], false, 'bruce', true, new Map([[14, body]])), plain);
});

test("a card from an older runner keeps its seqs: the body's diffs make way", () => {
  const card = { pending: { approvalId: 'a', kind: 'approval' as const, name: 'Bash', title: 'x', options: [] }, slot: 14 * SCALE + 3 };
  const ev = relayEvents(T, rows, idle, [card], false, 'bruce', true, new Map([[14, { state: 'text' as const, text: 'x', diffs: JSON.stringify([{ path: 'a.ts', hunks: [] }]) }]]));
  assert.equal(new Set(ev.map((e) => e.seq)).size, ev.length);
  assert.match(String((ev.find((e) => e.seq === 14 * SCALE + 6) as { text?: string }).text), /Also changed: a\.ts/);
});

test("an agent's archive goes to that runner with by: agent", async () => {
  const h = harness();
  h.subscribed.add(T);
  const done = h.relay.call(T, 'threads.update', { threadId: T, archived: true, archivedBy: 'agent' });
  await new Promise((r) => setImmediate(r));
  for (const s of h.subs.get('relay:commandState') ?? []) s.onUpdate({ state: 'done' });
  await done;
  const q = h.calls.find((c) => c.fn === 'relay:queueCommand')!;
  assert.deepEqual(q.args.args, { archive: true, by: 'agent' });
});

// Daybreak, 7 Nov 2026 ("Remote Plan first threads silently start with full
// access"): Relay.create forwarded only ask, full and auto, so a Plan first
// thread started on another Mac went with no mode and got full access.
test('a new thread there keeps every mode; Plan first goes as Ask plus a flag, and an unknown mode is refused', async () => {
  const sent = async (permissionMode: unknown) => {
    const h = harness();
    const made = h.relay.create('bruce', { text: 'Start', permissionMode });
    await new Promise((r) => setImmediate(r));
    h.subs.get('relay:commandState')![0]!.onUpdate({ state: 'done', threadId: 'new1' });
    await made;
    return h.calls.filter((c) => c.fn === 'relay:queueCommand').at(-1)!.args.args as Record<string, unknown>;
  };
  for (const mode of ['full', 'ask', 'auto'] as const) assert.deepEqual(await sent(mode), { text: 'Start', permissionMode: mode });
  const plan = await sent('plan');
  assert.deepEqual(plan, { text: 'Start', permissionMode: 'ask', plan: true });
  // What a runner from before Plan first reads of it: Ask, never the full-access default.
  const oldReceiver = (a: Record<string, unknown>) => (a.permissionMode === 'ask' || a.permissionMode === 'full' || a.permissionMode === 'auto' ? a.permissionMode : 'full');
  assert.equal(oldReceiver(plan), 'ask');
  assert.deepEqual(await sent(undefined), { text: 'Start' });
  const h = harness();
  await assert.rejects(h.relay.create('bruce', { text: 'Start', permissionMode: 'yolo' }), /permissionMode must be one of/);
  assert.equal(h.calls.filter((c) => c.fn === 'relay:queueCommand').length, 0, 'nothing queued');
});

test("hibernating in words goes as an update with the words, and answers the row once that runner has worked out when", async () => {
  // 7 Nov: "Hibernate until…" on another Mac's thread showed "Hibernating
  // until something happens isn't possible through Convex yet".
  const h = harness();
  const p = h.relay.call(T, 'threads.hibernateWhen', { threadId: T, said: 'at the weekend' });
  await new Promise((r) => setImmediate(r));
  const queued = h.calls.find((c) => c.fn === 'relay:queueCommand')!;
  assert.equal(queued.args.kind, 'update');
  // untilRestart: what an older runner, which doesn't read the words, does instead of waking it.
  assert.deepEqual(queued.args.args, { snooze: { said: 'at the weekend', untilRestart: true } });
  // That runner put it away at once, and its row says so before the model answers.
  h.remote.threads[0]!.snoozedUntil = at(3600);
  h.subs.get('relay:commandState')![0]!.onUpdate({ state: 'done' });
  const { thread } = (await p) as { thread: Record<string, unknown> };
  assert.equal(thread.id, T);
  assert.equal(thread.snoozedUntil, at(3600));
  // The model couldn't say: the note comes back as the error.
  const bad = h.relay.call(T, 'threads.hibernateWhen', { threadId: T, said: 'whenever' });
  await new Promise((r) => setImmediate(r));
  h.subs.get('relay:commandState')![1]!.onUpdate({ state: 'failed', note: "Couldn't work out when to wake it: no time in that" });
  await assert.rejects(bad, /Couldn't work out when/);
  await assert.rejects(h.relay.call(T, 'threads.hibernateWhen', { threadId: T, said: '  ' }), /Say when/);
});

test('hibernating in words: done before the row has its time still answers it as put away', async () => {
  // Codex's review: done can beat the row, and the app then put an awake row over its guess.
  const h = harness();
  const p = h.relay.call(T, 'threads.hibernateWhen', { threadId: T, said: 'tomorrow morning' });
  await new Promise((r) => setImmediate(r));
  h.subs.get('relay:commandState')![0]!.onUpdate({ state: 'done' });
  for (let i = 0; i < 10; i++) {
    await new Promise((r) => setImmediate(r));
    h.clock.advance(1000);
  }
  const { thread } = (await p) as { thread: Record<string, unknown> };
  assert.equal(thread.snoozedUntilRestart, true);
  assert.ok(thread.snoozedUntil);
  // Later replies keep it put away until a snoozed row comes, then show the row's time.
  assert.equal((h.relay as any).summary(T).snoozedUntilRestart, true);
  h.remote.threads[0]!.snoozedUntil = at(7200);
  assert.equal((h.relay as any).summary(T).snoozedUntil, at(7200));
});

// docs/parity.md showed reply buttons and slash commands working from the
// phone and not from the Mac on a Hetzner or bruce thread. The relay answers
// buttons.list from the rows' buttons (backend API 68), and presses and lists
// with the commands the phone sends.
test("a thread there's reply buttons come from its rows, and pressing one presses it there", async () => {
  const withButtons: RelayRow[] = [
    ...rows,
    { seq: 16, role: 'assistant', text: '[Fix it](button:abcd1234) [Tell them](button:efgh5678)', turn: 'bbbb2222', at: at(22), buttons: [
      { id: 'abcd1234', title: 'Fix it', kind: 'start', target: 'threadbear' },
      { id: 'efgh5678', title: 'Tell them', kind: 'send', target: 'Deploy', failed: 'that thread has gone' },
    ] },
  ];
  const h = harness({ rows: withButtons });
  const listed = (await h.relay.call(T, 'buttons.list', { threadId: T })) as { buttons: Record<string, unknown>[] };
  assert.deepEqual(listed.buttons.map((b) => [b.id, b.title, Boolean(b.start), Boolean(b.send), (b.failed as { error?: string } | undefined)?.error]), [
    ['abcd1234', 'Fix it', true, false, undefined],
    ['efgh5678', 'Tell them', false, true, 'that thread has gone'],
  ]);
  // Never the brief: the rows don't have it.
  assert.equal((listed.buttons[0]!.start as { text: string }).text, '');
  const press = h.relay.call(T, 'buttons.press', { threadId: T, buttonId: 'abcd1234' });
  await new Promise((r) => setTimeout(r, 5));
  assert.deepEqual(h.calls.find((c) => c.fn === 'relay:queueCommand')!.args, { key: 'owner', runnerId: 'bruce', threadId: T, kind: 'press', args: { buttonId: 'abcd1234' } });
  h.subs.get('relay:commandState')!.at(-1)!.onUpdate({ state: 'taken' });
  h.subs.get('relay:commandState')!.at(-1)!.onUpdate({ state: 'done', threadId: 'started1' });
  const pressed = (await press) as { button: { id: string; pressed?: { threadId: string } } };
  assert.equal(pressed.button.id, 'abcd1234');
  assert.equal(pressed.button.pressed?.threadId, 'started1');
  // A press that failed there says why.
  const failing = h.relay.call(T, 'buttons.press', { threadId: T, buttonId: 'efgh5678' });
  await new Promise((r) => setTimeout(r, 5));
  h.subs.get('relay:commandState')!.at(-1)!.onUpdate({ state: 'failed', note: 'Thread not found' });
  await assert.rejects(failing, /Thread not found/);
});

test("a thread there's slash commands are that runner's list", async () => {
  const h = harness();
  const list = h.relay.call(T, 'threads.commands', { threadId: T });
  await new Promise((r) => setTimeout(r, 5));
  assert.equal(h.calls.find((c) => c.fn === 'relay:queueCommand')!.args.kind, 'slashCommands');
  h.subs.get('relay:commandState')!.at(-1)!.onUpdate({ state: 'done', result: [{ name: 'review', description: 'Review the diff' }] });
  assert.deepEqual(await list, { commands: [{ name: 'review', description: 'Review the diff' }] });
});

test('an older backend says buttons and slash commands need it updated, and queues nothing', async () => {
  const h = harness({ apiVersion: 67 });
  await assert.rejects(h.relay.call(T, 'buttons.press', { threadId: T, buttonId: 'abcd1234' }), /backend is updated/);
  await assert.rejects(h.relay.call(T, 'threads.commands', { threadId: T }), /backend is updated/);
  assert.deepEqual(await h.relay.call(T, 'buttons.list', { threadId: T }), { buttons: [] });
  assert.equal(h.calls.filter((c) => c.fn === 'relay:queueCommand').length, 0);
});

// The done card's loose ends ride on the thread's row in the remote list
// (overlay), for the relay's own turn.completed seq, and only when the row
// says they're from that turn: a newer turn's row can come before its
// messages, and its loose ends aren't the older card's (Codex's review, 12 Nov).
test("a thread's loose ends go on its row in the list, for its own turn's card only", async () => {
  const ends = [{ kind: 'scheduled', text: 'Look at how the experiment went', at: '2026-11-15T01:30:00.000Z' }];
  const doneRows: RelayRow[] = [
    { seq: 3, role: 'user', text: 'Check it in', turn: 'aaaa1111', at: at(0) },
    { seq: 6, role: 'assistant', text: 'Checked it in.', turn: 'aaaa1111', at: at(2) },
  ];
  for (const [cardTurn, shown] of [['aaaa1111', true], ['bbbb2222', false], [undefined, false]] as const) {
    const h = harness({ rows: doneRows, row: () => ({ ...idle, looseEnds: ends, ...(cardTurn ? { cardTurn } : {}) }) });
    const { events } = (await h.relay.call(T, 'threads.events', { threadId: T })) as { events: { seq: number; type: string }[] };
    const end = events.filter((e) => e.type === 'turn.completed').at(-1)!;
    const listed = h.relay.overlay(h.remote).threads.find((t) => t.threadId === T) as { looseEnds?: { seq: number; ends: unknown[] } };
    if (shown) assert.deepEqual(listed.looseEnds, { seq: end.seq, ends }, String(cardTurn));
    else assert.equal(listed.looseEnds, undefined, String(cardTurn));
  }
});

// A plan answered while this Mac wasn't looking, with more than a tail's
// rows since: its decision is on a row the tail doesn't cover, so it's read
// again (Codex's review, 12 Nov).
test("a waiting plan older than the tail is read again, and its decision reaches the app", async () => {
  const plan: RelayRow = { seq: 4, role: 'assistant', text: '# Bots', plan: true, turn: 'aaaa1111', at: at(1) };
  const held = [{ seq: 2, role: 'user' as const, text: 'Plan it', turn: 'aaaa1111', at: at(0) }, plan];
  const backend = { rows: held as RelayRow[] };
  const h = harness({ rows: backend.rows });
  h.subscribed.add(T);
  await h.relay.call(T, 'threads.subscribe', { threadId: T, afterSeq: 0 });
  // Answered there: its row now says so, and twenty rows have come since.
  backend.rows.splice(0, backend.rows.length, { ...plan, planDecision: 'go' });
  const tail: RelayRow[] = Array.from({ length: 15 }, (_, i) => ({ seq: 30 + i, role: 'tool' as const, text: `step ${i}`, toolKind: 'read', turn: 'aaaa1111', at: at(10 + i) }));
  const sub = h.subs.get('relay:remoteTail')![0]!;
  sub.onUpdate({ messages: tail, hasMore: false });
  await new Promise((r) => setTimeout(r, 10));
  const sent = (h.published as { method: string; params: { event?: { type: string; decision?: string } } }[]).map((n) => n.params.event).filter(Boolean);
  assert.ok(sent.some((e) => e!.type === 'plan.resolved' && e!.decision === 'go'), JSON.stringify(sent.map((e) => e!.type)));
});

// Daybreak 12 Nov (finding 105): another runner can write tens of thousands
// of tiny plan rows to its own thread. Once this Mac had the thread open, a
// refresh read them all, held them all, and rebuilt every event (each plan
// copying every row after it) on each read, taking this runner's event loop
// for seconds per call. Now a watch holds MAX_HELD rows at most, the newest,
// and a read of a page is quick however many came.
test("a flood of another runner's rows is held to the newest few thousand, and reading stays quick", async () => {
  const backend: RelayRow[] = [{ seq: 1, role: 'user', text: 'Plan it', turn: 'aaaa1111', at: at(0) }];
  const h = harness({ paged: true, rows: backend });
  await h.relay.call(T, 'threads.events', { threadId: T, limit: 1 });
  for (let i = 0; i < 40_000; i++) backend.push({ seq: 2 + i, role: 'assistant', text: 'p', plan: true, turn: 'aaaa1111', at: at(1) });
  // Not subscribed and read over two seconds ago: the next read refreshes.
  h.clock.advance(3000);
  const first = (await h.relay.call(T, 'threads.events', { threadId: T, limit: 1 })) as { events: { seq: number; type: string }[] };
  assert.ok(h.relay.status().held <= MAX_HELD + 1000, `held ${h.relay.status().held}`);
  // The newest row's is what's shown (its turn's end, as the thread is idle).
  assert.ok(first.events[0]!.seq >= 40_001 * SCALE, `seq ${first.events[0]!.seq}`);
  // A read of what's held, as each one after it is.
  const before = process.cpuUsage();
  await h.relay.call(T, 'threads.events', { threadId: T, limit: 1 });
  const d = process.cpuUsage(before);
  const ms = (d.user + d.system) / 1000;
  assert.ok(ms < 100, `a read took ${ms.toFixed(1)} ms of CPU`);
  // Scrolling back reads what was dropped again.
  const older = (await h.relay.call(T, 'threads.events', { threadId: T, beforeSeq: first.events[0]!.seq, limit: 200 })) as { events: unknown[] };
  assert.ok(older.events.length > 0);
});

// Codex's review of that fix: trimming dropped every seq the app had been
// given, and emit, seeing none, sent nothing more; scrolling back held rows
// past the cap; and each new card another runner put on its row stayed,
// each costing a copy of the rows on every read.
test('after a gap bigger than the cap is filled, new rows still reach the app', async () => {
  const backend: RelayRow[] = [{ seq: 1, role: 'user', text: 'Go', turn: 'aaaa1111', at: at(0) }];
  const h = harness({ paged: true, rows: backend, row: () => ({ runnerId: 'bruce', status: 'running' }) });
  h.subscribed.add(T);
  await h.relay.call(T, 'threads.subscribe', { threadId: T, afterSeq: 0 });
  for (let i = 0; i < 8_000; i++) backend.push({ seq: 2 + i, role: 'assistant', text: 'p', turn: 'aaaa1111', at: at(1) });
  const tail = h.subs.get('relay:remoteTail')![0]!;
  tail.onUpdate({ messages: backend.slice(-15), hasMore: true });
  await new Promise((r) => setTimeout(r, 50));
  assert.ok(h.relay.status().held <= MAX_HELD + 1000, `held ${h.relay.status().held}`);
  const count = h.published.length;
  assert.ok(count > 0, 'the filled rows went out');
  backend.push({ seq: 9_000, role: 'user', text: 'Still here', turn: 'bbbb2222', at: at(5) });
  tail.onUpdate({ messages: backend.slice(-15), hasMore: true });
  await new Promise((r) => setTimeout(r, 20));
  const sent = h.published.slice(count).map((n) => (n as { params: { event: { type: string; text?: string } } }).params.event);
  assert.ok(sent.some((e) => e.type === 'user.message' && e.text === 'Still here'), 'the next row still reaches the app');
});

test('scrolling back stops holding more once twice the cap is held', async () => {
  const backend: RelayRow[] = Array.from({ length: 30_000 }, (_, i) => ({ seq: 1 + i, role: 'assistant' as const, text: 'p', turn: 'aaaa1111', at: at(1) }));
  const h = harness({ paged: true, rows: backend });
  let page = (await h.relay.call(T, 'threads.events', { threadId: T, limit: 200 })) as { events: { seq: number }[] };
  for (let n = 0; n < 200 && page.events.length; n++) {
    page = (await h.relay.call(T, 'threads.events', { threadId: T, beforeSeq: page.events[0]!.seq, limit: 2000 })) as { events: { seq: number }[] };
  }
  assert.ok(h.relay.status().held <= 2 * MAX_HELD + 1000, `held ${h.relay.status().held}`);
});

test("cards another runner keeps putting on its row are held to the newest few hundred", async () => {
  let n = 0;
  const h = harness({ row: () => ({ runnerId: 'bruce', status: 'waiting', pendingApproval: { approvalId: `ap${n}`, kind: 'approval', name: 'Bash', title: 'Run', options: ['allow', 'deny'] } }) });
  h.subscribed.add(T);
  await h.relay.call(T, 'threads.subscribe', { threadId: T, afterSeq: 0 });
  const row = h.subs.get('relay:remoteThread')![0]!;
  for (n = 0; n < 1_000; n++) row.onUpdate({ runnerId: 'bruce', status: 'waiting', pendingApproval: { approvalId: `ap${n}`, kind: 'approval', seq: 14, name: 'Bash', title: 'Run', options: ['allow', 'deny'] } });
  // With rising seqs too, so each goes out to the app (Codex's second
  // review): what's been given stays as small as the cards held.
  for (; n < 2_000; n++) {
    row.onUpdate({ runnerId: 'bruce', status: 'waiting', pendingApproval: { approvalId: `ap${n}`, kind: 'approval', seq: 100 + n, name: 'Bash', title: 'Run', options: ['allow', 'deny'] } });
    await new Promise((r) => setImmediate(r));
  }
  n = 1_999;
  const given = (h.relay as unknown as { watches: Map<string, { given: Set<number> }> }).watches.get(T)!.given;
  assert.ok(given.size <= 2 * MAX_CARDS + 50, `${given.size} seqs given held`);
  const page = (await h.relay.call(T, 'threads.events', { threadId: T, limit: 2000 })) as { events: { type: string; approvalId?: string }[] };
  const cards = page.events.filter((e) => e.type === 'approval.requested');
  assert.ok(cards.length <= MAX_CARDS, `${cards.length} cards`);
  assert.ok(cards.some((e) => e.approvalId === 'ap1999'), 'the open one stays');
});

// A watch started in the moment before this runner's first heartbeat (its
// cached backend version still the old one) asked for no login notes, and a
// live query keeps the arguments it was started with: the thread would go on
// showing none until it was closed and opened again (Codex's review, 13 Nov).
test('a watch from before the backend had login notes asks for them once it does', async () => {
  const opts: HarnessOpts = { apiVersion: 77 };
  const h = harness(opts);
  await h.relay.call(T, 'threads.subscribe', { threadId: T, afterSeq: 0 });
  const tails = () => h.subs.get('relay:remoteTail')!;
  assert.equal(tails().length, 1);
  assert.equal('notes' in tails()[0]!.args, false, 'an older backend refuses an argument it does not know');
  const page = await h.relay.call(T, 'threads.events', { threadId: T });
  assert.ok(page);
  assert.ok(h.calls.filter((c) => c.fn === 'relay:remoteMessages').every((c) => !('notes' in c.args)));
  opts.apiVersion = 78;
  tails()[0]!.onUpdate({ messages: rows.slice(-2), hasMore: true });
  await new Promise((r) => setImmediate(r));
  assert.equal(tails().length, 2, 'watched again');
  assert.equal(tails()[0]!.closed, true);
  assert.equal(tails()[1]!.args.notes, true);
  assert.equal(h.subs.get('relay:remoteThread')!.at(-1)!.closed, false);
  // The note in the new watch's tail shows, and nothing watches a third time.
  const moved: RelayRow = { seq: 15, role: 'note', text: 'Moved to mike@example.org: mike@example.net hit its 5-hour limit.', at: at(30) };
  tails()[1]!.onUpdate({ messages: [...rows.slice(-2), moved], hasMore: true });
  await new Promise((r) => setImmediate(r));
  assert.equal(tails().length, 2);
  const { events } = (await h.relay.call(T, 'threads.events', { threadId: T })) as { events: { type: string; text?: string; login?: boolean }[] };
  const note = events.find((e) => e.type === 'system.note');
  assert.equal(note?.text, moved.text);
  assert.equal(note?.login, true);
});

// Daybreak 113: the RPC server checks a send's runnerId against this, so it
// must be the runner `call` really sends to.
test('ownerOf names the runner a request goes to, following a move', async () => {
  const h = harness();
  assert.equal(h.relay.ownerOf(T), 'bruce');
  assert.equal(h.relay.ownerOf('not-listed'), undefined);
  await h.relay.call(T, 'threads.events', { threadId: T });
  h.remote.runners.push({ id: 'cloud', name: 'cloud', online: true, lastSeenAt: at(0) });
  h.remote.threads[0] = { ...h.remote.threads[0]!, runnerId: 'cloud' };
  assert.equal(h.relay.ownerOf(T), 'cloud');
  // And the open watch sends there too, not to the runner it was opened for.
  void h.relay.call(T, 'threads.interrupt', { threadId: T }).catch(() => {});
  await patiently(() => h.calls.some((c) => c.fn === 'relay:queueCommand'));
  assert.equal(h.calls.find((c) => c.fn === 'relay:queueCommand')?.args.runnerId, 'cloud');
  h.remote.threads.length = 0;
  assert.equal(h.relay.ownerOf(T), undefined);
});
