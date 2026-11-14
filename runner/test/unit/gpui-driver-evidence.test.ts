// The spec driver (features/support/gpui.ts) keeps the evidence when the
// GPUI app can't answer a command. A terminal spec on the cloud box once got
// only "terminal: the app stopped", and teardown then removed the temp
// folder holding the panic and the app's log (13 Nov). A small stand-in app
// (a node script, never a build or a window) plays each way it can go.
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import assert from 'node:assert/strict';
import { test, type TestContext } from 'node:test';
import { spawnSync } from 'node:child_process';

const root = path.resolve(import.meta.dirname, '../../..');

// Headless Linux runs the app under Xvfb and dbus-run-session (gpui.ts); a
// box without them (the Pixel following main, 14 Nov) can't start even the
// stand-in, so these skip there as on Windows.
const onPath = (bin: string) => spawnSync('sh', ['-c', `command -v ${bin}`]).status === 0;
const skip = process.platform === 'win32'
  || (process.platform === 'linux' && !process.env.DISPLAY && !process.env.WAYLAND_DISPLAY && !(onPath('Xvfb') && onPath('dbus-run-session')));

// What the real app does, as far as the driver can see: writes its port and
// token to THREADBEAR_AUTOMATION, logs beside it, and keeps panics in
// crashes/ there (crash_reporting.rs). FAKE_MODE picks how it fails.
const FAKE_APP = `#!/usr/bin/env node
const fs = require('node:fs'), net = require('node:net'), path = require('node:path');
const file = process.env.THREADBEAR_AUTOMATION, home = path.dirname(file), mode = process.env.FAKE_MODE;
const log = (line) => fs.appendFileSync(path.join(home, 'app.log'), line + '\\n');
const panic = () => {
  fs.mkdirSync(path.join(home, 'crashes'), { recursive: true });
  fs.writeFileSync(path.join(home, 'crashes', 'panic-2026-11-13-120000-000.json'), JSON.stringify({ at: 'now', main_thread: true, message: 'selection out of range', location: 'desktop/gpui/src/terminal.rs:42:9', frames: ['threadbear_gpui::terminal::select'], reported: false }));
  process.stderr.write("thread 'main' panicked at desktop/gpui/src/terminal.rs:42:9:\\nbyte index 3 is not a char boundary; it is inside 'é' of \`private words\`\\nnote: run with RUST_BACKTRACE=1\\n");
};
log('started');
const server = net.createServer((s) => {
  let buf = '';
  s.on('data', (d) => {
    buf += d;
    let at;
    while ((at = buf.indexOf('\\n')) >= 0) {
      const cmd = JSON.parse(buf.slice(0, at)).cmd;
      buf = buf.slice(at + 1);
      log(cmd);
      if (mode === 'refuse') s.write(JSON.stringify({ error: 'no button called Nope' }) + '\\n');
      else if (mode === 'old') { panic(); s.write(JSON.stringify({ error: 'the app stopped' }) + '\\n'); }
      else if (mode === 'drop') { panic(); s.write(JSON.stringify({ error: 'the app dropped \`' + cmd + '\` without answering', unanswered: 'dropped' }) + '\\n'); }
      else if (mode === 'exit') { panic(); process.exit(101); }
    }
  });
});
server.listen(0, '127.0.0.1', () => fs.writeFileSync(file, JSON.stringify({ port: server.address().port, token: 't' })));
`;

type Driver = {
  start(): Promise<void>;
  stop(): Promise<void>;
  do(cmd: Record<string, unknown>, timeoutSeconds?: number): Promise<string>;
  keptEvidence: string[];
};

// One stand-in for the whole file: the driver reads TB_GPUI_BIN once, at import.
const binDir = fs.mkdtempSync(path.join(os.tmpdir(), 'tb-driver-evidence-bin-'));
const bin = path.join(binDir, 'fake-gpui.cjs');
fs.writeFileSync(bin, FAKE_APP, { mode: 0o755 });
process.env.TB_GPUI_BIN = bin;
process.on('exit', () => fs.rmSync(binDir, { recursive: true, force: true }));
const { GpuiDriver } = (await import(path.join(root, 'features/support/gpui.ts'))) as { GpuiDriver: new () => Driver };

async function driver(t: TestContext, mode: string): Promise<{ app: Driver; home: string; kept: string }> {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'tb-driver-evidence-'));
  const home = path.join(dir, 'home');
  fs.mkdirSync(home);
  const kept = path.join(dir, 'kept');
  // A runner home of our own means no mock runner; FAKE_MODE reaches the
  // stand-in through the environment the driver hands it.
  Object.assign(process.env, { TB_RUNNER_HOME: home, TB_SPEC_EVIDENCE: kept, FAKE_MODE: mode });
  const app = new GpuiDriver();
  t.after(async () => {
    await app.stop();
    fs.rmSync(dir, { recursive: true, force: true });
  });
  await app.start();
  return { app, home, kept };
}

test('a dropped reply says the app is still running, with its panic, log and output, kept past teardown', { skip }, async (t) => {
  const { app, home } = await driver(t, 'drop');
  const error = await app.do({ cmd: 'terminal', select: 'x' }).then(() => assert.fail('it answered'), (e: Error) => e.message);
  assert.match(error, /^terminal: the app dropped `terminal` without answering/, 'the original error comes first');
  assert.match(error, /The app \(pid \d+\) is still running\./);
  assert.match(error, /panicked on its main thread at desktop\/gpui\/src\/terminal\.rs:42:9: selection out of range/);
  assert.match(error, /The end of its log:\nstarted\nterminal/);
  // On Linux the app runs under a virtual display, whose own warnings
  // (libEGL, xkbcomp) come before the panic in what it printed.
  assert.match(error, /The end of what it printed:\n[\s\S]*thread 'main' panicked/);
  // Teardown removes the scenario's folder; the copy stays.
  fs.rmSync(home, { recursive: true, force: true });
  const [dir] = app.keptEvidence;
  assert.ok(dir && error.includes(`Kept in ${dir}`));
  assert.deepEqual(fs.readdirSync(path.join(dir, 'crashes')), ['panic-2026-11-13-120000-000.json']);
  assert.match(fs.readFileSync(path.join(dir, 'app.log'), 'utf8'), /terminal/);
  assert.match(fs.readFileSync(path.join(dir, 'output.txt'), 'utf8'), /panicked/);
  assert.equal(fs.statSync(dir).mode & 0o077, 0, 'only this user can read it');
  // Rust's own hook prints the panic's raw message; only the scrubbed record's is kept.
  assert.doesNotMatch(error, /private words/);
  assert.doesNotMatch(fs.readFileSync(path.join(dir, 'output.txt'), 'utf8'), /private words/);
});

test("an older app's \"the app stopped\" gets the same evidence", { skip }, async (t) => {
  const { app } = await driver(t, 'old');
  const error = await app.do({ cmd: 'dump' }).then(() => assert.fail('it answered'), (e: Error) => e.message);
  assert.match(error, /^dump: the app stopped\n/);
  assert.match(error, /selection out of range/);
  assert.equal(app.keptEvidence.length, 1);
});

test('an app that exits mid-command fails it at once, with the exit and the panic, not at the timeout', { skip }, async (t) => {
  const { app } = await driver(t, 'exit');
  const started = Date.now();
  const error = await app.do({ cmd: 'open', id: 'demo' }, 20).then(() => assert.fail('it answered'), (e: Error) => e.message);
  assert.ok(Date.now() - started < 10_000, `took ${Date.now() - started}ms`);
  assert.match(error, /^open: the app closed its automation connection without answering/);
  // The close can come before node has seen the exit; either way it's not "running".
  assert.match(error, /exited with code 101|is still running/);
  assert.match(error, /selection out of range/);
  assert.equal(app.keptEvidence.length, 1);
});

test('pruning old evidence leaves alone anything else in its folder', { skip }, async (t) => {
  const { app, kept } = await driver(t, 'drop');
  fs.mkdirSync(kept, { recursive: true });
  for (let i = 0; i < 25; i++) fs.writeFileSync(path.join(kept, `000-report-${i}`), '');
  for (let i = 0; i < 22; i++) fs.mkdirSync(path.join(kept, `2020-01-01T00-00-${String(i).padStart(2, '0')}-000Z-abcdef`));
  await app.do({ cmd: 'dump' }).catch(() => {});
  const left = fs.readdirSync(kept);
  assert.equal(left.filter((n) => n.startsWith('000-report-')).length, 25);
  assert.equal(left.filter((n) => !n.startsWith('000-report-')).length, 20);
});

test("an ordinary refusal stays short and keeps nothing: specs poll for those", { skip }, async (t) => {
  const { app, kept } = await driver(t, 'refuse');
  const error = await app.do({ cmd: 'label-point', label: 'Nope' }, 5).then(() => assert.fail('it answered'), (e: Error) => e.message);
  assert.equal(error, 'label-point: no button called Nope');
  assert.equal(fs.existsSync(kept), false);
});
