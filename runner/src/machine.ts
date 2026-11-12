// The kind of Mac a runner is on, as an SF Symbol, so the app and the phone
// can tell runners apart at a glance (step 6: the M5 is a laptop, Bruce a
// Mac mini). Worked out once at startup from the model name macOS gives
// ("MacBook Pro", "Mac mini"); hw.model alone is no use on Apple silicon,
// where it's "Mac17,9" for a laptop and a desktop alike. `sync.icon` in
// runner.json overrides it.
import os from 'node:os';
import path from 'node:path';
import { execFile } from 'node:child_process';
import { atomicWriteFileSync, readJsonSync } from './fsutil.ts';

// Not a Mac (a cloud sandbox, later): a cloud.
export const NOT_A_MAC_ICON = 'cloud';
export const DEFAULT_MAC_ICON = 'desktopcomputer';

export function iconForMachine(machineName: string | undefined): string {
  const n = (machineName ?? '').toLowerCase();
  if (n.includes('macbook')) return 'laptopcomputer';
  if (n.includes('mac mini')) return 'macmini';
  if (n.includes('mac studio')) return 'macstudio';
  if (n.includes('mac pro')) return 'macpro.gen3';
  if (n.includes('imac')) return 'desktopcomputer';
  return DEFAULT_MAC_ICON;
}

// system_profiler takes about a third of a second; a slow or missing one
// gives the plain desktop rather than holding the runner's start up.
export function detectMachineIcon(timeoutMs = 3000): Promise<string> {
  if (os.platform() !== 'darwin') return Promise.resolve(NOT_A_MAC_ICON);
  return new Promise((resolve) => {
    execFile('/usr/sbin/system_profiler', ['SPHardwareDataType', '-json', '-detailLevel', 'mini'], { timeout: timeoutMs }, (err, stdout) => {
      if (err) return resolve(DEFAULT_MAC_ICON);
      try {
        const name = (JSON.parse(stdout) as { SPHardwareDataType?: { machine_name?: string }[] }).SPHardwareDataType?.[0]?.machine_name;
        resolve(iconForMachine(name));
      } catch {
        resolve(DEFAULT_MAC_ICON);
      }
    });
  });
}

// detectMachineIcon, remembered in <run>/machine-icon.json: system_profiler
// held each restart up 0.3 to 1 s (12 Nov) to learn what never changes. A
// remembered icon is used straight away and checked again in the
// background, for the next start; it's per host name, so a data dir
// carried to another Mac works it out again before using it.
export async function machineIcon(runDir: string, detect: () => Promise<string> = detectMachineIcon): Promise<string> {
  const file = path.join(runDir, 'machine-icon.json');
  const host = os.hostname();
  const remember = (icon: string) => {
    try {
      atomicWriteFileSync(file, JSON.stringify({ host, icon }) + '\n');
    } catch {
      // worked out again next time
    }
  };
  const saved = readJsonSync<{ host?: unknown; icon?: unknown }>(file);
  if (saved && saved.host === host && typeof saved.icon === 'string' && saved.icon) {
    const icon = saved.icon;
    void detect().then((now) => {
      if (now !== icon) remember(now);
    });
    return icon;
  }
  const icon = await detect();
  remember(icon);
  return icon;
}
