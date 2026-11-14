// No half emoji reaches the apps. 14 Nov 2026: a Bash tool title cut through
// 🧵 kept its first UTF-16 half, JSON.stringify sent it as "\ud83e", the GPUI
// app's serde_json refused the whole threads.events reply and dropped it, and
// the thread ("Threadbear Shorts and things that make sense series") never
// opened: every try timed out after 120 s.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { RpcServer } from '../../src/rpc.ts';
import { RunnerClient } from '../../src/client.ts';
import { nullLogger } from '../../src/log.ts';
import { makeHarness } from '../helpers/fakes.ts';
import { wellFormedJson } from '../../src/well-formed-json.ts';
import { cutSafely, safeEnd, safeStart, tailSafely } from '../../src/safe-cut.ts';
import { toolTitle } from '../../src/tools.ts';
import { convexPoster } from '../../src/sync.ts';

const THREAD = '\u{1F9F5}'; // 🧵, two UTF-16 halves
const HIGH = THREAD[0];
const LOW = THREAD[1];

// What serde_json and Swift refuse: a surrogate escape that isn't itself an
// escaped backslash's text.
function hasLoneEscape(json: string): boolean {
  return /(?<!\\)(?:\\\\)*\\ud[89a-f][0-9a-f]{2}/i.test(json);
}

test('wellFormedJson turns a lone half into U+FFFD and leaves everything else as it was', () => {
  const sent = JSON.stringify({ title: `make sense in ${HIGH}…`, low: `${LOW}x`, whole: `a ${THREAD} b` });
  assert.ok(hasLoneEscape(sent), 'JSON.stringify writes the lone halves as escapes');
  const fixed = wellFormedJson(sent);
  assert.ok(!hasLoneEscape(fixed));
  assert.deepEqual(JSON.parse(fixed), { title: 'make sense in \uFFFD…', low: '\uFFFDx', whole: `a ${THREAD} b` });

  // The text "\ud83e" (a backslash and five letters) is not an escape.
  for (const text of ['\\ud83e', '\\\\ud83e', 'C:\\udata', `\\${HIGH}`]) {
    const json = JSON.stringify({ text });
    const back = JSON.parse(wellFormedJson(json)).text;
    assert.equal(back, text.isWellFormed() ? text : text.toWellFormed(), JSON.stringify(text));
  }
  // An escaped backslash then a real lone half: the half still goes.
  const both = JSON.stringify({ t: `\\${HIGH}` });
  assert.equal(JSON.parse(wellFormedJson(both)).t, '\\\uFFFD');

  const plain = JSON.stringify({ a: 'nothing to do 🐻' });
  assert.equal(wellFormedJson(plain), plain);
});

test('cuts never split an emoji between its halves', () => {
  const text = 'a'.repeat(9) + THREAD + 'after';
  // 10 lands between the halves.
  assert.equal(safeEnd(text, 10), 9);
  assert.equal(safeEnd(text, 11), 11);
  assert.equal(safeStart(text, 10), 11);
  assert.equal(safeStart(text, 9), 9);
  for (let max = 1; max <= text.length; max++) {
    assert.ok(cutSafely(text, max, '…').isWellFormed(), `cutSafely at ${max}`);
    assert.ok(tailSafely(text, max, '…').isWellFormed(), `tailSafely at ${max}`);
  }
});

test("a long command's tool title cut at an emoji keeps it whole or leaves it out", () => {
  // The real one: printf of four X posts, the first starting "Things that
  // just make sense in 🧵🐻", its title cut at 100.
  for (let pad = 90; pad <= 100; pad++) {
    const command = 'printf "' + 'x'.repeat(pad) + `${THREAD}🐻 #11"`;
    const title = toolTitle('Bash', { command }, '/');
    assert.ok(title.isWellFormed(), `pad ${pad}: ${JSON.stringify(title)}`);
    assert.ok(!hasLoneEscape(JSON.stringify(title)));
  }
});

test('a transcript page with a lone half already saved reaches the app as JSON a strict parser reads', async () => {
  const h = makeHarness();
  const token = 'well-formed-token';
  const server = new RpcServer({ host: '127.0.0.1', port: 0, token, runner: h.runner, log: nullLogger });
  const { port } = await server.listen();
  const c = await RunnerClient.connect(`ws://127.0.0.1:${port}/rpc`, token);
  try {
    const frames: string[] = [];
    (c as unknown as { ws: { on(e: 'message', f: (d: Buffer) => void): void } }).ws.on('message', (d) => frames.push(d.toString()));
    // Saved with the half in it, as the old title cut left one on disk.
    const { thread } = await c.call('threads.create', { text: `Things that just make sense in ${HIGH}…`, accountId: 'claude-3' });
    const page = await c.call('threads.events', { threadId: thread.id });
    assert.ok(JSON.stringify(page).includes('make sense in \uFFFD…'), 'the half became U+FFFD');
    assert.ok(frames.length > 0);
    for (const f of frames) assert.ok(!hasLoneEscape(f), `a frame still had a lone half: ${f.slice(0, 200)}`);
  } finally {
    c.close();
    await server.close();
  }
});

// Codex's review of the first fix: sync to Convex has cuts of its own (a
// message at 8,000, a tool line at 200), and its posts didn't go through the
// socket's repair.
test('what goes to Convex has no lone half either', async () => {
  const bodies: string[] = [];
  const fetchFn = (async (_url: string, init: RequestInit) => {
    bodies.push(init.body as string);
    return { ok: true, status: 200, json: async () => ({ status: 'success', value: null }) } as Response;
  }) as unknown as typeof fetch;
  await convexPoster('https://x.convex.cloud', fetchFn)('mutation', 'sync:push', { text: `${'a'.repeat(7999)}${HIGH}`, whole: THREAD });
  assert.ok(!hasLoneEscape(bodies[0]!));
  assert.deepEqual(JSON.parse(bodies[0]!).args, { text: `${'a'.repeat(7999)}\uFFFD`, whole: THREAD });
});
