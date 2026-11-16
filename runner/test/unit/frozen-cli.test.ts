import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawn } from 'node:child_process';
import { makeHarness, tick } from '../helpers/fakes.ts';
import { nullLogger } from '../../src/log.ts';
import { FrozenClis, QUIET_MS, STOP_AFTER_MS, SWEEP_MS, fileCallInSample, parseProcs, realProbe, sampleSandbox, type FrozenProbe, type Proc } from '../../src/frozen-cli.ts';

class FakeProbe implements FrozenProbe {
  procs: Proc[] = [];
  stuck = new Set<number>();
  sampled: number[] = [];
  async processes() {
    return this.procs;
  }
  async stuckIn(pid: number) {
    this.sampled.push(pid);
    return this.stuck.has(pid) ? '__openat_nocancel' : undefined;
  }
}

function setUp() {
  const h = makeHarness();
  const probe = new FakeProbe();
  const check = new FrozenClis({ host: h.runner, clock: h.clock, log: nullLogger, machine: 'mac-mini', probe });
  const { thread } = h.runner.create({ text: 'Print the plans', accountId: 'claude-3' });
  const s = h.adapter.last;
  s.init();
  const sessionId = s.opts.sessionId;
  probe.procs = [{ pid: 100, command: `claude --output-format stream-json --session-id=${sessionId}` }];
  const notes = () => h.events(thread.id).filter((e) => e.type === 'system.note').map((e) => String(e.text));
  const summary = () => h.runner.list().find((t) => t.id === thread.id)!;
  const look = async (ms = SWEEP_MS) => {
    h.clock.advance(ms);
    await check.sweep();
  };
  return { h, probe, check, thread, s, notes, summary, look };
}

test('a frozen CLI is told about at the second look, and the thread needs you', async () => {
  const { probe, notes, summary, look } = setUp();
  probe.stuck.add(100);
  await look(QUIET_MS);
  assert.deepEqual(notes(), [], 'once could be a slow disk');
  await look();
  assert.equal(notes().length, 1);
  assert.match(notes()[0], /Claude Code is stuck opening a file on mac-mini/);
  assert.match(notes()[0], /permission prompt/);
  assert.equal(summary().needsYou, true);
  await look();
  assert.equal(notes().length, 1, 'said once');
});

test('a quiet CLI that is not stuck in a file call says nothing, and is sampled less and less', async () => {
  const { probe, notes, look, summary } = setUp();
  await look(QUIET_MS);
  for (let i = 0; i < 20; i++) await look();
  assert.deepEqual(notes(), []);
  assert.equal(summary().needsYou, false);
  assert.ok(probe.sampled.length <= 6, `sampled ${probe.sampled.length} times in 11 minutes`);
});

test('a turn that has said something lately is not looked at', async () => {
  const { h, probe, s, look } = setUp();
  h.clock.advance(QUIET_MS - 1000);
  s.delta('a', 'Thinking');
  await look(2000);
  assert.deepEqual(probe.sampled, []);
});

test('a turn waiting on an approval is not looked at', async () => {
  const { h, probe, s, look, thread } = setUp();
  const ac = new AbortController();
  void s.sink.requestApproval({ name: 'Bash', title: 'ls', input: {}, canAlwaysAllow: true, signal: ac.signal });
  await tick();
  assert.equal(h.runner.list().find((t) => t.id === thread.id)!.status, 'waiting');
  await look(QUIET_MS);
  assert.deepEqual(probe.sampled, []);
});

test('moving again after the note says so and leaves Needs you', async () => {
  const { h, probe, s, notes, summary, look } = setUp();
  probe.stuck.add(100);
  await look(QUIET_MS);
  await look();
  s.delta('a', 'Carrying on');
  h.clock.advance(1000);
  await look(1000);
  assert.match(notes().at(-1)!, /moving again/);
  assert.equal(summary().needsYou, false);
});

test('a CLI still frozen after the wait is stopped, the turn fails and the next message starts a new one', async () => {
  const { h, probe, s, thread, look } = setUp();
  probe.stuck.add(100);
  await look(QUIET_MS);
  await look();
  for (let t = 0; t < STOP_AFTER_MS; t += SWEEP_MS) await look();
  assert.equal(s.killed, true);
  const ends = h.events(thread.id).filter((e) => e.type === 'turn.completed');
  assert.equal(ends.at(-1)!.outcome, 'failed');
  assert.match(String(ends.at(-1)!.error), /stuck opening a file on mac-mini/);
  assert.equal(h.runner.list().find((t) => t.id === thread.id)!.needsYou, true);
  h.runner.send(thread.id, 'Try again', []);
  await tick();
  assert.notEqual(h.adapter.last, s);
});

test('stopping an interrupted turn kills a CLI known to be frozen, so the next message does not go to it', async () => {
  const { h, probe, s, thread, look } = setUp();
  probe.stuck.add(100);
  await look(QUIET_MS);
  await look();
  h.runner.interrupt(thread.id);
  // A look while the interrupt waits doesn't take it for moving again.
  await look(5000);
  h.clock.advance(5000);
  await tick();
  assert.equal(s.killed, true);
  assert.ok(!h.events(thread.id).some((e) => e.type === 'system.note' && /moving again/.test(String(e.text))));
});

// From the real sample on Bruce, 31 Oct 2026, with a live CLI's for contrast.
const FROZEN = `Call graph:
    1490 Thread_22917456   DispatchQueue_1: com.apple.main-thread  (serial)
    + 1490 start  (in dyld) + 6652  [0x190520d7c]
    +   1490 ???  (in 2.1.314)  load address 0x10455c000 + 0x130c42c  [0x10586842c]
    +     1490 openat$NOCANCEL  (in libsystem_kernel.dylib) + 76  [0x189eab9a0]
    +       1490 __openat_nocancel  (in libsystem_kernel.dylib) + 8  [0x189eab954]
    1490 Thread_22917457: mi-scavenger
    + 1490 thread_start  (in libsystem_pthread.dylib) + 8  [0x189ee4c1c]
`;
const HEALTHY = `Call graph:
    745 Thread_4612139   DispatchQueue_1: com.apple.main-thread  (serial)
    + 745 start  (in dyld) + 6652  [0x190520d7c]
    +   740 kevent64  (in libsystem_kernel.dylib) + 8  [0x189eab954]
    +   4 __open  (in libsystem_kernel.dylib) + 8  [0x189eab954]
    745 Thread_4612140: worker
    + 745 __open  (in libsystem_kernel.dylib) + 8  [0x189eab954]
`;

test('sample: stuck means the main thread is in a file call all the way through', () => {
  assert.equal(fileCallInSample(FROZEN), 'openat$NOCANCEL');
  assert.equal(fileCallInSample(HEALTHY), undefined);
  assert.equal(fileCallInSample('no such thing'), undefined);
});

test('ps lines parse', () => {
  assert.deepEqual(parseProcs('  12 /bin/zsh -c ls\n'), [{ pid: 12, command: '/bin/zsh -c ls' }]);
});

test('the real probe sees a process blocked in open', { skip: process.platform !== 'darwin' }, async () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'frozen-cli-'));
  const fifo = path.join(dir, 'f');
  // Opening a named pipe with no writer blocks just as a privacy prompt does.
  spawn('/usr/bin/mkfifo', [fifo], { stdio: 'ignore' });
  await new Promise((r) => setTimeout(r, 100));
  const cat = spawn('/bin/cat', [fifo], { stdio: 'ignore' });
  const sleeper = spawn('/bin/sleep', ['20'], { stdio: 'ignore' });
  try {
    await new Promise((r) => setTimeout(r, 200));
    const probe = realProbe();
    assert.match((await probe.stuckIn(cat.pid!)) ?? '', /open/);
    assert.equal(await probe.stuckIn(sleeper.pid!), undefined);
    assert.ok((await probe.processes()).some((p) => p.pid === cat.pid));
  } finally {
    cat.kill('SIGKILL');
    sleeper.kill('SIGKILL');
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

test('sample is kept away from what macOS asks about', () => {
  const profile = sampleSandbox('/Users/me');
  for (const p of ['/Volumes', '/Users/me/Downloads', '/Users/me/Documents', '/Users/me/Desktop', '/Users/me/Library/Mobile Documents']) {
    assert.ok(profile.includes(`(subpath "${p}")`), p);
  }
});
