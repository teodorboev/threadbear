// What makes Grok ask Threadbear before it runs a tool.
//
// Grok has no setting a client can force that makes it always ask. Its own
// modes come from its config and from ~/.claude/settings.json: tried on 1.0.77
// (15 Nov 2026), a user whose Claude settings say `defaultMode: "auto"` got
// Grok's auto mode, and a file write and `echo ran > shell.txt` ran with no
// session/request_permission at all, its classifier allowing them. Nothing in
// the environment, GROK_CONFIG or session/new's _meta turned that off.
//
// What does hold is a PreToolUse hook answering "ask": Grok then sends the
// call to the client's permission prompt, and nothing of its own (auto mode,
// always-approve, a saved grant, a read-only command, a folder's allow rules)
// approves it instead (its docs, hooks "Output", and tried the same day with a
// folder whose .claude/settings.json said bypassPermissions and whose
// .grok/config.toml allowed everything). So every Grok process starts with
// `--plugin-dir` naming a plugin of Threadbear's holding that hook. A plugin
// dir given that way is trusted, and is ignored in Grok's leader mode, hence
// `--no-leader` (agents.ts).
//
// A folder can turn a plugin off by name (`[plugins] disabled` in its
// .grok/config.toml, which Grok reads untrusted, with no wildcards: tried the
// same day, and a write then ran unasked). So each start gets a plugin of its
// own, named at random, and the plugin also has a UserPromptSubmit hook: Grok
// reports every hook it ran, and a turn in which Threadbear's didn't run is
// stopped before the model's first word (AcpSession, guardSeen).
//
// A folder Grok trusts (~/.grok/trusted_folders.toml) runs its own hooks
// before anything asks: a SessionStart hook runs as the session opens, and a
// PreToolUse hook's rewritten input wins over what this hook saw (tried: a
// read of note.txt read a file outside the folder, with no card). So in such
// a folder the hook asks for every call, reads too, and a thread that has to
// ask doesn't start there at all (agents.ts, refuses).
//
// Whether a folder was trusted is settled when a process starts and kept for
// its life: its hooks loaded then, and a repo's own SessionStart hook could
// take the folder out of the trust file afterwards. So that start's plugin
// says so in its hook's arguments, and its thread can't leave Full.
// Grok also counts your home folder and a disk's root as trusted, so they are
// here too.
//
// A Full access thread could rewrite guard-ask.mjs, shared by every start, but
// it could as well rewrite the runner itself, so that adds nothing.
//
// The hook asks for every tool except a read inside the thread's folder,
// which goes ahead as Grok's own read-only tools do anywhere. For a small job
// (noTools) it denies everything, reads too, from a script of its own. Each
// fails closed: if it can't run or answers nonsense, the call is denied.
import crypto from 'node:crypto';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

// The folders ~/.grok/trusted_folders.toml trusts, as `[folders."<path>"]`
// tables with `trusted = true`. Undefined when the file is there but can't be
// read, which counts as trusting everything. Kept the same as the copy in
// ASK_SCRIPT below.
export function grokTrustedFolders(grokHome: string): string[] | undefined {
  let text: string;
  try {
    text = fs.readFileSync(path.join(grokHome, 'trusted_folders.toml'), 'utf8');
  } catch (e) {
    return (e as NodeJS.ErrnoException).code === 'ENOENT' ? [] : undefined;
  }
  const out: string[] = [];
  let at: string | undefined;
  for (const line of text.split('\n')) {
    const head = /^\s*\[folders\."((?:[^"\\]|\\.)*)"\]\s*$/.exec(line);
    if (head) {
      at = head[1].replace(/\\(.)/g, '$1');
      continue;
    }
    if (/^\s*\[/.test(line)) at = undefined;
    else if (at !== undefined && /^\s*trusted\s*=\s*true\s*(#.*)?$/.test(line)) out.push(at);
  }
  // Something says trusted that wasn't understood: assume the worst.
  if (/trusted\s*=\s*true/.test(text) && !out.length) return undefined;
  return out;
}

function onDisk(p: string): string | undefined {
  let rest = '';
  let at = p;
  for (;;) {
    try {
      return path.join(fs.realpathSync(at), rest);
    } catch {
      const up = path.dirname(at);
      if (up === at) return undefined;
      rest = path.join(path.basename(at), rest);
      at = up;
    }
  }
}

// Whether Grok trusts this folder, or a folder above it. Grok counts the home
// folder and a disk's root as trusted whatever the file says.
export function grokTrusts(cwd: string, grokHome: string = process.env.GROK_HOME || path.join(os.homedir(), '.grok'), home: string = os.homedir()): boolean {
  const here = onDisk(cwd) ?? cwd;
  if (here === path.parse(here).root || here === (onDisk(home) ?? home)) return true;
  const folders = grokTrustedFolders(grokHome);
  if (!folders) return true;
  return folders.some((f) => {
    const t = onDisk(f) ?? f;
    return here === t || here.startsWith(t.endsWith(path.sep) ? t : t + path.sep);
  });
}

// Run by the runner's own node, never by anything a thread's folder supplies.
// Kept free of imports beyond node's, as it's written out and run on its own.
export const ASK_SCRIPT = `// Threadbear's guard for Grok, written by runner/src/adapters/acp/grok-guard.ts.
// Grok runs it before every tool call. "ask" sends the call to Threadbear,
// whatever Grok's own mode and rules would have done.
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

// Grok's tools that only look, which may go ahead when all they name is
// inside the thread's folder: its file reads and searches, the search of its
// own tool list (how it finds an MCP tool), its to-do list, and the output of
// a command it already ran. Anything else asks, a tool added later included,
// and so does the web (a search can carry what it read away).
const READS = new Set(['read_file', 'list_dir', 'grep', 'search_tool', 'todo_write', 'get_command_or_subagent_output']);
const PATH_KEYS = new Set(['target_file', 'target_directory', 'path', 'paths', 'file_path', 'file', 'files', 'directory', 'dir', 'cwd', 'root']);

function say(decision, reason) {
  process.stdout.write(JSON.stringify({ decision, reason }));
}

// As the disk has it, links followed; a path that isn't there yet resolves
// through the nearest folder that is.
function onDisk(p) {
  let rest = '';
  let at = p;
  for (;;) {
    try {
      return path.join(fs.realpathSync(at), rest);
    } catch {
      const up = path.dirname(at);
      if (up === at) return undefined;
      rest = path.join(path.basename(at), rest);
      at = up;
    }
  }
}

function inside(p, root) {
  return p === root || p.startsWith(root.endsWith(path.sep) ? root : root + path.sep);
}

// The same as grokTrustedFolders in grok-guard.ts.
function trustedFolders() {
  const home = process.env.GROK_HOME || path.join(os.homedir(), '.grok');
  let text;
  try {
    text = fs.readFileSync(path.join(home, 'trusted_folders.toml'), 'utf8');
  } catch (e) {
    return e && e.code === 'ENOENT' ? [] : undefined;
  }
  const out = [];
  let at;
  for (const line of text.split('\\n')) {
    const head = /^\\s*\\[folders\\."((?:[^"\\\\]|\\\\.)*)"\\]\\s*$/.exec(line);
    if (head) {
      at = head[1].replace(/\\\\(.)/g, '$1');
      continue;
    }
    if (/^\\s*\\[/.test(line)) at = undefined;
    else if (at !== undefined && /^\\s*trusted\\s*=\\s*true\\s*(#.*)?$/.test(line)) out.push(at);
  }
  if (/trusted\\s*=\\s*true/.test(text) && !out.length) return undefined;
  return out;
}

function decide(e) {
  const name = e.toolName ?? e.tool_name;
  if (!READS.has(name) || e.toolInputTruncated === true) return 'ask';
  const root = typeof e.cwd === 'string' && path.isAbsolute(e.cwd) ? onDisk(e.cwd) : undefined;
  if (!root) return 'ask';
  // A folder Grok trusts runs its own hooks, and one of those can rewrite
  // what this call reads after this has looked: everything asks there.
  // Trusted when this process started stays so (its hooks loaded then), and
  // Grok trusts the home folder and a disk's root as well.
  if (process.argv[2] === 'trusted') return 'ask';
  if (root === path.parse(root).root || root === (onDisk(os.homedir()) ?? os.homedir())) return 'ask';
  const trusted = trustedFolders();
  if (!trusted || trusted.some((f) => inside(root, onDisk(f) ?? f))) return 'ask';
  const input = e.toolInput ?? e.tool_input ?? {};
  if (!input || typeof input !== 'object' || Array.isArray(input)) return 'ask';
  for (const [k, v] of Object.entries(input)) {
    if (v === null || typeof v === 'number' || typeof v === 'boolean') continue;
    const values = Array.isArray(v) ? v : [v];
    for (const s of values) {
      if (typeof s !== 'string') return 'ask';
      // A .. anywhere asks: the system follows a link before it goes up, so
      // link/.. can be anywhere, while a path tidied first can't tell.
      if (s.split(/[\\\\/]/).includes('..')) return 'ask';
      // A named place, or anything that reads like one (a pattern starting
      // at / or ~): checked as a path.
      const looks = PATH_KEYS.has(k) || s.startsWith('/') || s.startsWith('~');
      if (!looks || s === '') continue;
      if (s.startsWith('~')) return 'ask';
      const where = onDisk(path.isAbsolute(s) ? s : root + path.sep + s);
      if (!where || !inside(where, root)) return 'ask';
    }
  }
  return 'allow';
}

let raw = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', (d) => {
  raw += d;
});
process.stdin.on('end', () => {
  let decision = 'ask';
  try {
    decision = decide(JSON.parse(raw));
  } catch {
    decision = 'ask';
  }
  if (decision === 'allow') return say('allow', 'A read inside the thread\\'s folder.');
  say('ask', 'Threadbear decides.');
});
`;

// A small job's: every call is denied, whatever it is.
export const DENY_SCRIPT = `// Threadbear's guard for Grok's small jobs (runner/src/adapters/acp/grok-guard.ts).
process.stdin.resume();
process.stdin.on('end', () => process.stdout.write(JSON.stringify({ decision: 'deny', reason: 'Tools are off for this job.' })));
`;

// Run as each prompt goes in, so Grok reports that Threadbear's plugin is
// loaded before the model can do anything.
export const SEEN_SCRIPT = `// Threadbear's sign that its Grok plugin loaded (runner/src/adapters/acp/grok-guard.ts).
process.stdin.resume();
`;

const SCRIPTS = { 'guard-ask.mjs': ASK_SCRIPT, 'guard-deny.mjs': DENY_SCRIPT, 'seen.mjs': SEEN_SCRIPT };

// How long an old start's plugin is kept. A Grok process reads its plugin
// while it runs; one that outlives this has its turns stopped (guardSeen).
const KEEP_MS = 7 * 24 * 60 * 60 * 1000;

let fallbackRoot: string | undefined;

// The folder all of it lives in: a real folder of this user's, 0700, never a
// link to somewhere else (one is removed and the folder made afresh).
function guardRoot(dataDir: string | undefined): string {
  if (!dataDir) {
    fallbackRoot ??= fs.mkdtempSync(path.join(os.tmpdir(), 'threadbear-grok-guard-'));
    return fallbackRoot;
  }
  fs.mkdirSync(dataDir, { recursive: true });
  const root = path.join(dataDir, 'grok-guard');
  let st: fs.Stats | undefined;
  try {
    st = fs.lstatSync(root);
  } catch {
    st = undefined;
  }
  const uid = process.getuid?.();
  if (st && (!st.isDirectory() || (uid !== undefined && st.uid !== uid))) {
    fs.rmSync(root, { recursive: true, force: true });
    st = undefined;
  }
  if (!st) fs.mkdirSync(root, { mode: 0o700 });
  fs.chmodSync(root, 0o700);
  return root;
}

// Always written beside and renamed over: a rename replaces a link itself,
// never what it points at, and a Grok starting meanwhile never reads half.
function writeFresh(file: string, text: string): void {
  const tmp = `${file}.${crypto.randomBytes(6).toString('hex')}.tmp`;
  fs.writeFileSync(tmp, text, { mode: 0o600, flag: 'wx' });
  fs.renameSync(tmp, file);
}

function sweep(root: string, now: number): void {
  for (const name of fs.readdirSync(root)) {
    if (!/^threadbear-[0-9a-f]{16}$/.test(name)) continue;
    const p = path.join(root, name);
    try {
      if (now - fs.lstatSync(p).mtimeMs > KEEP_MS) fs.rmSync(p, { recursive: true, force: true });
    } catch {
      // gone already
    }
  }
}

// A plugin folder for one Grok process, made before it starts and named at
// random, so a folder's config can't name it to turn it off. Under the
// runner's data dir; with none (a test), a private temp folder of this
// process's own.
export function grokGuardDir(o: { dataDir?: string; noTools?: boolean; trusted?: boolean } = {}): string {
  const root = guardRoot(o.dataDir);
  for (const [name, text] of Object.entries(SCRIPTS)) writeFresh(path.join(root, name), text);
  sweep(root, Date.now());
  const plugin = path.join(root, `threadbear-${crypto.randomBytes(8).toString('hex')}`);
  fs.mkdirSync(path.join(plugin, 'hooks'), { recursive: true, mode: 0o700 });
  // The runner's own node, by its whole path: a thread's PATH can't swap it.
  const run = (script: string, failClosed: boolean, args: string[] = []) => ({ type: 'command', command: process.execPath, args: [path.join(root, script), ...args], timeout: 30, failClosed });
  const hooks = {
    hooks: {
      PreToolUse: [{ hooks: [o.noTools ? run('guard-deny.mjs', true) : run('guard-ask.mjs', true, o.trusted ? ['trusted'] : [])] }],
      UserPromptSubmit: [{ hooks: [run('seen.mjs', false)] }],
    },
  };
  writeFresh(path.join(plugin, 'hooks', 'hooks.json'), `${JSON.stringify(hooks, null, 2)}\n`);
  return plugin;
}

// The name Grok gives this plugin's hooks in what it reports
// ("plugin/<name>/hooks:pre_tool_use[0].hooks[0]"), from a process's args.
export function guardPluginName(args: string[]): string | undefined {
  const at = args.indexOf('--plugin-dir');
  return at >= 0 && args[at + 1] ? path.basename(args[at + 1]) : undefined;
}
