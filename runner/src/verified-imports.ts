// Imports a trusted project patch from the bytes its trust hash was made
// from, never from the folder as it is by then (docs/protocol.md,
// "Trusting a folder's patches"). Checking a patch's hash and then
// importing its path left a gap: a command that rewrote the patch, or a
// helper it imports, between the two ran unhashed code under the earlier
// yes (Codex review, 5 Nov).
//
// So the loader gets the patch's import graph as the bytes patchHash read
// (patch-trust.ts, `code`) and where each local import led (`edges`), and
// Node's module hooks serve exactly that. What an import from one of those
// files may load is decided by how it's written, not where it resolves to
// now:
//   - a local one (`./x`, `../x`, `/x`, `file:`) loads the very file the
//     hash followed it to, from the hashed bytes, without resolving it
//     again: a symlink retargeted since, even into node_modules, changes
//     nothing (Codex's second review, 5 Nov). One the hash didn't follow
//     refuses the patch.
//   - a bare one (a package, `node:fs`) resolves as Node resolves it,
//     and must land in a builtin or in that package's own folder in a
//     node_modules (`node_modules/zod/...`):
//     packages aren't hashed, by design, as `npm install` put them there.
//     One the repo commits is its own code, though: every file git tracks
//     under the repo's node_modules is hashed with the patch and served
//     from those bytes like a helper (tracked-packages.ts, patchHash).
//     Anything else is a file of the folder's that nothing hashed, so it
//     refuses the patch. Node resolves a bare name through the nearest
//     package.json's own `name` and `exports` before it looks in
//     node_modules, so a pull that renamed the repo's package to `zod`
//     sent `import 'zod'` to a script of the repo's, unasked (Gautam's
//     scan, 6 Nov); a workspace package (node_modules links to
//     packages/x, which Node follows) is one of the repo's files too.
//
// Each trusted version of a patch is a graph of its own: every file in it
// (the patch, its helpers, the packages its repo commits) is a module at
// its own path with the graph's id as the query, `file:///…/h.ts?tb=<id>`,
// and its edges, bytes and repo are kept under those URLs. Only the patch
// had a versioned URL once; its helpers were modules at their plain path,
// once per process, and their edges a map by path that the next version
// wrote over. So a newer trusted patch got an older trusted version's
// helper (Node had already loaded it), and an older one's late `import()`
// followed the newer one's edges: code that was in neither version anyone
// trusted (Daybreak 131, 15 Nov 2026). Now nothing is shared between
// graphs, and nothing a graph noted is changed by another. The query
// doesn't change the path, so `import.meta.filename`, `.dirname` and
// `new URL('./x', import.meta.url)` mean what they did (`import.meta.url`
// itself carries the query). CommonJS has no graph of its own in Node's
// cache, so a trusted patch's graph refuses it (load, below).
import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { registerHooks } from 'node:module';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { cutQueryAndHash } from './linear-text.ts';
import { repoTopOf, unhashedPackageFile } from './tracked-packages.ts';

// A module's graph-scoped URL to the bytes to serve for it.
const sources = new Map<string, Buffer>();
// A served module's URL to its local specifiers' targets (URLs, in the
// same graph).
const edges = new Map<string, Map<string, string>>();
// A served module's URL to its patch's repo top.
const tops = new Map<string, string>();
let installed = false;

const plain = (url: string) => cutQueryAndHash(url);

// The graph a served module's URL is in, by its `tb` query.
function graphOf(url: string): string | undefined {
  const q = url.indexOf('?');
  if (q < 0) return undefined;
  return new URLSearchParams(url.slice(q + 1).split('#')[0]).get('tb') ?? undefined;
}

// A file's URL in graph `g`.
const inGraph = (file: string, g: string) => `${pathToFileURL(file).href}?tb=${g}`;

// The URL to import a trusted patch at, after noting its code. `root` is
// the patch file's real path, `hash` its trust hash (so a new version is
// a new graph), `code` its import graph by real path, `links` each of
// those files' local specifiers to the real path the hash followed.
export function verifiedUrl(root: string, hash: string, code: Map<string, Buffer>, links: Map<string, Map<string, string>>): string {
  install();
  // By the patch's path as well as its hash, so two patches whose graphs
  // hash alike still share nothing.
  const g = crypto.createHash('sha256').update(`${root}\0${hash}`).digest('hex').slice(0, 40);
  const url = inGraph(root, g);
  // Noted already: the same path and hash is the same bytes and edges.
  if (sources.has(url)) return url;
  const top = repoTopOf(root);
  for (const [file, bytes] of code) {
    sources.set(inGraph(file, g), bytes);
    tops.set(inGraph(file, g), top);
  }
  for (const [from, specs] of links) {
    const to = new Map([...specs].map(([spec, target]) => [spec, inGraph(target, g)]));
    edges.set(inGraph(from, g), to);
  }
  return url;
}

// Only ever by the whole URL, graph and all: a plain path is no graph's.
function served(url: string): Buffer | undefined {
  return sources.get(url);
}

// How a specifier is written: local (a path or file: URL) or bare.
export function isLocalSpecifier(specifier: string): boolean {
  return /^\.{1,2}(\/|$)|^\//.test(specifier) || specifier.startsWith('file:');
}

// Where a bare import may lead from a served file: a builtin, or a file of
// that package's in a node_modules folder, `node_modules/<its name>/...`
// (its real path, as Node gives it; pnpm's store ends the same way). Not
// just any path with a node_modules in it: a self-reference's `exports`
// could name `./lib/node_modules/x.mjs`.
export function isPackageUrl(url: string, specifier: string): boolean {
  if (url.startsWith('node:')) return true;
  if (!url.startsWith('file:')) return false;
  let file: string;
  try {
    file = fileURLToPath(plain(url));
  } catch {
    return false;
  }
  const name = packageName(specifier).split('/');
  if (!name.every(Boolean)) return false;
  const segs = file.split(path.sep);
  for (let i = segs.indexOf('node_modules'); i >= 0; i = segs.indexOf('node_modules', i + 1)) {
    if (name.every((n, k) => segs[i + 1 + k] === n)) return true;
  }
  return false;
}

// A bare specifier's package: `zod` of `zod/v4`, `@ws/lib` of `@ws/lib/x`.
function packageName(specifier: string): string {
  const parts = specifier.split('/');
  return (specifier.startsWith('@') ? parts.slice(0, 2) : parts.slice(0, 1)).join('/');
}

// The `name` of the package a served file is in, as Node finds it for a
// self-reference: the nearest package.json up from it, not past a
// node_modules folder. Refused by name whatever its `exports` say, so a
// folder extracted under some node_modules/zod can't pass either (Codex's
// review, 6 Nov). Only a plain file is read (a fifo would hold the hook).
function ownPackage(file: string): string | undefined {
  for (let d = path.dirname(file); ; d = path.dirname(d)) {
    if (path.basename(d) === 'node_modules') return undefined;
    const pkg = path.join(d, 'package.json');
    try {
      if (fs.statSync(pkg).isFile()) {
        const name = (JSON.parse(fs.readFileSync(pkg, 'utf8')) as { name?: unknown }).name;
        return typeof name === 'string' ? name : undefined;
      }
    } catch {
      // none here, or not JSON: Node wouldn't self-resolve through it
      if (fs.existsSync(pkg)) return undefined;
    }
    if (path.dirname(d) === d) return undefined;
  }
}

// Why the package.json files Node resolved a committed package's file by
// aren't this graph's, or undefined when each is: every one in a folder
// under a node_modules between the file and the repo's top, as it is on
// disk now, must be one the graph has, byte for byte.
function packageJsonsDiffer(file: string, top: string, mine: (f: string) => Buffer | undefined): string | undefined {
  for (let d = path.dirname(file); d.startsWith(top) && d !== top; d = path.dirname(d)) {
    if (!path.relative(top, d).split(path.sep).includes('node_modules')) break;
    const pj = path.join(d, 'package.json');
    let now: Buffer;
    try {
      now = fs.readFileSync(pj);
    } catch {
      if (fs.existsSync(pj)) return `${pj} can't be read`;
      continue;
    }
    const kept = mine(pj);
    if (!kept || !kept.equals(now)) return `${pj} isn't the package.json patch trust hashed`;
  }
  return undefined;
}

function describe(url: string): string {
  try {
    return url.startsWith('file:') ? fileURLToPath(plain(url)) : url;
  } catch {
    return url;
  }
}

function install(): void {
  if (installed) return;
  installed = true;
  registerHooks({
    resolve(specifier, context, nextResolve) {
      const parent = context.parentURL;
      if (!parent || !served(parent)) return nextResolve(specifier, context);
      if (!isLocalSpecifier(specifier)) {
        const own = ownPackage(fileURLToPath(plain(parent)));
        if (own !== undefined && own === packageName(specifier)) throw new Error(`${fileURLToPath(plain(parent))} imports ${specifier}, its own package by name, which resolves through package.json to a file patch trust didn't hash: not loaded.`);
        const out = nextResolve(specifier, context);
        if (!isPackageUrl(out.url, specifier)) throw new Error(`${fileURLToPath(plain(parent))} imports ${specifier}, which resolves to ${describe(out.url)}, not a package in node_modules, and patch trust didn't hash it: not loaded.`);
        // One the repo commits loads only from hashed bytes (patchHash
        // hashes them all once a patch has a bare import), and from this
        // graph's: it's a module of the graph, like a helper.
        const top = tops.get(parent);
        const g = graphOf(parent)!;
        if (top && out.url.startsWith('file:')) {
          const file = fileURLToPath(plain(out.url));
          const why = unhashedPackageFile(file, top, (f) => !!served(inGraph(f, g)));
          if (why) throw new Error(`${fileURLToPath(plain(parent))} imports ${specifier}, and ${why}: not loaded.`);
          if (served(inGraph(file, g))) {
            // Node picked the file by the package.json on disk now, which
            // may be another trusted version's: its `exports` could send
            // this version's import to a file this version never loaded as
            // that (Grok's review of Daybreak 131). So the package.json it
            // went by must be this graph's, byte for byte.
            // Every package.json under node_modules on the way up from the
            // file counts, a nested package's and the one above it: an
            // `exports` can name ./node_modules/helper/index.js (Grok's
            // second review).
            const why = packageJsonsDiffer(file, top, (f) => served(inGraph(f, g)));
            if (why) throw new Error(`${fileURLToPath(plain(parent))} imports ${specifier}, and ${why}: not loaded.`);
            return { url: inGraph(file, g), format: out.format, shortCircuit: true };
          }
        }
        return out;
      }
      const key = specifier.startsWith('file:') ? fileURLToPath(specifier) : specifier;
      const target = edges.get(parent)?.get(key);
      const from = fileURLToPath(plain(parent));
      if (!target || !served(target)) throw new Error(`${from} imports ${specifier}, which patch trust didn't hash: not loaded.`);
      return { url: target, format: undefined, shortCircuit: true };
    },
    load(url, context, nextLoad) {
      const bytes = url.startsWith('file:') ? served(url) : undefined;
      if (!bytes) return nextLoad(url, context);
      // The format as Node would take it (from the extension and the
      // package.json); the source is the hashed bytes, whatever the
      // file holds now. Only a plain file is read for that (a fifo put
      // there would hang).
      if (!fs.statSync(fileURLToPath(plain(url))).isFile()) throw new Error(`${fileURLToPath(plain(url))} isn't a file now: not loaded.`);
      const out = nextLoad(url, context);
      // CommonJS is kept by Node once per process by its path, query or
      // not, so two trusted versions of one would share whichever ran
      // first: a graph of its own can only be ES modules (Grok's review of
      // Daybreak 131, 15 Nov 2026). A `.ts` or `.js` written with
      // import/export is one, as Node detects it.
      if (typeof out.format === 'string' && out.format.includes('commonjs')) throw new Error(`${fileURLToPath(plain(url))} is CommonJS, which Node loads once per process whichever version of it was trusted, so a trusted patch can't load it: not loaded. Make it an ES module (import and export, or .mjs).`);
      return { ...out, source: bytes, shortCircuit: true };
    },
  });
}
