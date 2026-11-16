// A turn that failed because the API couldn't be reached (errorKind
// network): the thread waits, probes with backoff, and carries on by
// itself once the API answers (Runner.onNetworkLost). Other failures
// never retry; a message from Alex ends the wait; it outlives a restart;
// and a carry-on that can't get through either gives up after five.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { makeHarness, tick } from '../helpers/fakes.ts';
import { checkSummary } from '../helpers/protocol-check.ts';
import { CARRY_ON_NETWORK } from '../../src/runner.ts';
import type { Provider } from '../../src/protocol.ts';

const DOWN = "API Error: Can't reach the API server — check your internet or DNS (ENOTFOUND)";
const lose = (s: { result: (...a: any[]) => void }) => s.result('failed', undefined, DOWN, { errorKind: 'network' });

// A harness whose API is up or down as `net.up` says, counting probes.
function netHarness(home?: string, net = { up: false, probes: [] as Provider[] }) {
  const h = makeHarness(home, {}, { reachable: async (p) => (net.probes.push(p), net.up) });
  return { ...h, net, again: () => netHarness(h.home, net) };
}

const notes = (h: { events(id: string): any[] }, id: string): string[] => h.events(id).filter((e) => e.type === 'system.note').map((e) => e.text);
const runnerMessages = (h: { events(id: string): any[] }, id: string) => h.events(id).filter((e) => e.type === 'user.message' && e.origin === 'runner');

test('a turn that lost the API waits, probes with backoff, and carries on once it answers', async () => {
  const h = netHarness();
  const { thread } = h.runner.create({ text: 'build the thing' });
  const s = h.adapter.last;
  s.init();
  lose(s);
  await tick();
  const store = () => h.store.load(thread.id)!;
  assert.equal(store().waitingForNetwork?.tries, 0);
  // Clients see it, so the row can say it's waiting rather than Failed.
  const summary = h.runner.get(thread.id);
  assert.deepEqual(checkSummary(summary), []);
  assert.equal(summary.waitingForNetwork?.since, store().waitingForNetwork?.since);
  assert.deepEqual(notes(h, thread.id), ["Lost the connection to the API partway through that turn. This thread carries on by itself once it's back."]);
  // Probes at 5s, then 10s after that, while it's still down.
  h.clock.advance(4_000);
  await tick();
  assert.equal(h.net.probes.length, 0);
  h.clock.advance(1_000);
  await tick();
  assert.deepEqual(h.net.probes, ['claude']);
  h.clock.advance(9_000);
  await tick();
  assert.equal(h.net.probes.length, 1, 'backed off');
  h.net.up = true;
  h.clock.advance(1_000);
  await tick();
  assert.equal(h.net.probes.length, 2);
  // Carried on in the same session, as the runner's own message.
  assert.equal(h.adapter.sessions.length, 1);
  assert.equal(s.sent.at(-1)?.text, CARRY_ON_NETWORK);
  assert.equal(runnerMessages(h, thread.id).length, 1);
  assert.equal(store().waitingForNetwork, undefined);
  assert.equal(h.runner.get(thread.id).waitingForNetwork, undefined);
  assert.equal(store().netCarryOn?.tries, 1);
  // It finishes: nothing more happens, and the count is gone with the next turn.
  s.result('completed');
  await tick();
  h.clock.advance(120_000);
  await tick();
  assert.equal(runnerMessages(h, thread.id).length, 1);
  assert.equal(h.net.probes.length, 2, 'no probing with nothing waiting');
});

test('other failures never carry on by themselves', async () => {
  const h = netHarness();
  h.net.up = true;
  for (const [error, extra] of [
    ['API Error: 500 Internal server error', {}],
    ["You've hit your usage limit", { errorKind: 'usageLimit' }],
    ['Failed to authenticate', { errorKind: 'auth' }],
  ] as const) {
    const { thread } = h.runner.create({ text: 'x' });
    const s = h.adapter.last;
    s.init();
    s.result('failed', undefined, error, extra);
    await tick();
    assert.equal(h.store.load(thread.id)!.waitingForNetwork, undefined, error);
  }
  h.clock.advance(600_000);
  await tick();
  assert.equal(h.net.probes.length, 0);
  // An interrupted turn isn't a failure either.
  const { thread } = h.runner.create({ text: 'y' });
  h.adapter.last.init();
  h.adapter.last.result('interrupted', undefined, DOWN, { errorKind: 'network' });
  await tick();
  assert.equal(h.store.load(thread.id)!.waitingForNetwork, undefined);
});

test('a message from Alex ends the wait; one already queued goes instead of a carry-on', async () => {
  const h = netHarness();
  const { thread } = h.runner.create({ text: 'x' });
  const s = h.adapter.last;
  s.init();
  lose(s);
  await tick();
  h.runner.send(thread.id, 'sorry, internet went down, continue');
  await tick();
  assert.equal(h.store.load(thread.id)!.waitingForNetwork, undefined);
  h.net.up = true;
  h.clock.advance(60_000);
  await tick();
  assert.equal(runnerMessages(h, thread.id).length, 0);
  assert.equal(s.sent.at(-1)?.text, 'sorry, internet went down, continue');
  // Queued while the failing turn ran: that message goes, no wait.
  s.result('completed');
  await tick();
  h.runner.send(thread.id, 'next');
  h.runner.send(thread.id, 'and then this');
  await tick();
  lose(s);
  await tick();
  assert.equal(h.store.load(thread.id)!.waitingForNetwork, undefined);
  assert.equal(s.sent.at(-1)?.text, 'and then this');
});

test('a carry-on that cannot get through either waits again, then gives up after five', async () => {
  const h = netHarness();
  const { thread } = h.runner.create({ text: 'x' });
  const s = h.adapter.last;
  s.init();
  lose(s);
  await tick();
  // The probe says yes but the CLI still can't get through, five times.
  h.net.up = true;
  for (let i = 1; i <= 5; i++) {
    h.clock.advance(5_000);
    await tick();
    assert.equal(s.sent.at(-1)?.text, CARRY_ON_NETWORK, `carry-on ${i}`);
    assert.equal(h.store.load(thread.id)!.netCarryOn?.tries, i);
    lose(s);
    await tick();
  }
  assert.equal(runnerMessages(h, thread.id).length, 5);
  assert.equal(h.store.load(thread.id)!.waitingForNetwork, undefined);
  const said = notes(h, thread.id);
  assert.equal(said.filter((n) => n.startsWith('Lost the connection')).length, 1, 'one note for the outage');
  assert.equal(said.at(-1), "Still can't reach the API, so this thread has stopped trying by itself. Send a message to carry on.");
  h.clock.advance(600_000);
  await tick();
  assert.equal(runnerMessages(h, thread.id).length, 5);
});

test('the wait outlives a restart', async () => {
  const h = netHarness();
  const { thread } = h.runner.create({ text: 'x' });
  h.adapter.last.init();
  lose(h.adapter.last);
  await tick();
  await h.runner.shutdown();
  const h2 = h.again();
  assert.equal(h2.store.load(thread.id)!.waitingForNetwork?.tries, 0);
  h2.net.up = true;
  h2.clock.advance(5_000);
  await tick();
  assert.equal(h2.adapter.sessions.length, 1);
  assert.equal(h2.adapter.last.sent[0].text, CARRY_ON_NETWORK);
  assert.equal(h2.adapter.last.opts.resume, true);
});

test('a turn the CLI opens by itself during the wait is part of the same outage', async () => {
  const h = netHarness();
  const { thread } = h.runner.create({ text: 'x' });
  const s = h.adapter.last;
  s.init();
  lose(s);
  await tick();
  // A background job ends while the line's still down: that turn fails too.
  s.out({ kind: 'cliTurn', note: 'Background command "sleep 30" completed (exit code 0)' });
  lose(s);
  await tick();
  assert.equal(notes(h, thread.id).filter((n) => n.startsWith('Lost the connection')).length, 1, 'no second note');
  assert.equal(h.store.load(thread.id)!.waitingForNetwork?.tries, 0);
  // Another wakes it once the line is back and gets through: no carry-on.
  s.out({ kind: 'cliTurn', note: 'Background command "npm test" completed (exit code 0)' });
  s.result('completed');
  await tick();
  assert.equal(h.store.load(thread.id)!.waitingForNetwork, undefined);
  h.net.up = true;
  h.clock.advance(120_000);
  await tick();
  assert.equal(runnerMessages(h, thread.id).length, 0);
});
