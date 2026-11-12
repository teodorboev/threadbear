// Temp folders a scenario makes for the app to work in, and their removal.
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

const made: string[] = [];

/**
 * A new temp folder, by its real path, so what an app resolves reads the
 * same. `realpathSync.native`, as the JS one leaves Windows' short names
 * alone: GitHub's runner's temp folder is C:\Users\RUNNER~1\..., which its
 * shell and the app show as C:\Users\runneradmin\..., so every check of a
 * folder or file the app showed missed there (12 Nov). On a Mac it's the
 * /private/var/... of /var/....
 *
 * It's removed after the scenario's app has stopped (`removeTempFolders`,
 * world.ts): Windows won't remove a folder a process is still in, and the
 * Terminal's shell is in its folder until the app goes.
 */
export function tempFolder(prefix: string): string {
  const dir = fs.realpathSync.native(fs.mkdtempSync(path.join(os.tmpdir(), prefix)));
  made.push(dir);
  return dir;
}

export function removeTempFolders(): void {
  for (const dir of made.splice(0)) {
    // A shell the app started may take a moment to go after it. A failure
    // after these retries is still a leak to fix, even on Windows.
    fs.rmSync(dir, { recursive: true, force: true, maxRetries: 10, retryDelay: 200 });
  }
}
