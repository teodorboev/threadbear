// The Swift Mac app, through app/Tools/dev-app.sh: a debug build in a temp
// bundle, on the mock runner, with its window kept behind everything.
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import path from 'node:path';
import fs from 'node:fs';
import { fileURLToPath } from 'node:url';
import type { Driver, Dump } from './driver.ts';

const run = promisify(execFile);
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const script = path.join(root, 'app/Tools/dev-app.sh');
let instances = 0;

export class SwiftDriver implements Driver {
  readonly name = 'swift' as const;
  private id = `spec-${process.pid}-${++instances}`;

  private async script(args: string[], timeoutSeconds = 60): Promise<string> {
    // Only start/rebuild can detach work. Other commands stay in the worker's
    // group, avoiding a registry entry for every dump in a polling loop.
    const registry = (args[0] === 'start' || args[2] === 'rebuild') ? process.env.THREADBEAR_PERF_CHILDREN : undefined;
    // A shared host lease gates the frozen launcher's detached mock and app.
    // Ordinary debug launches still use LS, including its temp-folder setup.
    let instance = '';
    if (registry) {
      // Match the launcher's fallback exactly; os.tmpdir() also uses TMP/TEMP.
      const stateRoot = path.join(process.env.TMPDIR || '/tmp', 'threadbear-dev-app');
      fs.mkdirSync(stateRoot, { recursive: true });
      // dev-app.sh canonicalizes the instance root too, which can itself be
      // a symlink. A mismatched path would miss the private-app adapter.
      instance = path.join(fs.realpathSync(stateRoot), this.id);
    }
    const { stdout } = await run(registry ? 'perl' : script, registry
      ? [path.join(root, 'scripts/perf-swift.pl'), '--run', registry,
        process.env.THREADBEAR_NODE || process.env.THREADBARE_NODE || process.execPath, instance, script, ...args]
      : args, {
      timeout: timeoutSeconds * 1000,
      maxBuffer: 32 * 1024 * 1024,
      // The other Macs' demo threads are their own feature's business, and
      // leaving them out makes the inbox the same for both apps. A scenario
      // about them sets TB_MOCK_REMOTE (@remote-threads).
      env: { ...process.env, TB_MOCK_NO_REMOTE: process.env.TB_MOCK_REMOTE ? '' : '1' },
    });
    return stdout.trim();
  }

  async start(): Promise<void> {
    if (process.env.THREADBEAR_PERF_CHILDREN) console.error('Swift reference: direct private-bundle launch under a shared host lease.');
    // The build is done once, up front (features/README.md): `start` would
    // otherwise spend several seconds finding nothing to build each scenario.
    const fixtures = process.env.TB_MOCK_FIXTURES ? ['--fixtures', process.env.TB_MOCK_FIXTURES] : [];
    // TB_BEAR_DATE: the bears' clock (bedtime, December's hat), for clips of them.
    const bearDate = process.env.TB_BEAR_DATE ? ['--bear-date', process.env.TB_BEAR_DATE] : [];
    await this.script(['start', '--mock', ...fixtures, ...bearDate, '--no-build', '--id', this.id, '--size', '1180x800'], 120);
  }

  /** The current private app process, for process-level performance counters. */
  async processId(): Promise<number> {
    const status = await this.script(['status']);
    const line = status.split('\n').find(line => line.trim().replace(/^\*\s*/, '').startsWith(`${this.id} `));
    const pid = line?.match(/running, pid (\d+)/)?.[1];
    if (!pid) throw new Error(`The Swift test app ${this.id} is not running.`);
    return Number(pid);
  }

  /** The owned bundle's executable, for PID-targeted benchmark activation. */
  privateExecutablePath(): string {
    const stateRoot = fs.realpathSync(path.join(process.env.TMPDIR || '/tmp', 'threadbear-dev-app'));
    return path.join(stateRoot, this.id, 'Threadbear.app', 'Contents', 'MacOS', 'Threadbear');
  }

  async do(cmd: Record<string, unknown>, timeoutSeconds = 30): Promise<string> {
    const out = await this.script(['--id', this.id, 'do', JSON.stringify(cmd), '--timeout', String(timeoutSeconds)], timeoutSeconds + 10);
    return out;
  }

  async dump(): Promise<Dump> {
    const dump = JSON.parse(await this.script(['--id', this.id, 'dump', '--json'])) as Dump;
    // No thread open means no transcript in its dump.
    dump.transcript ??= { rows: [] };
    return dump;
  }

  async shot(file: string): Promise<void> {
    await this.script(['--id', this.id, 'shot', file]);
  }

  async relaunch(): Promise<void> {
    await this.script(['--id', this.id, 'rebuild', '--no-build'], 120);
  }

  async appLog(): Promise<string> {
    return this.script(['--id', this.id, 'log', '-n', '5000']).catch(() => '');
  }

  async stop(): Promise<void> {
    await this.script(['--id', this.id, 'stop'], 60).catch(() => {});
  }
}
