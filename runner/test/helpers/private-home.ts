// Unit tests may inspect fixture accounts, never the machine's accounts.
// HOME alone is insufficient: a copied environment can still name live
// CODEX_HOME/XDG folders, and a caught filesystem error can hide a leak.
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { syncBuiltinESMExports } from 'node:module';
import childProcess from 'node:child_process';

// A container's uid may have no passwd entry, or name a home that doesn't
// exist. Preserve both the OS home and the caller's HOME when available.
const homes = new Set([process.env.HOME, process.env.USERPROFILE].filter((p): p is string => Boolean(p)));
// A test worker starts after its parent's preload already replaced HOME.
// Keep the first caller's homes guarded through every later preload.
try {
  const inherited: unknown = JSON.parse(process.env.THREADBEAR_TEST_GUARDED_HOMES ?? '[]');
  if (Array.isArray(inherited)) for (const home of inherited) if (typeof home === 'string' && home) homes.add(home);
} catch { /* A standalone run can still guard its own HOME. */ }
try { homes.add(os.userInfo().homedir); } catch { /* HOME is enough in a uid-only container. */ }
const resolve = fs.realpathSync.native;
for (const home of [...homes]) {
  try { homes.add(resolve(home)); } catch { /* Still guard its lexical path. */ }
}
process.env.THREADBEAR_TEST_GUARDED_HOMES = JSON.stringify([...homes]);
const codeRoot = resolve(path.join(import.meta.dirname, '../../..'));
const testHome = resolve(fs.mkdtempSync(path.join(os.tmpdir(), 'tb-unit-home-')));
process.env.HOME = testHome;
process.env.USERPROFILE = testHome;
// On check hosts TMPDIR is under the real home. Trust scans walk a
// fixture's ancestors up to HOME, so its temp files must be below HOME too.
// Private whatever the umask: a 002 umask (the cloud box) made it 775, and
// private-dir.mjs then refused every folder under it as shared.
const temp = path.join(testHome, 'tmp');
fs.mkdirSync(temp, { mode: 0o700 });
for (const key of ['TMPDIR', 'TMP', 'TEMP']) process.env[key] = temp;
for (const [name, suffix] of Object.entries({ XDG_CONFIG_HOME: '.config', XDG_DATA_HOME: '.local/share', XDG_CACHE_HOME: '.cache', XDG_STATE_HOME: '.local/state', CODEX_HOME: '.codex' })) {
  process.env[name] = path.join(testHome, suffix);
}
// A test that needs an override supplies its own; never inherit the live
// runner's data, account folders, config files or thread credentials.
for (const name of Object.keys(process.env)) {
  if (/^THREAD(?:BEAR|BARE)_(?:HOME|DATA_DIR|RUNNER_HOME|CLAUDE_MAIN_DIR|CLAUDE_MAIN_JSON|ACCOUNTS_DIR|CODEX_MAIN_DIR|TOKEN_STATS_LIMITS|THREAD_KEY|THREAD_ID|URL|LOG)$/.test(name) || /^OPENCODE_CONFIG(?:_DIR|_CONTENT)?$/.test(name)) delete process.env[name];
}

const roots = [...homes].flatMap((home) => ['.claude', '.claude.json', '.codex', '.cursor', '.opencode', '.config/opencode', '.local/share/opencode', 'Library/Application Support/Token Stats', 'Library/Application Support/Threadbear'].map((p) => path.join(home, p)));
const violations = new Map<string, string>();
const canonical = new Map<string, string>();
let clearQueued = false;
function forbidden(file: string): boolean {
  return roots.some((root) => file === root || file.startsWith(root + path.sep));
}
function reject(file: string): never {
  const error = new Error(`Unit test reached a real account folder: ${file}`);
  if (!violations.has(file)) violations.set(file, error.stack ?? error.message);
  throw error;
}
function canonicalPath(named: string): string {
  // The kernel refuses paths this long. Do not walk thousands of missing
  // ancestors in malformed-path tests just to discover the same failure.
  if (Buffer.byteLength(named) > 4096) return named;
  // Reuse ancestors during one synchronous scan. Clear on filesystem
  // changes and before another turn can observe a subprocess's changes.
  if (!clearQueued) {
    clearQueued = true;
    queueMicrotask(() => { canonical.clear(); clearQueued = false; });
  }
  let ancestor = named;
  const missing: string[] = [];
  while (true) {
    try {
      const real = canonical.get(ancestor) ?? resolve(ancestor);
      canonical.set(ancestor, real);
      for (const p of missing) canonical.set(p, path.join(real, path.relative(ancestor, p)));
      return path.join(real, path.relative(ancestor, named));
    } catch (error) {
      if ((error as NodeJS.ErrnoException).code === 'ENAMETOOLONG') return named;
    }
    missing.push(ancestor);
    const parent = path.dirname(ancestor);
    if (parent === ancestor) return named;
    ancestor = parent;
  }
}
// A thread inherits the installed release's bin directory. Probing every
// PATH entry must not probe that release or accidentally invoke its tb.
process.env.PATH = (process.env.PATH ?? '').split(path.delimiter).filter((p) => !p || (!forbidden(path.resolve(p)) && !forbidden(canonicalPath(path.resolve(p))))).join(path.delimiter);

function check(file: unknown): void {
  if (typeof file !== 'string' && !Buffer.isBuffer(file) && !(file instanceof URL)) return;
  const named = path.resolve(file instanceof URL ? fileURLToPath(file) : file.toString());
  // Resolve existing ancestors too, so a fixture symlink cannot lead back
  // into a real login, even when the requested leaf doesn't exist yet.
  // The runner also checks its own update clone, inside its real data
  // dir. Its source and dependencies are fixtures, not live runner data.
  if (forbidden(named) && named !== codeRoot && !named.startsWith(codeRoot + path.sep)) reject(named);
  const canonical = canonicalPath(named);
  if (forbidden(canonical) && canonical !== codeRoot && !canonical.startsWith(codeRoot + path.sep)) reject(named);
}

// Patch both forms: several production callers swallow ENOENT, so the
// exit check below also fails the file after any attempted forbidden read.
for (const target of [fs, fs.promises]) {
  for (const name of ['access', 'exists', 'lstat', 'open', 'openAsBlob', 'opendir', 'readFile', 'readdir', 'readlink', 'realpath', 'stat', 'statfs', 'watch', 'watchFile', 'createReadStream', 'writeFile', 'appendFile', 'mkdir', 'rm', 'rmdir', 'unlink', 'chmod', 'chown', 'utimes', 'truncate', 'createWriteStream', 'copyFile', 'cp', 'rename', 'link', 'symlink']) {
    for (const key of [name, name + 'Sync']) {
      const descriptor = Object.getOwnPropertyDescriptor(target, key);
      if (typeof descriptor?.value !== 'function') continue;
      const original = descriptor.value;
      const mutates = /^(?:open|write|append|mk|rm|unlink|chmod|chown|utimes|truncate|createWrite|copy|cp|rename|link|symlink)/.test(name);
      const guarded = function (this: unknown, file: unknown, ...args: unknown[]) {
        if (mutates) canonical.clear();
        try {
          // A relative symlink target is relative to the new link, not
          // cwd, and creating it reads no target. Guard the destination;
          // later reads through the link still check its resolved target.
          if (name !== 'symlink') check(file);
          if (['copyFile', 'cp', 'rename', 'link', 'symlink'].includes(name)) check(args[0]);
        } catch (error) {
          if (target === fs.promises) return Promise.reject(error);
          throw error;
        }
        if (mutates && typeof args.at(-1) === 'function') {
          const callback = args.at(-1) as (...values: unknown[]) => unknown;
          args[args.length - 1] = (...values: unknown[]) => { canonical.clear(); return callback(...values); };
        }
        const result = Reflect.apply(original, this, [file, ...args]);
        if (mutates) {
          canonical.clear();
          if (target === fs.promises) return (result as Promise<unknown>).finally(() => canonical.clear());
        }
        return result;
      };
      // fs.realpathSync.native is used to canonicalise paths too.
      if (typeof original.native === 'function') Object.assign(guarded, { native: (file: unknown, ...args: unknown[]) => { check(file); return original.native(file, ...args); } });
      Object.defineProperty(target, key, { ...descriptor, value: guarded });
    }
  }
}
// Synchronous git/ln calls can replace a link without yielding to the
// microtask that clears the cache, so invalidate around them as well.
for (const key of ['spawnSync', 'execSync', 'execFileSync'] as const) {
  const original = childProcess[key];
  Object.defineProperty(childProcess, key, { configurable: true, writable: true, value: (...args: unknown[]) => {
    canonical.clear();
    try { return Reflect.apply(original, childProcess, args); } finally { canonical.clear(); }
  } });
}
syncBuiltinESMExports();
process.on('exit', () => {
  if (violations.size) {
    console.error('Unit tests accessed real account folders:\n' + [...violations.values()].join('\n'));
    process.exitCode = 1;
  }
  // A child finishing its shutdown can still have a file open here.
  // Cleanup must not hide a violation or fail an otherwise passing test.
  try { fs.rmSync(testHome, { recursive: true, force: true, maxRetries: 3, retryDelay: 20 }); } catch { /* Temporary files can be swept later. */ }
});
