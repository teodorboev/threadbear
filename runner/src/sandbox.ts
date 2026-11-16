// Threads that run their CLI in a container (an experiment from 6 Nov 2026,
// docs/protocol.md "Sandboxed threads"). runner.json's `sandbox` names
// folders, and a thread under one of them has its Claude Code run inside
// `docker run` from `image`, with only its own folder, its login's config
// and the CLI itself mounted in, each at the same path as on the runner's
// machine. Everything else (the runner, its data and secrets, other repos)
// isn't there, so an agent with full access can only touch its thread's
// folder. The runner stays where it is: transcripts, pools, schedules and
// Convex sync don't change, and the threadbear tools and hooks reach the CLI
// over the same stdin and stdout as always.
//
// A sandbox lasts as long as the CLI does. When a thread goes idle the
// runner stops its CLI, the container goes with it (--rm), and the next
// message starts a fresh one that resumes the session from the login's
// config folder. Nothing outside the mounts survives that: an installed
// package, a running server.
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { execFile } from 'node:child_process';
import type { SpawnOptions, SpawnedProcess } from '@anthropic-ai/claude-agent-sdk';

export type SandboxConfig = {
  folders: string[]; // absolute, ~ expanded
  image: string;
  docker: string; // the docker binary
};

// runner.json's `sandbox`, or undefined when it's missing or unusable.
export function readSandbox(raw: unknown, expandHome: (p: string) => string): SandboxConfig | undefined {
  if (!raw || typeof raw !== 'object') return undefined;
  const r = raw as Record<string, unknown>;
  const folders = Array.isArray(r.folders) ? r.folders.filter((f): f is string => typeof f === 'string' && !!f.trim()).map((f) => path.resolve(expandHome(f.trim()))) : [];
  const image = typeof r.image === 'string' && r.image.trim() ? r.image.trim() : undefined;
  if (!folders.length || !image) return undefined;
  return { folders, image, docker: typeof r.docker === 'string' && r.docker.trim() ? r.docker.trim() : 'docker' };
}

const real = (p: string): string => {
  try {
    return fs.realpathSync(p);
  } catch {
    return path.resolve(p);
  }
};

const inside = (p: string, dir: string): boolean => p === dir || p.startsWith(dir.endsWith(path.sep) ? dir : dir + path.sep);

// Said when a thread shut in a sandbox (ThreadRecord.confined) would have
// to run outside one: its folder isn't in one now, or isn't really there.
export const CONFINED_NO_SANDBOX = (cwd: string) =>
  `This thread runs in a sandbox, but ${cwd} isn't in a sandbox folder now, so it wasn't started. Move it back into one, or start a new thread there.`;

// The sandbox a thread in `cwd` runs in, if its folder is under one of the
// configured ones.
export function sandboxFor(sb: SandboxConfig | undefined, cwd: string): SandboxConfig | undefined {
  if (!sb) return undefined;
  const at = real(cwd);
  return sb.folders.some((f) => inside(at, real(f))) ? sb : undefined;
}

// What of the CLI's environment goes into the container: an allowlist,
// never "everything but". The runner's environment is the user's login
// shell's (and whatever launched it), so it can hold any cloud, GitHub,
// signing or API key, an ssh-agent socket or a proxy with a password in
// it; forwarded, each was readable by the confined agent and usable over
// the container's network (Daybreak 124, 15 Nov 2026). So only what Claude
// Code itself needs to run its session goes in: its config folder (its
// login, read from there, as the CLI in a Linux container keeps it in a
// file, not the Keychain), the settings Threadbear and the SDK set for it
// (each by name),
// which model, the thread's id, and the terminal's locale. Nothing named
// like a credential goes in, even under an allowed prefix, so a token the
// runner's environment hands the CLI (CLAUDE_CODE_OAUTH_TOKEN,
// ANTHROPIC_API_KEY) stays out too: the sandbox's Claude signs in from its
// login's folder. THREADBEAR_THREAD_KEY never goes in (a confined thread
// isn't given one, and nothing in the sandbox should reach the runner).
// PATH, HOME, TMPDIR, USER and the like are the container's own, and
// THREADBEAR_NODE names the host's Node, which isn't in it: passed in,
// the repo's scripts would refuse it rather than use the container's own
// node (Codex's review, 9 Nov 2026). A new name here needs a reason it
// can't carry a secret; sandbox.test.ts lists what a sandboxed agent sees.
const SANDBOX_ENV = new Set([
  'CLAUDE_CONFIG_DIR',
  'CLAUDE_SECURESTORAGE_CONFIG_DIR',
  // What the runner sets for the CLI (adapter.ts), and what the Agent SDK
  // adds as it spawns it (sdk.mjs), by name: a prefix let through
  // CLAUDE_CODE_REMOTE_MCP_CONFIG (a remote MCP server's tokens),
  // CLAUDE_CODE_EXTRA_BODY and the like whenever the runner's environment
  // had them (Grok's review, 15 Nov 2026).
  'CLAUDE_AGENT_SDK_CLIENT_APP',
  'CLAUDE_AGENT_SDK_VERSION',
  'CLAUDE_CODE_ENTRYPOINT',
  'CLAUDE_CODE_TERMINAL_MCP_TOOLS',
  'CLAUDE_CODE_ADDITIONAL_DIRECTORIES_CLAUDE_MD',
  'CLAUDE_CODE_ENABLE_SDK_FILE_CHECKPOINTING',
  'CLAUDE_CODE_SDK_READS_SESSION_STATE',
  'CLAUDE_CODE_SDK_READS_STARTUP_MARKS',
  'CLAUDE_CODE_QUESTION_PREVIEW_FORMAT',
  'CLAUDE_CODE_SDK_HAS_OAUTH_REFRESH',
  'CLAUDE_CODE_SDK_HAS_HOST_AUTH_REFRESH',
  'ANTHROPIC_MODEL',
  'ANTHROPIC_SMALL_FAST_MODEL',
  'THREADBEAR_THREAD_ID',
  'THREADBARE_THREAD_ID',
  'THREADBEAR_RUNNER_VERSION',
  'LANG',
  'LANGUAGE',
  'TERM',
  'COLORTERM',
  'TZ',
  'NO_COLOR',
  'FORCE_COLOR',
]);
// The model defaults, and the locale's categories.
const SANDBOX_ENV_PREFIX = /^(ANTHROPIC_DEFAULT_[A-Z0-9_]*_MODEL$|LC_[A-Z]+$)/;
// Anything that looks like it carries a secret or reaches elsewhere stays
// out, whatever its prefix. Matched on the name's words, upper-cased.
const SANDBOX_ENV_DENY = /(^|_)(TOKEN|TOKENS|KEY|KEYS|SECRET|SECRETS|PASSWORD|PASSWD|PASS|PWD|CREDENTIAL|CREDENTIALS|CREDS|AUTH|OAUTH|AUTHORIZATION|COOKIE|PRIVATE|CERT|SOCK|SOCKET|PROXY|HEADERS|URL)($|_)/;
// Flags the SDK sets that only say what it can do ("1"), whose names
// happen to say AUTH.
const SANDBOX_ENV_FLAGS = new Set(['CLAUDE_CODE_SDK_HAS_OAUTH_REFRESH', 'CLAUDE_CODE_SDK_HAS_HOST_AUTH_REFRESH']);

// Whether the CLI's variable `name` is handed into the sandbox.
export function sandboxEnvAllowed(name: string): boolean {
  if (SANDBOX_ENV_FLAGS.has(name)) return true;
  const n = name.toUpperCase();
  if (n !== name) return false; // a lower-case http_proxy and friends
  if (SANDBOX_ENV_DENY.test(n)) return false;
  return SANDBOX_ENV.has(n) || SANDBOX_ENV_PREFIX.test(n);
}

export type DockerRun = { command: string; args: string[]; name: string };

// The `docker run` for one CLI start. Pure, so it can be tested without
// docker: `exists` says whether a path is there (for args the SDK hands the
// CLI by absolute path, like a settings file), `home` and the ids are the
// runner's user, whose files the container's user owns.
export function dockerRun(
  spawn: { command: string; args: string[]; cwd?: string; env: Record<string, string | undefined> },
  sb: SandboxConfig,
  ctx: { threadId: string; home: string; uid: number; gid: number; exists: (p: string) => boolean; realpath: (p: string) => string },
): DockerRun {
  const name = `tb-${ctx.threadId.slice(0, 8)}-${Math.random().toString(36).slice(2, 8)}`;
  const mounts = new Map<string, 'rw' | 'ro'>();
  const mount = (p: string, mode: 'rw' | 'ro') => {
    if (mounts.get(p) === 'rw') return;
    mounts.set(p, mode);
  };
  const cwd = ctx.realpath(spawn.cwd ?? ctx.home);
  // Checked again where the folder is mounted from: a link put in its
  // place since the thread was matched to the sandbox would mount
  // wherever it points (Daybreak, 7 Nov 2026).
  if (!sb.folders.some((f) => inside(cwd, ctx.realpath(f)))) throw new Error(CONFINED_NO_SANDBOX(spawn.cwd ?? ctx.home));
  mount(cwd, 'rw');
  // The login's config: sessions, credentials, checkpoints. ~/.claude.json
  // sits beside the default one.
  const configDir = spawn.env.CLAUDE_CONFIG_DIR || path.join(ctx.home, '.claude');
  if (ctx.exists(configDir)) mount(ctx.realpath(configDir), 'rw');
  if (!spawn.env.CLAUDE_CONFIG_DIR && ctx.exists(path.join(ctx.home, '.claude.json'))) mount(path.join(ctx.home, '.claude.json'), 'rw');
  // The CLI: the native installer's ~/.local/bin/claude is a link into its
  // versions folder, so the real file goes in and is what runs.
  const command = ctx.realpath(spawn.command);
  mount(command, 'ro');
  // Anything else the CLI is handed by absolute path that's on this machine.
  for (const a of spawn.args) if (path.isAbsolute(a) && ctx.exists(a)) mount(ctx.realpath(a), 'ro');
  const env: string[] = [];
  for (const k of Object.keys(spawn.env).sort()) if (spawn.env[k] !== undefined && sandboxEnvAllowed(k)) env.push('-e', k);
  const args = [
    'run', '-i', '--rm', '--init', '--name', name,
    '--user', `${ctx.uid}:${ctx.gid}`,
    '-e', `HOME=${ctx.home}`,
    ...env,
    ...[...mounts].flatMap(([p, mode]) => ['-v', `${p}:${p}${mode === 'ro' ? ':ro' : ''}`]),
    '-w', cwd,
    sb.image, command, ...spawn.args,
  ];
  return { command: sb.docker, args, name };
}

// A spawner (proc.ts's groupSpawner) whose CLI runs in the sandbox. The env
// is passed to `docker` itself, which hands each `-e NAME` on. Stopping the
// CLI stops the docker client; the container is killed too, as a client
// that's killed outright leaves its container running.
export function sandboxSpawner(base: (o: SpawnOptions) => SpawnedProcess, sb: SandboxConfig, threadId: string): (o: SpawnOptions) => SpawnedProcess {
  const me = os.userInfo();
  return (o) => {
    const run = dockerRun({ command: o.command, args: o.args, cwd: o.cwd, env: o.env as Record<string, string | undefined> }, sb, {
      threadId,
      home: os.homedir(),
      uid: me.uid,
      gid: me.gid,
      exists: (p) => fs.existsSync(p),
      realpath: real,
    });
    o.signal.addEventListener('abort', () => execFile(sb.docker, ['kill', run.name], () => {}), { once: true });
    return base({ ...o, command: run.command, args: run.args });
  };
}
