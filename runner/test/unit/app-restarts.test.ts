import { test } from 'node:test';
import assert from 'node:assert/strict';
import { makeHarness } from '../helpers/fakes.ts';
import { AppRestarts, asksForAppRestart } from '../../src/app-restarts.ts';
import { nullLogger } from '../../src/log.ts';

const A = { commit: 'aaaa1111', short: 'aaaa111', tree: 'tree-a' };
const B = { commit: 'bbbb2222', short: 'bbbb222', tree: 'tree-b' };

function setUp() {
  const h = makeHarness();
  const watcher = new AppRestarts({ runner: h.runner, home: h.home, clock: h.clock, log: nullLogger });
  const { thread } = h.runner.create({ text: 'Build the dock', accountId: 'claude-3' });
  h.adapter.last.init();
  return { h, watcher, thread };
}

const said = (h: ReturnType<typeof makeHarness>, threadId: string) => {
  const evs = h.events(threadId);
  return {
    note: evs.findLast((e) => e.type === 'system.note' && String(e.text).startsWith('Threadbear app restarted'))?.text,
    message: evs.findLast((e) => e.type === 'user.message' && e.origin === 'runner')?.text,
  };
};

test('a thread waiting hears when the app comes back on another build', () => {
  const { h, watcher, thread } = setUp();
  watcher.appHello(A);
  watcher.watch(thread.id);
  // The same build again (the app reconnecting to a restarted runner): nothing.
  watcher.appHello(A);
  assert.equal(said(h, thread.id).message, undefined);
  watcher.appHello(B);
  const { note, message } = said(h, thread.id);
  assert.equal(note, 'Threadbear app restarted: aaaa111 to bbbb222');
  assert.match(message, /restarted onto bbbb222/);
  // Once only.
  watcher.appHello({ ...A, tree: 'tree-c' });
  assert.equal(h.events(thread.id).filter((e) => e.type === 'user.message' && e.origin === 'runner').length, 1);
});

test('the agent\'s own text is sent, and the last request wins', () => {
  const { h, watcher, thread } = setUp();
  watcher.appHello(A);
  watcher.watch(thread.id, 'Check the dock in the real app.');
  watcher.watch(thread.id, 'Check the panels move.');
  watcher.appHello(B);
  assert.match(said(h, thread.id).message, /Check the panels move\.$/);
});

test('remembered across a runner restart', () => {
  const { h, watcher, thread } = setUp();
  watcher.appHello(A);
  watcher.watch(thread.id);
  const again = new AppRestarts({ runner: h.runner, home: h.home, clock: h.clock, log: nullLogger });
  again.appHello(A);
  assert.equal(said(h, thread.id).message, undefined);
  again.appHello(B);
  assert.match(said(h, thread.id).message, /restarted onto bbbb222/);
});

test('an archived thread or a stale request only gets the note', () => {
  const { h, watcher, thread } = setUp();
  watcher.appHello(A);
  watcher.watch(thread.id);
  h.clock.advance(25 * 60 * 60_000);
  watcher.appHello(B);
  assert.equal(said(h, thread.id).message, undefined);
  assert.match(said(h, thread.id).note, /more than a day old/);
});

test('a thread on a draining runner hears once it comes across', () => {
  // THREADBEAR-5: the app came back while the thread was finishing a turn
  // on the old runner; the primary said "Thread not found" and dropped
  // the request.
  const { h, thread } = setUp();
  const warned: string[] = [];
  // The primary as the watcher sees it: the thread is on a draining
  // runner, so this one hasn't got it.
  const away = new Set<string>();
  const primary = { has: (id: string) => !away.has(id) && h.runner.has(id), get: h.runner.get.bind(h.runner), note: h.runner.note.bind(h.runner), send: h.runner.send.bind(h.runner) };
  const watcher = new AppRestarts({ runner: primary, home: h.home, clock: h.clock, log: { ...nullLogger, warn: (m) => void warned.push(m) }, away: (id) => away.has(id) });
  watcher.appHello(A);
  watcher.watch(thread.id);
  h.adapter.last.result('completed');
  away.add(thread.id);
  watcher.appHello(B);
  assert.deepEqual(warned, []);
  assert.equal(said(h, thread.id).note, undefined);
  assert.equal(watcher.isWatching(thread.id), true, 'still waiting to tell it');
  // It comes across: now it hears, about the build the app came back on.
  away.delete(thread.id);
  watcher.adopted(thread.id);
  assert.deepEqual(warned, []);
  assert.match(said(h, thread.id).message, /restarted onto bbbb222/);
  assert.equal(said(h, thread.id).note, 'Threadbear app restarted: aaaa111 to bbbb222');
  assert.equal(watcher.isWatching(thread.id), false);
  // Once only.
  watcher.adopted(thread.id);
  assert.equal(h.events(thread.id).filter((e) => e.type === 'user.message' && e.origin === 'runner').length, 1);
});

test('a deleted thread is dropped quietly', () => {
  const { h, thread } = setUp();
  const warned: string[] = [];
  const watcher = new AppRestarts({ runner: h.runner, home: h.home, clock: h.clock, log: { ...nullLogger, warn: (m) => void warned.push(m) }, away: () => false });
  watcher.appHello(A);
  watcher.watch(thread.id);
  h.runner.delete(thread.id);
  watcher.appHello(B);
  assert.deepEqual(warned, []);
  assert.equal(watcher.isWatching(thread.id), false);
});

test('asks that tell Mike to restart the app count as a request', () => {
  assert.equal(asksForAppRestart([{ action: 'Click Restart to Update in the Threadbear menu' }]), true);
  assert.equal(asksForAppRestart([{ action: 'Restart Threadbear', where: 'Threadbear menu > Restart to Update' }]), true);
  // The ask is one paragraph now, so the words can be in its why.
  assert.equal(asksForAppRestart([{ action: 'Try the new card', why: 'It needs Restart to Update in the Threadbear menu first.' }]), true);
  assert.equal(asksForAppRestart([{ action: 'Restart the runner' }]), false);
  assert.equal(asksForAppRestart(undefined), false);
});

test('end_turn asking for Restart to Update signs the thread up; wait_for_app_restart sets the text', async () => {
  const { buildThreadbearTools } = await import('../../src/threadbear-mcp.ts');
  const { h, watcher, thread } = setUp();
  watcher.appHello(A);
  const tools = buildThreadbearTools(h.runner, thread.id, Date.now, undefined, undefined, watcher);
  const call = async (name: string, args: Record<string, unknown>) => {
    const t = tools.find((x) => x.name === name)!;
    const r = (await t.handler(args as never, {})) as { content: { text: string }[] };
    return r.content.map((c) => c.text).join('\n');
  };
  const reply = await call('end_turn', { needs: 'you', asks: [{ action: 'Click Restart to Update', why: 'To check the fix.' }] });
  assert.match(reply, /message here when the app restarts/);
  assert.equal(watcher.isWatching(thread.id), true);
  // Asks about something else don't.
  const { thread: other } = h.runner.create({ text: 'Other', accountId: 'claude-3' });
  const otherTools = buildThreadbearTools(h.runner, other.id, Date.now, undefined, undefined, watcher);
  await otherTools.find((x) => x.name === 'end_turn')!.handler({ needs: 'you', asks: [{ action: 'Paste the key', why: 'The deploy needs it.' }] } as never, {});
  assert.equal(watcher.isWatching(other.id), false);
  // Every ask has to say why: a card with only an action and a place
  // left Mike guessing (28 Oct). The schema refuses one without.
  const shape = (buildThreadbearTools(h.runner, thread.id, Date.now, undefined, undefined, watcher, { provider: 'codex' }).find((x) => x.name === 'end_turn')!.inputSchema as any).asks;
  assert.equal(shape.safeParse([{ action: 'Fix the Slack draft', where: 'Slack' }]).success, false);
  assert.equal(shape.safeParse([{ action: 'Fix the Slack draft', why: 'It says Hayden; he is Pete.', links: [{ title: 'Open the Slack message', url: 'https://x.slack.com/archives/C1/p1' }] }]).success, true);
  // The tool's text is what's sent.
  assert.match(await call('wait_for_app_restart', { text: 'Drag Highlights about.' }), /aaaa111/);
  watcher.appHello(B);
  assert.match(said(h, thread.id).message, /Drag Highlights about\.$/);
});

test('a turn whose end_turn was refused still signs up from its closing "What I need from you"', () => {
  // The thread that asked Mike to reply "restarted" instead (29 Oct).
  const asked: string[] = [];
  const h = makeHarness(undefined, {}, { appRestartAsked: (id) => asked.push(id) });
  const { thread } = h.runner.create({ text: 'Share a thread', accountId: 'claude-3' });
  const s = h.adapter.last;
  s.init();
  s.text('m1', 'Landed.\n\n**What I need from you**\n\n1. Choose Restart to Update from the Threadbear menu, then reply "restarted".');
  s.result('completed');
  assert.deepEqual(asked, [thread.id]);
  // A turn asking for something else doesn't.
  h.runner.send(thread.id, 'ok');
  h.adapter.last.text('m2', 'Done.\n\n**What I need from you**\n\n1. Paste the key.');
  h.adapter.last.result('completed');
  assert.deepEqual(asked, [thread.id]);
});

const D1 = { commit: 'dddd1111', short: 'dddd111', tree: 'desktop-1' };
const D2 = { commit: 'dddd2222', short: 'dddd222', tree: 'desktop-2' };

test('the GPUI app restarting is told apart from the Swift app', () => {
  const { h, watcher, thread } = setUp();
  watcher.appHello(A);
  watcher.appHello(D1, 'desktop');
  watcher.watch(thread.id);
  // Each reconnecting on the build it had: nothing.
  watcher.appHello(A);
  watcher.appHello(D1, 'desktop');
  assert.equal(said(h, thread.id).message, undefined);
  watcher.appHello(D2, 'desktop');
  const evs = h.events(thread.id);
  assert.equal(evs.findLast((e) => e.type === 'system.note')?.text, 'Threadbear GPUI app restarted: dddd111 to dddd222');
  assert.match(String(evs.findLast((e) => e.type === 'user.message' && e.origin === 'runner')?.text), /The Threadbear GPUI app has restarted onto dddd222/);
  assert.equal(watcher.isWatching(thread.id), false);
});

test("the GPUI app's first build isn't a restart of the Swift app a thread waits for", () => {
  // Before the GPUI app said its build, a thread only knew the Swift app's.
  const { h, watcher, thread } = setUp();
  watcher.appHello(A);
  watcher.watch(thread.id);
  watcher.appHello(D1, 'desktop');
  assert.equal(said(h, thread.id).message, undefined);
  assert.equal(watcher.isWatching(thread.id), true);
  watcher.appHello(B);
  assert.match(said(h, thread.id).message, /restarted onto bbbb222/);
});

test('with no build known at all, either app coming back counts', () => {
  const { h, watcher, thread } = setUp();
  watcher.watch(thread.id);
  watcher.appHello(D1, 'desktop');
  assert.match(String(h.events(thread.id).findLast((e) => e.type === 'user.message' && e.origin === 'runner')?.text), /GPUI app has restarted onto dddd111/);
});
