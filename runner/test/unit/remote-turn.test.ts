// desktop/scripts/remote.sh stopped while it waits for its turn (another run
// from the same checkout to the same host). On 14 Nov a TERM to it sat unanswered
// for over 30 s: bash holds a trap until a foreground child ends, and the lock
// wait was one (d7aca215). No remote work starts here: the ssh is a fake that
// only records being called.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawn } from 'node:child_process';

const ROOT = path.join(import.meta.dirname, '..', '..', '..');
const SCRIPT = path.join(ROOT, 'desktop', 'scripts', 'remote.sh');

for (const tryOnly of [false, true]) test(tryOnly ? 'an updater skips a busy checkout without contacting the host' : 'a run waiting for its turn stops at once on TERM, and never reaches the host', async (t) => {
  const home = fs.mkdtempSync(path.join(os.tmpdir(), 'tb-turn-'));
  t.after(() => fs.rmSync(home, { recursive: true, force: true }));
  const bin = path.join(home, 'bin');
  fs.mkdirSync(bin);
  const called = path.join(home, 'ssh-called');
  for (const tool of ['ssh', 'rsync', 'scp']) {
    fs.writeFileSync(path.join(bin, tool), `#!/bin/bash\necho "${tool} $*" >>"${called}"\nexit 1\n`, { mode: 0o755 });
  }
  // The turn's lock file, as remote.sh names it, held by another process.
  const key = crypto.createHash('sha1').update(ROOT).digest('hex').slice(0, 10);
  const cache = process.platform === 'darwin' ? path.join(home, 'Library', 'Caches', 'Threadbear') : path.join(home, '.cache', 'threadbear');
  fs.mkdirSync(path.join(cache, 'remote'), { recursive: true });
  const lock = path.join(cache, 'remote', `${key}-somehost.lock`);
  const holder = spawn('perl', ['-e', 'use Fcntl ":flock"; open(my $f, ">>", $ARGV[0]) or die; flock($f, LOCK_EX) or die; print "held\\n"; $| = 1; sleep 300', lock], { stdio: ['ignore', 'pipe', 'ignore'] });
  t.after(() => holder.kill('SIGKILL'));
  await new Promise<void>((r) => holder.stdout.once('data', () => r()));

  const env: Record<string, string | undefined> = { ...process.env, HOME: home, PATH: `${bin}:${process.env.PATH}` };
  delete env.XDG_CACHE_HOME;
  delete env.THREADBEAR_REMOTE_KEY;
  const run = spawn('/bin/bash', [SCRIPT, 'somehost', ...(tryOnly ? ['--try'] : []), '--', 'true'], { env, stdio: ['ignore', 'ignore', 'pipe'] });
  t.after(() => run.kill('SIGKILL'));
  let stderr = '';
  run.stderr.on('data', (d) => (stderr += d));
  // close, not exit: it waits for everything holding the run's stderr, so a
  // lock waiter left behind (still holding it) fails this too (Codex).
  const closed = new Promise<number | null>((r) => run.on('close', (code) => r(code)));
  if (tryOnly) {
    const code = await Promise.race([closed, new Promise<'late'>((r) => setTimeout(() => r('late'), 5000))]);
    assert.equal(code, 75, stderr);
    assert.ok(!fs.existsSync(called), 'a deferred updater never contacts the host');
    return;
  }
  const end = Date.now() + 15_000;
  while (!stderr.includes('Waiting for another remote.sh run')) {
    assert.ok(Date.now() < end, `never said it was waiting: ${stderr}`);
    await new Promise((r) => setTimeout(r, 50));
  }
  // The message comes just before the blocking wait: give that a moment to
  // start, so this is a TERM during the wait.
  await new Promise((r) => setTimeout(r, 500));
  run.kill('SIGTERM');
  const code = await Promise.race([closed, new Promise<'late'>((r) => setTimeout(() => r('late'), 5000))]);
  assert.notEqual(code, 'late', 'still going 5 s after TERM');
  assert.equal(code, 143, stderr);
  assert.ok(!fs.existsSync(called), `reached the host: ${fs.existsSync(called) ? fs.readFileSync(called, 'utf8') : ''}`);
});
