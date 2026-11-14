// The checks the real-screen tools make just before each real pointer event
// (features/tools/real-screen.ts), against made-up screens. Nothing here
// opens a window, reads the real screen or posts an event.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { EDGE, guardInput, guardPath, guardWindowMove, realDrag, realGripDrag, type Post, type Screen } from '../../../features/tools/real-screen.ts';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../../..');
const APP = 100, ALERT = 200, OTHER = 300;
type Win = { pid: number; owner: string; frame: number[]; id?: number };
const covers = ({ frame: [fx, fy, fw, fh] }: Win, x: number, y: number) => x >= fx && x < fx + fw && y >= fy && y < fy + fh;

/** A 1920 x 1080 display with windows listed front to back. */
function screen(windows: Win[], opts: { recording?: string; displays?: number[][] } = {}): Screen {
  return {
    windowAt: (x, y) => {
      const w = windows.find((w) => covers(w, x, y));
      return w ? { pid: w.pid, owner: w.owner } : null;
    },
    windowFrame: (pid) => windows.find((w) => w.pid === pid)!.frame,
    windowsAbove: (pid, x, y) => {
      const at = windows.findIndex((w) => w.pid === pid);
      if (at < 0) return null;
      return windows.slice(0, at).filter((w) => covers(w, x, y)).map((w) => `${w.id ?? windows.indexOf(w)} ${w.owner}`);
    },
    displays: () => opts.displays ?? [[0, 0, 1920, 1080]],
    recording: () => opts.recording ?? '',
  };
}
const app: Win = { pid: APP, owner: 'Threadbear', frame: [300, 100, 1240, 820] };

test('a press and drop on the app pass', () => {
  guardInput(APP, [[400, 200], [1000, 600]], screen([app]));
});

test('anything recording stops it before the windows are looked at', () => {
  const s = screen([app], { recording: '123 ffmpeg -f avfoundation' });
  s.windowAt = () => assert.fail('looked at windows while recording');
  assert.throws(() => guardInput(APP, [[400, 200]], s), /recording.*No events were posted/);
});

test('a window that has come up over a later point stops it', () => {
  const alert: Win = { pid: ALERT, owner: 'Crash Reporter', frame: [900, 500, 300, 200] };
  assert.throws(() => guardInput(APP, [[400, 200], [1000, 600]], screen([alert, app])), /Crash Reporter is there/);
});

test('a point off the window, off screen or near an edge is refused', () => {
  const s = screen([app]);
  assert.throws(() => guardInput(APP, [[200, 200]], s), /nothing is there/);
  assert.throws(() => guardInput(APP, [[-10, 500]], s), /off screen/);
  // The window running up to the screen's left edge: a drag that ends on it,
  // but in the Dock's reveal strip, is still refused.
  const edge = screen([{ ...app, frame: [0, 100, 1240, 820] }]);
  assert.throws(() => guardInput(APP, [[EDGE - 1, 500]], edge), /within 24 points/);
  guardInput(APP, [[EDGE, 500]], edge);
  // A second display counts as screen.
  guardInput(APP, [[2100, 500]], screen([{ ...app, frame: [1950, 100, 800, 600] }], { displays: [[0, 0, 1920, 1080], [1920, 0, 1920, 1080]] }));
});

test('a grip drag may end off the window, where the window will be', () => {
  const s = screen([app]);
  // Away by 70 and 50 from the activity bar, then back by the same grip from
  // its new place, which ends left of where the window is then.
  guardWindowMove(APP, [324, 114], [394, 164], s);
  const moved = screen([{ ...app, frame: [370, 150, 1240, 820] }]);
  guardWindowMove(APP, [394, 164], [324, 114], moved);
});

test('a grip drag is refused when the grip is covered or off the window', () => {
  const alert: Win = { pid: ALERT, owner: 'Crash Reporter', frame: [300, 100, 200, 100] };
  assert.throws(() => guardWindowMove(APP, [324, 114], [394, 164], screen([alert, app])), /Crash Reporter is there/);
  const other: Win = { pid: OTHER, owner: 'Finder', frame: [0, 0, 300, 1080] };
  assert.throws(() => guardWindowMove(APP, [200, 114], [270, 164], screen([other, app])), /Finder is there/);
});

test('a grip drag is refused when its drop is over something covering the window now', () => {
  const alert: Win = { pid: ALERT, owner: 'Crash Reporter', frame: [380, 150, 100, 100] };
  assert.throws(() => guardWindowMove(APP, [324, 114], [394, 164], screen([alert, app])), /Crash Reporter is there at|Not in front at \(394, 164\)/);
});

test('a grip drag is refused near an edge or when it would take the window off its display', () => {
  const left = screen([{ ...app, frame: [40, 100, 1240, 820] }]);
  assert.throws(() => guardWindowMove(APP, [64, 114], [10, 114], left), /drop \(10, 114\) is off screen or within 24/);
  const right = screen([{ ...app, frame: [640, 100, 1240, 820] }]);
  assert.throws(() => guardWindowMove(APP, [1740, 114], [1810, 164], right), /off its display/);
  assert.throws(() => guardWindowMove(APP, [324, 114], [394, 164], screen([app], { recording: 'ffmpeg' })), /recording/);
});

test('a return drag ending off the window is refused when something is over its place there', () => {
  // The window moved right and down; the drag back ends left of it, where an
  // alert has come up over where the window was.
  const moved: Win = { ...app, frame: [370, 150, 1240, 820] };
  const alert: Win = { pid: ALERT, owner: 'Crash Reporter', frame: [300, 100, 60, 60] };
  assert.throws(() => guardWindowMove(APP, [394, 164], [324, 114], screen([alert, moved])), /Something is over the window's place at \(324, 114\): Crash Reporter/);
  // Another app's window behind ours there is fine: ours slides over it.
  guardWindowMove(APP, [394, 164], [324, 114], screen([moved, { pid: OTHER, owner: 'Finder', frame: [0, 0, 600, 600] }]));
  // A see-through overlay over the whole screen, let through at the grip
  // (the Dock's, which window-at proves), doesn't count at the drop either.
  const dock: Win = { pid: OTHER, owner: 'Dock', frame: [0, 0, 1920, 1080], id: 9 };
  const s = screen([dock, moved]);
  s.windowAt = (x, y) => (covers(moved, x, y) ? { pid: APP, owner: 'Threadbear' } : { pid: OTHER, owner: 'Dock' });
  guardWindowMove(APP, [394, 164], [324, 114], s);
  const both = screen([alert, dock, moved]);
  both.windowAt = s.windowAt;
  assert.throws(() => guardWindowMove(APP, [394, 164], [324, 114], both), /Crash Reporter/);
});

test('a drag path over someone else\'s window, or off screen, is refused', () => {
  const panel: Win = { pid: OTHER, owner: 'Slack', frame: [700, 300, 100, 100] };
  assert.throws(() => guardPath([APP], [400, 350], [1000, 350], screen([panel, app])), /passes over Slack/);
  guardPath([APP], [400, 350], [1000, 350], screen([app]));
  // Two displays side by side but staggered: endpoints on each, path through the gap.
  const two = { displays: [[0, 0, 1000, 600], [1000, 500, 1000, 600]] };
  const a: Win = { ...app, frame: [0, 0, 2000, 1100] };
  assert.throws(() => guardPath([APP], [900, 100], [1100, 1000], screen([a], two)), /off screen or within 24/);
});

/** The fake post, keeping what would have gone out. */
function sink(): { post: Post; sent: { tool: string; args: string[]; env: Record<string, string> }[] } {
  const sent: { tool: string; args: string[]; env: Record<string, string> }[] = [];
  return { post: (tool, args, env) => sent.push({ tool, args, env }), sent };
}

test('each drag looks at the screen as it is then, not as it was for the last one', () => {
  const windows: Win[] = [app];
  const s = screen(windows);
  const { post, sent } = sink();
  realDrag('realdrag', [400, 300], [400, 300], 0, { pid: APP, screen: s, post });
  assert.equal(sent.length, 1);
  // A window comes up over the drop of the next drag: nothing is posted.
  windows.unshift({ pid: ALERT, owner: 'Crash Reporter', frame: [700, 500, 200, 200] });
  assert.throws(() => realDrag('realdrag', [400, 300], [800, 600], 0.2, { pid: APP, screen: s, post }), /Crash Reporter/);
  // Or over the middle of its path.
  assert.throws(() => realDrag('realdrag', [400, 300], [1200, 900], 0.2, { pid: APP, screen: s, post }), /passes over Crash Reporter/);
  assert.equal(sent.length, 1);
  // Recording starts: nothing either.
  windows.shift();
  const rec = { ...s, recording: () => '9 ffmpeg' };
  assert.throws(() => realDrag('realdrag', [400, 300], [800, 600], 0, { pid: APP, screen: rec, post }), /recording/);
  assert.equal(sent.length, 1);
});

test('a drag hands realdrag who must be at the press and the drop, to look again itself', () => {
  const target: Win = { pid: 555, owner: 'droptarget', frame: [1540, 250, 240, 240] };
  const { post, sent } = sink();
  realDrag('realdrag', [1400, 360], [1660, 370], 0.4, { pid: APP, dropPid: 555, screen: screen([target, app]), post });
  assert.deepEqual(sent[0].args, ['1400', '360', '1660', '370', '0.4', '']);
  assert.equal(sent[0].env.RD_PRESS_PID, String(APP));
  assert.equal(sent[0].env.RD_DROP_PID, '555');
  assert.ok(sent[0].env.RD_GUARD);
  // A drop on our target, but the target dropped onto the app's: refused.
  assert.throws(() => realDrag('realdrag', [1400, 360], [1000, 370], 0.4, { pid: APP, dropPid: 555, screen: screen([target, app]), post }), /Threadbear is there/);
  // A grip drag has realdrag look for the app at the drop too: by the time
  // it lets go, the window has moved there with the pointer.
  realGripDrag('realdrag', [324, 114], [394, 164], 0.1, { pid: APP, env: { RD_PRESS: '0.02' }, screen: screen([app]), post });
  assert.equal(sent[1].env.RD_PRESS_PID, String(APP));
  assert.equal(sent[1].env.RD_DROP_PID, String(APP));
  assert.equal(sent[1].env.RD_PRESS, '0.02');
});

// The tools post every mouse event through realDrag or realGripDrag, and
// every swipe with a fresh guard and post-gesture's own look (gestureGuard).
test('the drag tools post nothing but through the guarded helpers', () => {
  for (const file of ['features/tools/real-gestures.ts', 'features/tools/window-drag.ts']) {
    const text = fs.readFileSync(path.join(root, file), 'utf8');
    assert.doesNotMatch(text, /(execFileSync|spawn)\(dragger\b/, `${file} runs realdrag directly`);
    assert.doesNotMatch(text, /assertInFront/, `${file} should check before each event, not once up front`);
    const lines = text.split('\n');
    for (const [i, line] of lines.entries()) {
      if (!/(execFileSync|spawn)\(gesture\b/.test(line)) continue;
      assert.match(line, /gestureGuard\(/, `${file}:${i + 1} runs post-gesture without its own look`);
      assert.match(lines.slice(Math.max(0, i - 3), i).join('\n'), /guardInput\(/, `${file}:${i + 1} runs post-gesture without a guard just before it`);
    }
  }
});

test('realdrag looks again at the press, every step and the drop, and backs out to the press point', { skip: process.platform !== 'darwin' }, () => {
  // Compiles only the drag's decisions with a made-up screen; posts nothing.
  const folder = fs.mkdtempSync(path.join(os.tmpdir(), 'tb-realdrag-'));
  try {
    const main = path.join(folder, 'main.swift');
    fs.writeFileSync(main, fs.readFileSync(path.join(root, 'app/Tools/realdrag.swift'), 'utf8') + '\n' + fs.readFileSync(path.join(root, 'runner/test/fixtures/realdrag.swift'), 'utf8'));
    const binary = path.join(folder, 'realdrag-test');
    execFileSync('swiftc', ['-D', 'TESTING', main, '-o', binary], { timeout: 120_000 });
    assert.match(execFileSync(binary, { encoding: 'utf8', timeout: 10_000 }), /realdrag checks passed/);
  } finally {
    fs.rmSync(folder, { recursive: true, force: true });
  }
});

test('post-gesture looks again before it touches and before every event', () => {
  const gesture = fs.readFileSync(path.join(root, 'app/Tools/post-gesture.swift'), 'utf8');
  const look = gesture.indexOf('env["PG_GUARD"]'), touch = gesture.indexOf('post(0, 0, phase: 128)'), each = gesture.indexOf('if let want, covering()');
  const fn = gesture.indexOf('func post(');
  assert.ok(look > 0 && touch > 0 && each > 0 && fn > 0, 'the checks are all there');
  assert.ok(look < touch, 'the look comes before the fingers touch');
  assert.ok(fn < each && each < gesture.indexOf('cg.post(tap:'), 'every event is checked before it goes');
  assert.match(gesture, /timedOut/, 'the guard program has a deadline');
  assert.match(gesture, /terminationStatus == 0/, 'and must exit cleanly');
  assert.match(gesture, /over\[\.\.<mine\]\.allSatisfy\(\{ \$0\.overlay \}\)/, 'only the Dock overlay may be over the app at the start');
});
