// Call before doing any work in a measurement entry point, including an A/B
// parent. Its children inherit the same host lock; standalone tools take it.
import { spawn } from 'node:child_process';
import os from 'node:os';
import path from 'node:path';

export async function ensurePerfLock(): Promise<void> {
  // Measurement hosts use Unix locks/process groups. Keep standalone Windows
  // screenshot tools on their existing path, without the Unix supervisor.
  if (process.platform === 'win32') return;
  const owner = process.env.THREADBEAR_PERF_LOCK_OWNER ?? '';
  const prefix = `${os.hostname()}:`;
  const suffix = `:${process.env.HOME}`;
  if (owner.startsWith(prefix) && owner.endsWith(suffix)) {
    const pid = Number(owner.slice(prefix.length, -suffix.length));
    if (Number.isSafeInteger(pid) && pid > 0) {
      try { process.kill(pid, 0); return; } catch { /* The previous holder ended. */ }
    }
  }
  const helper = path.resolve(import.meta.dirname, '../../scripts/perf-lock.pl');
  const child = spawn('perl', [helper, '-', process.execPath, ...process.execArgv, ...process.argv.slice(1)], {
    stdio: ['pipe', 'inherit', 'inherit'], detached: true,
  });
  // Closing the channel, rather than killing the holder, lets its guard stop
  // the whole tree before the lock is released. KILL of this parent does too.
  for (const signal of ['SIGTERM', 'SIGHUP', 'SIGINT'] as const) process.on(signal, () => child.stdin!.end());
  child.on('error', (error) => { console.error(error.message); process.exit(1); });
  const code = await new Promise<number>((resolve) => child.on('exit', (code, signal) => resolve(code ?? (signal ? 1 : 0))));
  process.exit(code);
}
