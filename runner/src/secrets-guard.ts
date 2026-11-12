// Keeps a thread that isn't on Full access from reading the runner's secrets
// with Claude's own tools (docs/protocol.md, "Who's really calling"). Defence in
// depth behind the RPC's own check of the caller's process: a shell that's
// allowed to run anything can spell a path in ways no pattern will see, and
// SECURITY.md says so. A Full access thread can already run anything as you,
// so it isn't checked.
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

const REASON = "The runner's secrets aren't readable from a thread without Full access. tb in this thread already works with its own key.";

const GREP_REASON = "This search's folder holds the runner's secrets, which aren't readable from a thread without Full access. Search a folder further in instead.";

const PATH_KEYS = ['file_path', 'path', 'notebook_path'] as const;

function expand(p: string): string {
  return p === '~' || p.startsWith('~/') ? path.join(os.homedir(), p.slice(1)) : p;
}

// The deepest part of a path that exists, with links followed, and the rest
// on the end: a Read of secrets/runner-token through a link in the path is
// that file.
function resolved(p: string, cwd?: string): string {
  let head = path.resolve(cwd ?? process.cwd(), expand(p));
  const tail: string[] = [];
  for (;;) {
    try {
      return path.join(fs.realpathSync.native(head), ...tail);
    } catch {
      const up = path.dirname(head);
      if (up === head) return path.join(head, ...tail);
      tail.unshift(path.basename(head));
      head = up;
    }
  }
}

// Paths and commands are compared without regard to case: APFS as it's usually
// made (and NTFS) opens the same folder whatever the letters. Where the file
// system tells them apart, a folder of the same name in other letters is
// refused too, which nothing has a reason to read.
// Unicode is folded to one form first too: APFS opens "é" whether it's written
// as one character or as "e" and an accent (cross-lab review, 11 Nov 2026).
const fold = (s: string) => s.normalize('NFC').toLowerCase();
const same = (a: string, b: string) => fold(a) === fold(b);
const inside = (p: string, dir: string) => {
  const [low, root] = [fold(p), fold(dir)];
  // `/` already ends in the separator: `//` would never match.
  return low === root || low.startsWith(root.endsWith(path.sep) ? root : root + path.sep);
};

// The reason a call is denied, or undefined if it's fine. `home` is the
// runner's data dir.
export function denySecrets(tool: string, input: Record<string, unknown>, home: string, cwd?: string): string | undefined {
  const secrets = path.join(home, 'secrets');
  if (tool === 'Bash') {
    const command = typeof input.command === 'string' ? input.command : '';
    // Quotes and backslashes that don't change which path it is.
    const bare = command.replace(/\\(.)/g, '$1').replace(/["']/g, '');
    // The token file's name first, before anything below folds a letter:
    // NFC and lower-casing both turn the Kelvin sign (K, U+212A) into a K,
    // and `runner-tokenK` (which a shell's ${x%K} cuts back to the file's
    // name) lost its word boundary that way (cross-lab review, 11 Nov 2026).
    if (/\brunner-token\b/i.test(bare)) return REASON;
    // Then the ways a shell writes the same folder: `/./` and `//`, an accent
    // as one character or two, and in any letters (see `same`). Checked as
    // written and also with `${X:-y}` or `${X%y}` read as `${X}`, which names
    // the same folder whenever X is set; as written too, as the fallback can
    // hold the path itself (`${X:-$THREADBEAR_HOME/secrets/...}`). The
    // modifier can hold a `$` (`${THREADBEAR_HOME:-$HOME}`) and is bounded,
    // so the match stays linear; a longer one gets past, as any spelling a
    // pattern doesn't know does. Each is checked without NFC as well, as NFC
    // can join an accent onto the s of secrets that the shell then cuts off
    // (`${p%<accent>}`, the third pass).
    const tidy = (c: string) => c.replace(/\/(?:\.\/)+/g, '/').replace(/\/{2,}/g, '/');
    const read = bare.replace(/\$\{(\w+)(?!\w)[^}\n]{0,300}\}/g, '${$1}');
    const spellings = [bare, read].flatMap((c) => [tidy(c).toLowerCase(), fold(tidy(c))]);
    const home2 = os.homedir();
    // The variables every thread is given for the data dir (threadEnv in
    // restart.ts), the old names too, as a shell writes them.
    const given = ['THREADBEAR_HOME', 'THREADBEAR_RUNNER_HOME', 'THREADBARE_HOME', 'THREADBARE_RUNNER_HOME'].flatMap((v) => [`$${v}`, `\${${v}}`]);
    const forms = [
      secrets,
      secrets.replace(home2, '~'),
      secrets.replace(home2, '$HOME'),
      secrets.replace(home2, '${HOME}'),
      ...given.map((v) => `${v}/secrets`),
      'Threadbear/secrets',
    ].map(fold);
    if (spellings.some((c) => /\brunner-token\b/.test(c) || forms.some((f) => c.includes(f)))) return REASON;
    // The data dir named with a wildcard after it reaches the same files
    // without those words (secr*/*).
    const dataDir = [home, home.replace(home2, '~'), home.replace(home2, '$HOME'), ...given, 'Application Support/Threadbear'].map(fold);
    if (spellings.some((c) => dataDir.some((d) => c.includes(d))) && /[*?[]/.test(bare)) return REASON;
    return undefined;
  }
  const target = resolved(secrets);
  for (const k of PATH_KEYS) {
    const v = input[k];
    if (typeof v === 'string' && v && (inside(resolved(v, cwd), target) || inside(path.resolve(cwd ?? process.cwd(), expand(v)), secrets))) return REASON;
  }
  // A Glob or Grep from the data dir itself reaches the folder whatever its
  // pattern says. From further up, a Glob only lists names, so it's refused
  // when its pattern names the folder; a Grep reads what's in every file it
  // passes, and its glob, not its pattern, picks the files (`pattern: "."`
  // with `glob: "Threadbear/secrets/runner-token"` printed the token, cross-lab
  // review, 11 Nov 2026), so it's refused whenever the folder is under it.
  if (tool === 'Glob' || tool === 'Grep') {
    const base = typeof input.path === 'string' && input.path ? input.path : (cwd ?? '.');
    const dir = resolved(base, cwd);
    const pattern = typeof input.pattern === 'string' ? input.pattern : '';
    if (same(dir, resolved(home))) return REASON;
    if (inside(target, dir) && (tool === 'Grep' || /\bsecrets\b|runner-token/i.test(pattern))) return GREP_REASON;
  }
  return undefined;
}
