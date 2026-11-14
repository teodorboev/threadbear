// desktop/scripts/remote.sh over a connection that drops. On 14 Nov a network
// drop to Tinker Desk left a run's ssh sitting on a dead connection after
// "Waiting for another build or measurement...", for over half an hour, as
// nothing told it the host had gone. Every ssh it makes now sends keep-alives,
// so a drop ends it with ssh's 255, and it says the connection went rather
// than leaving the lock wait as the last word.
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import assert from 'node:assert/strict';
import { test } from 'node:test';

const root = path.resolve(import.meta.dirname, '../../..');

test('remote.sh sends keep-alives on every ssh and says when the connection drops mid-run', (t) => {
  const work = fs.mkdtempSync(path.join(os.tmpdir(), 'tb-remote-drop-'));
  t.after(() => fs.rmSync(work, { recursive: true, force: true }));
  const home = path.join(work, 'home');
  const bin = path.join(work, 'bin');
  const calls = path.join(work, 'calls');
  fs.mkdirSync(home);
  fs.mkdirSync(bin);
  // The run itself (perf-lock.pl starting run-<tag>.sh) waits for a lock and
  // then loses its connection, as ssh does once 4 keep-alives go unanswered.
  // Everything before it runs here.
  fs.writeFileSync(path.join(bin, 'ssh'), `#!/bin/bash
printf 'ssh %s\\n' "\${*:1:$#-2}" >>${JSON.stringify(calls)}
case "\${@: -1}" in
  *perf-lock.pl\\ -\\ bash\\ .cache/*/run-*.sh*)
    echo "Waiting for another build or measurement on desk (lock)..." >&2
    echo "Timeout, server host not responding." >&2
    exit 255 ;;
esac
cd "$HOME"
exec /bin/bash -c "\${@: -1}"
`, { mode: 0o755 });
  fs.writeFileSync(path.join(bin, 'rsync'), `#!/bin/bash
printf 'rsync %s\\n' "\${*:1:$#-2}" >>${JSON.stringify(calls)}
dest="$HOME/\${!#}"
mkdir -p "\${dest/host:/}"
`, { mode: 0o755 });
  fs.writeFileSync(path.join(bin, 'df'), "#!/bin/bash\necho 'Filesystem 1024-blocks Used Available Capacity Mounted'\necho 'fake 0 0 41943040 0% /'\n", { mode: 0o755 });
  const r = spawnSync('/bin/bash', [path.join(root, 'desktop/scripts/remote.sh'), 'host', '--', 'true'], {
    env: { ...process.env, HOME: home, PATH: `${bin}:${process.env.PATH}`, THREADBEAR_PERF_LOCK_OWNER: '' },
    encoding: 'utf8',
    timeout: 20_000,
  });
  assert.equal(r.status, 255, r.stderr);
  assert.match(r.stderr, /Waiting for another build or measurement[^]*remote\.sh: lost the ssh connection to host/);
  const lines = fs.readFileSync(calls, 'utf8').trim().split('\n');
  assert.ok(lines.filter((l) => l.startsWith('ssh ')).length >= 4, lines.join('\n'));
  for (const line of lines) assert.match(line, /ServerAliveInterval=30 -o ServerAliveCountMax=4/, line);
  assert.ok(lines.some((l) => /^rsync .*-e ssh -o ServerAliveInterval=30/.test(l)), 'rsync goes over the same keep-alive ssh');
});
