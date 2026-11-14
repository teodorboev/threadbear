// scripts/hangup-guard.sh: what runs a command on a check host stops it when
// the ssh that started it goes. Here the guard's stdin stands in for the ssh
// channel: closing it is the ssh client dying. The real thing was tried on
// the cloud box and Tinker Desk (Desk Runner) when it was written (13 Nov).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawn, spawnSync } from 'node:child_process';

const LIB = path.join(import.meta.dirname, '..', '..', '..', 'scripts', 'remote-guard-lib.sh');

function guarded(...args: string[]): string {
  const r = spawnSync('/bin/bash', ['-c', '. "$0"; guarded_cmd "$@"', LIB, ...args], { encoding: 'utf8' });
  assert.equal(r.status, 0, r.stderr);
  return r.stdout;
}

const alive = (pid: number) => {
  try {
    process.kill(pid, 0);
    return true;
  } catch {
    return false;
  }
};

async function until(what: string, ok: () => boolean, ms = 10_000) {
  const end = Date.now() + ms;
  while (!ok()) {
    if (Date.now() > end) assert.fail(`timed out waiting for ${what}`);
    await new Promise((r) => setTimeout(r, 100));
  }
}

function tmp(t: { after: (fn: () => void) => void }) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'tb-guard-'));
  t.after(() => fs.rmSync(dir, { recursive: true, force: true }));
  return dir;
}

// The channel stays open until the command has finished, as a live ssh's
// does: closed straight after the typing (spawnSync's input), it was the ssh
// going, and on a busy Mac the guard stopped the command (143) before its
// exit 7 (the runner's release build, 13 Nov).
test('a finished command keeps its output and exit code, and gets no stdin', async () => {
  const ssh = spawn('/bin/sh', ['-c', guarded('-', 'bash', '-c', `echo "it's out"; echo err >&2; read x; echo "in=[$x]"; exit 7`)], { stdio: ['pipe', 'pipe', 'pipe'] });
  let out = '';
  let err = '';
  ssh.stdout.setEncoding('utf8').on('data', (d: string) => (out += d));
  ssh.stderr.setEncoding('utf8').on('data', (d: string) => (err += d));
  const exited = new Promise<number | null>((resolve) => ssh.on('exit', resolve));
  const closed = new Promise((resolve) => ssh.on('close', resolve));
  ssh.stdin.write('typed\n');
  const status = await exited;
  ssh.stdin.end();
  await closed;
  assert.equal(status, 7);
  assert.equal(out, "it's out\nin=[]\n");
  assert.equal(err, 'err\n');
});

test('the ssh going stops the command and everything under it', async (t) => {
  const dir = tmp(t);
  const pids = path.join(dir, 'pids');
  const ssh = spawn('/bin/sh', ['-c', guarded('-', 'bash', '-c', `sleep 300 & echo $! >>"${pids}"; bash -c 'echo $$ >>"${pids}"; sleep 301'`)], { stdio: ['pipe', 'ignore', 'ignore'] });
  t.after(() => ssh.kill('SIGKILL'));
  await until('the command to start', () => fs.existsSync(pids) && fs.readFileSync(pids, 'utf8').trim().split('\n').length === 2);
  const started = fs.readFileSync(pids, 'utf8').trim().split('\n').map(Number);
  assert.ok(started.every(alive));
  ssh.stdin.end();
  await until('the command to stop', () => !started.some(alive));
});

test('a command Desk Runner started away from the guard goes too, by its pidfile', async (t) => {
  const dir = tmp(t);
  const pidfile = path.join(dir, 'run.pid');
  // What remote.sh's run script does there: says where it is, unless the ssh
  // has already gone.
  const away = spawn('/bin/bash', ['-c', `echo $$ >"${pidfile}"; [ ! -e "${pidfile}.gone" ] || exit 129; sleep 300`], { detached: true, stdio: 'ignore' });
  t.after(() => {
    try {
      process.kill(-away.pid!, 'SIGKILL');
    } catch {}
  });
  await until('the run to start', () => fs.existsSync(pidfile));
  const ssh = spawn('/bin/sh', ['-c', guarded(pidfile, 'sleep', '300')], { stdio: ['pipe', 'ignore', 'ignore'] });
  const done = new Promise<void>((r) => ssh.on('exit', () => r()));
  ssh.stdin.end();
  await until('the run to stop', () => away.exitCode !== null || away.signalCode !== null);
  await done;
  // Left for a run that starts late to find (the last test here).
  assert.ok(fs.existsSync(`${pidfile}.gone`), 'the .gone marker stays');
});

test('a command that ends just as the ssh goes still has its Desk Runner run stopped', async (t) => {
  const dir = tmp(t);
  const pidfile = path.join(dir, 'run.pid');
  const flag = path.join(dir, 'flag');
  const away = spawn('/bin/bash', ['-c', `echo $$ >"${pidfile}"; sleep 300`], { detached: true, stdio: 'ignore' });
  t.after(() => {
    try {
      process.kill(-away.pid!, 'SIGKILL');
    } catch {}
  });
  await until('the run to start', () => fs.existsSync(pidfile));
  // The desk client, which dies writing to the channel that's gone: here it
  // ends as the ssh does, so the guard sees the command end and the channel
  // close at about the same moment.
  const ssh = spawn('/bin/sh', ['-c', guarded(pidfile, 'bash', '-c', `until [ -e "${flag}" ]; do sleep 0.02; done`)], { stdio: ['pipe', 'ignore', 'ignore'] });
  await new Promise((r) => setTimeout(r, 300));
  ssh.stdin.end();
  fs.writeFileSync(flag, '');
  await until('the run to stop', () => away.exitCode !== null || away.signalCode !== null);
});

test('something forked into the group as it stops, ignoring TERM, is still killed', async (t) => {
  const dir = tmp(t);
  const late = path.join(dir, 'late');
  const ready = path.join(dir, 'ready');
  const script = `trap 'bash -c "trap \\"\\" TERM; echo \\$\\$ >${late}; sleep 300" & sleep 0.3; exit 0' TERM; : >"${ready}"; while :; do sleep 0.1; done`;
  const ssh = spawn('/bin/sh', ['-c', guarded('-', 'bash', '-c', script)], { stdio: ['pipe', 'ignore', 'ignore'] });
  t.after(() => ssh.kill('SIGKILL'));
  await until('the command to start', () => fs.existsSync(ready));
  ssh.stdin.end();
  await until('the late worker to start', () => fs.existsSync(late) && fs.readFileSync(late, 'utf8').trim() !== '');
  const pid = Number(fs.readFileSync(late, 'utf8').trim());
  t.after(() => {
    try {
      process.kill(pid, 'SIGKILL');
    } catch {}
  });
  await until('the late worker to be killed', () => !alive(pid), 15_000);
});

test('a Desk Runner run that starts after the ssh has gone never runs', async (t) => {
  const dir = tmp(t);
  const pidfile = path.join(dir, 'run.pid');
  const ran = path.join(dir, 'ran');
  const ssh = spawn('/bin/sh', ['-c', guarded(pidfile, 'sleep', '300')], { stdio: ['pipe', 'ignore', 'ignore'] });
  const done = new Promise<void>((r) => ssh.on('exit', () => r()));
  await new Promise((r) => setTimeout(r, 300));
  ssh.stdin.end();
  await done;
  const r = spawnSync('/bin/bash', ['-c', `echo $$ >"${pidfile}"; [ ! -e "${pidfile}.gone" ] || exit 129; : >"${ran}"`]);
  assert.equal(r.status, 129);
  assert.ok(!fs.existsSync(ran));
});
