import { test } from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync, spawn } from 'node:child_process';
import { once } from 'node:events';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { setTimeout as sleep } from 'node:timers/promises';
import { executableProcesses, processSnapshot, recording } from '../../../features/tools/real-screen.ts';

test('the real process reader sees this executable without its arguments', { skip: process.platform === 'win32' }, () => {
  assert.ok(executableProcesses().some(p => p.pid === process.pid && p.name === path.basename(process.execPath)));
});

test('macOS retains the kernel executable name when argv0 changes', { skip: process.platform !== 'darwin' }, async () => {
  // Neutral names only: fake encoder names could block another Mac GUI job.
  const child = spawn('/bin/sleep', ['10'], { argv0: 'tb-guard-alias', stdio: 'ignore' });
  const exited = once(child, 'exit');
  try {
    await once(child, 'spawn');
    const snapshot = processSnapshot();
    const names = executableProcesses(() => snapshot).filter(p => p.pid === child.pid).map(p => p.name);
    assert.ok(names.includes('tb-guard-alias'), 'the launch name is missing');
    assert.ok(names.includes('sleep'), 'argv0 hid the kernel executable name');
    assert.match(snapshot, new RegExp(`^\\s*${child.pid} /bin/sleep$`, 'm'), 'the full kernel path is missing');
  } finally {
    if (child.exitCode === null && child.signalCode === null) child.kill();
    await exited;
  }
});

test('script text mentioning ffmpeg does not make its wrapper a recording', { skip: process.platform === 'win32' }, async () => {
  // No encoder, GUI events or display assertion. Hold it until the check
  // finishes, with a backstop in case this test process goes away.
  const command = [process.execPath, '-e', `/* ffmpeg preflight */
    process.stdin.resume();
    process.stdin.once('end', () => process.exit(0));
    setTimeout(() => process.exit(2), 15000).unref();
    console.log(process.pid);`];
  if (process.platform === 'darwin') command.unshift('/usr/bin/caffeinate', '-i');
  const child = spawn(command[0], command.slice(1),
  { stdio: ['pipe', 'pipe', 'inherit'] });
  const exited = once(child, 'exit');
  try {
    const [ready] = await Promise.race([
      once(child.stdout!, 'data'),
      exited.then(() => { throw new Error('wrapper exited before reporting its PID'); }),
    ]);
    const pid = Number(ready.toString().trim());
    const owned = new Set([child.pid!, pid]);
    const parents = execFileSync('/bin/ps', ['-Aww', '-o', 'pid=', '-o', 'ppid='], { encoding: 'utf8' })
      .trim().split('\n').map(line => line.trim().split(/\s+/).map(Number));
    // caffeinate may exec the command and stay as its companion child.
    for (let size = -1; size !== owned.size;) {
      size = owned.size;
      for (const [kid, parent] of parents) if (owned.has(parent)) owned.add(kid);
    }
    process.kill(pid, 0); // Only our child: the regression needs it alive.
    const matches = recording().split('\n').filter(Boolean).map(line => Number(line.split(/\s+/)[0]));
    process.kill(pid, 0);
    assert.ok(!matches.some(match => owned.has(match)), 'the harmless wrapper or command matched its ffmpeg comment');
  } finally { child.stdin!.end(); await exited; }
});

test('no matches is empty, but a failed lookup never permits input', () => {
  assert.equal(recording(() => []), '');
  for (const output of ['', ' 123\n 456\n']) {
    assert.throws(() => executableProcesses(() => output), /Could not read process executable names/);
  }
  for (const status of [1, 2, 3, null, undefined]) {
    const error = Object.assign(new Error('lookup failed'), { status });
    assert.throws(() => recording(() => { throw error; }), e => e === error);
  }
  const missing = Object.assign(new Error('pgrep missing'), { code: 'ENOENT' });
  assert.throws(() => recording(() => { throw missing; }), e => e === missing);
});

test('executable basenames preserve long variants and ignore directory names', () => {
  assert.equal(recording(() => executableProcesses(() => `
  10 /usr/local/bin/ffmpeg
  11 /tmp/a directory/tb-screen-record-ffmpeg
  12 /opt/FFmpeg-static
  13 /tmp/ffmpeg-work/node
  14 /usr/bin/caffeinate
  15 /opt/ffmpeg-static-rebuild
  15 ffmpeg-static-r
  16 -
  16 ffmpeg
  10 ffmpeg
`)), '10 ffmpeg\n11 tb-screen-record-ffmpeg\n12 FFmpeg-static\n15 ffmpeg-static-rebuild\n16 ffmpeg');
});

test('encoder executables and variants still block input with default and renamed argv0', {
  skip: process.platform !== 'linux',
}, async () => {
  // Linux checks run on cloud. Macs use the neutral identity test above
  // so fake encoder names never compete with their real GUI jobs.
  const folder = fs.mkdtempSync(path.join(os.tmpdir(), 'tb-recording-'));
  try {
    for (const name of ['ffmpeg', 'ffmpeg-static', 'ffmpeg7', 'FFmpeg', 'custom-ffmpeg', 'tb-screen-record-ffmpeg']) {
      const binary = path.join(folder, name);
      fs.copyFileSync('/bin/sleep', binary);
      for (const argv0 of [undefined, 'tb-encoder-alias']) {
        const child = spawn(binary, ['5'], { argv0, stdio: 'ignore' });
        const exited = once(child, 'exit');
        try {
          await once(child, 'spawn');
          let matches: number[] = [];
          for (let i = 0; i < 50; i++) {
            matches = recording().split('\n').map(line => Number(line.split(/\s+/)[0]));
            if (matches.includes(child.pid!)) break;
            await sleep(10);
          }
          assert.ok(matches.includes(child.pid!), `${name} (${argv0 ?? 'default argv0'}) was not detected`);
        } finally {
          if (child.exitCode === null && child.signalCode === null) child.kill();
          await exited;
        }
      }
    }
  } finally { fs.rmSync(folder, { recursive: true, force: true }); }
});
