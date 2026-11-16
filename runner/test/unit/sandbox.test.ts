// Sandboxed threads (sandbox.ts): which threads run their CLI in a
// container, and the `docker run` that does it.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { dockerRun, readSandbox, sandboxFor } from '../../src/sandbox.ts';
import { tmpDir } from '../helpers/fakes.ts';

const home = '/home/alex';
const expand = (p: string) => (p.startsWith('~/') ? path.join(home, p.slice(2)) : p);

test('runner.json sandbox: folders and an image, else nothing', () => {
  assert.deepEqual(readSandbox({ folders: ['~/sandboxed'], image: 'threadbear-sandbox' }, expand), { folders: ['/home/alex/sandboxed'], image: 'threadbear-sandbox', docker: 'docker' });
  assert.equal(readSandbox({ folders: [], image: 'x' }, expand), undefined);
  assert.equal(readSandbox({ folders: ['~/s'] }, expand), undefined);
  assert.equal(readSandbox(undefined, expand), undefined);
  assert.equal(readSandbox({ folders: ['~/s'], image: 'x', docker: '/usr/bin/podman' }, expand)?.docker, '/usr/bin/podman');
});

test('a thread is sandboxed when its folder is under a sandbox folder, and not a sibling that only starts the same', () => {
  const root = tmpDir();
  const box = path.join(root, 'sandboxed');
  fs.mkdirSync(path.join(box, 'demo'), { recursive: true });
  fs.mkdirSync(path.join(root, 'sandboxed-not'), { recursive: true });
  const sb = { folders: [box], image: 'i', docker: 'docker' };
  assert.equal(sandboxFor(sb, path.join(box, 'demo')), sb);
  assert.equal(sandboxFor(sb, box), sb);
  assert.equal(sandboxFor(sb, path.join(root, 'sandboxed-not')), undefined);
  assert.equal(sandboxFor(undefined, box), undefined);
});

test('the docker run mounts the folder, the login and the CLI at the same paths, and nothing else of the machine', () => {
  const there = new Set(['/home/alex/.claude', '/home/alex/.claude.json', '/tmp/settings-abc.json']);
  const run = dockerRun(
    {
      command: '/home/alex/.local/bin/claude',
      args: ['--output-format', 'stream-json', '--settings', '/tmp/settings-abc.json', '--resume', 'abc'],
      cwd: '/home/alex/sandboxed/demo',
      env: { PATH: '/usr/bin', HOME: '/home/alex', ANTHROPIC_MODEL: 'x', THREADBEAR_THREAD_ID: 't1', THREADBEAR_NODE: '/Users/alex/.nvm/node', THREADBARE_NODE: '/Users/alex/.nvm/node', GONE: undefined },
    },
    { folders: ['/home/alex/sandboxed'], image: 'threadbear-sandbox', docker: 'docker' },
    {
      threadId: 'abcdef0123', home, uid: 1000, gid: 1000,
      exists: (p) => there.has(p),
      realpath: (p) => (p === '/home/alex/.local/bin/claude' ? '/home/alex/.local/share/claude/versions/2.1.318' : p),
    },
  );
  assert.equal(run.command, 'docker');
  assert.match(run.name, /^tb-abcdef01-/);
  const a = run.args;
  const vols = a.flatMap((x, i) => (x === '-v' ? [a[i + 1]] : []));
  assert.deepEqual(vols.sort(), [
    '/home/alex/.claude.json:/home/alex/.claude.json',
    '/home/alex/.claude:/home/alex/.claude',
    '/home/alex/.local/share/claude/versions/2.1.318:/home/alex/.local/share/claude/versions/2.1.318:ro',
    '/home/alex/sandboxed/demo:/home/alex/sandboxed/demo',
    '/tmp/settings-abc.json:/tmp/settings-abc.json:ro',
  ]);
  const envs = a.flatMap((x, i) => (x === '-e' ? [a[i + 1]] : []));
  assert.deepEqual(envs.sort(), ['ANTHROPIC_MODEL', 'HOME=/home/alex', 'THREADBEAR_THREAD_ID'], "the container's own PATH and node, and nothing unset");
  assert.deepEqual(a.slice(a.indexOf('-w'), a.indexOf('-w') + 2), ['-w', '/home/alex/sandboxed/demo']);
  // The image, then the CLI's real file and its own arguments, untouched.
  const at = a.indexOf('threadbear-sandbox');
  assert.deepEqual(a.slice(at), ['threadbear-sandbox', '/home/alex/.local/share/claude/versions/2.1.318', '--output-format', 'stream-json', '--settings', '/tmp/settings-abc.json', '--resume', 'abc']);
  assert.ok(a.includes('--rm') && a.includes('--init') && a.includes('-i'));
  assert.deepEqual(a.slice(a.indexOf('--user'), a.indexOf('--user') + 2), ['--user', '1000:1000']);
});

test("another login's config folder goes in instead of ~/.claude, without ~/.claude.json", () => {
  const cfg = '/home/alex/Library/Application Support/Threadbear/accounts/claude-1';
  const run = dockerRun(
    { command: '/c', args: [], cwd: '/home/alex/sandboxed/x', env: { CLAUDE_CONFIG_DIR: cfg } },
    { folders: ['/home/alex/sandboxed'], image: 'img', docker: 'docker' },
    { threadId: 't', home, uid: 1, gid: 1, exists: () => true, realpath: (p) => p },
  );
  const vols = run.args.flatMap((x, i) => (x === '-v' ? [run.args[i + 1]] : []));
  assert.ok(vols.includes(`${cfg}:${cfg}`));
  assert.ok(!vols.some((v) => v.startsWith('/home/alex/.claude')));
});

// Daybreak 124 (15 Nov 2026): the container got every variable of the
// runner's environment but a few of its own, so any cloud, GitHub, API or
// signing key the runner had was readable by the confined agent. This is
// the whole of what a sandboxed thread's CLI sees from the runner: the
// CLI's real environment as the adapter builds it, from a runner
// environment full of credentials.
test('a sandboxed agent sees only what Claude needs, never the runner\'s credentials, sockets or proxies', async () => {
  const { cliEnv, terminalMcpTools } = await import('../../src/adapters/claude/adapter.ts');
  const { threadEnv } = await import('../../src/restart.ts');
  const runnerEnv: Record<string, string> = {
    PATH: '/usr/bin', HOME: '/home/alex', USER: 'alex', SHELL: '/bin/zsh', TMPDIR: '/var/folders/x', LANG: 'en_AU.UTF-8', LC_ALL: 'en_AU.UTF-8', TERM: 'xterm-256color',
    AWS_ACCESS_KEY_ID: 'AKIA', AWS_SECRET_ACCESS_KEY: 's', AWS_SESSION_TOKEN: 't', AWS_PROFILE: 'prod',
    GITHUB_TOKEN: 'ghp_x', GH_TOKEN: 'ghp_y', GITLAB_TOKEN: 'g', NPM_TOKEN: 'n',
    OPENAI_API_KEY: 'sk-o', ANTHROPIC_API_KEY: 'sk-ant', ANTHROPIC_AUTH_TOKEN: 'a', CLAUDE_CODE_OAUTH_TOKEN: 'oat', ANTHROPIC_BASE_URL: 'https://evil.example', ANTHROPIC_CUSTOM_HEADERS: 'Authorization: x',
    CONVEX_DEPLOY_KEY: 'prod:x', SENTRY_AUTH_TOKEN: 'st', STRIPE_SECRET_KEY: 'sk_live', DATABASE_URL: 'postgres://u:p@h/db', APPLE_ID_PASSWORD: 'pw', MATCH_PASSWORD: 'pw', CODESIGN_IDENTITY: 'Developer ID',
    GOOGLE_APPLICATION_CREDENTIALS: '/home/alex/gcp.json', AZURE_CLIENT_SECRET: 'az', DOCKER_AUTH_CONFIG: '{}',
    SSH_AUTH_SOCK: '/tmp/ssh-agent.sock', GPG_AGENT_INFO: '/tmp/gpg', HTTP_PROXY: 'http://u:p@proxy:3128', HTTPS_PROXY: 'http://u:p@proxy:3128', ALL_PROXY: 'socks5://proxy', http_proxy: 'http://u:p@proxy', NO_PROXY: 'localhost',
    THREADBEAR_THREAD_KEY: 'should-never-go',
    // Grok's review: secrets the SDK reads under CLAUDE_CODE_, which a prefix let through.
    CLAUDE_CODE_REMOTE_MCP_CONFIG: '{"headers":{"Authorization":"Bearer x"}}', CLAUDE_CODE_HOST_CREDS_FILE: '/home/alex/creds.json', CLAUDE_CODE_EXTRA_BODY: '{"k":"s"}', CLAUDE_CODE_SHELL_PREFIX: 'with-token abc', CLAUDE_CODE_SESSION_ACCESS_TOKEN: 't', CLAUDE_AGENT_SDK_ANYTHING: 'x', CLAUDE_CODE_MESSAGING_TOKEN: 'parent', CLAUDE_CODE_TERMINAL_MCP_TOOLS: 'x',
  };
  const account = { id: 'claude-1', provider: 'claude', title: 'Claude', configDir: '/home/alex/tb/accounts/claude-1' } as const;
  const base = cliEnv(account as never, runnerEnv);
  // What the adapter's start() hands the SDK, the thread's env (a confined
  // thread gets no key), and what the SDK adds before it spawns.
  const env: Record<string, string | undefined> = {
    ...base,
    CLAUDE_CODE_TERMINAL_MCP_TOOLS: terminalMcpTools(base.CLAUDE_CODE_TERMINAL_MCP_TOOLS),
    ...threadEnv('t1', '/home/alex/tb', '/usr/bin', 'leaked-key'),
    CLAUDE_CODE_ADDITIONAL_DIRECTORIES_CLAUDE_MD: '1',
    CLAUDE_CODE_ENTRYPOINT: 'sdk-ts', CLAUDE_AGENT_SDK_VERSION: '0.3', CLAUDE_CODE_ENABLE_SDK_FILE_CHECKPOINTING: 'true', CLAUDE_CODE_SDK_READS_SESSION_STATE: '1', CLAUDE_CODE_SDK_HAS_OAUTH_REFRESH: '1',
  };
  const run = dockerRun({ command: '/c', args: [], cwd: '/home/alex/sandboxed/x', env }, { folders: ['/home/alex/sandboxed'], image: 'img', docker: 'docker' }, { threadId: 't1', home, uid: 1, gid: 1, exists: () => true, realpath: (p) => p });
  const envs = run.args.flatMap((x, i) => (x === '-e' ? [run.args[i + 1]] : []));
  assert.deepEqual(envs.sort(), [
    'CLAUDE_AGENT_SDK_CLIENT_APP',
    'CLAUDE_AGENT_SDK_VERSION',
    'CLAUDE_CODE_ADDITIONAL_DIRECTORIES_CLAUDE_MD',
    'CLAUDE_CODE_ENABLE_SDK_FILE_CHECKPOINTING',
    'CLAUDE_CODE_ENTRYPOINT',
    'CLAUDE_CODE_SDK_HAS_OAUTH_REFRESH',
    'CLAUDE_CODE_SDK_READS_SESSION_STATE',
    'CLAUDE_CODE_TERMINAL_MCP_TOOLS',
    'CLAUDE_CONFIG_DIR',
    'HOME=/home/alex',
    'LANG',
    'LC_ALL',
    'TERM',
    'THREADBARE_THREAD_ID',
    'THREADBEAR_RUNNER_VERSION',
    'THREADBEAR_THREAD_ID',
  ]);
  // Nothing goes in with its value in the arguments either (where ps
  // would show it), bar the container's own HOME.
  assert.ok(!run.args.some((a) => /=/.test(a) && a !== 'HOME=/home/alex' && !a.includes(':')), 'no NAME=value');
});

test('names that look like credentials stay out even under an allowed prefix', async () => {
  const { sandboxEnvAllowed } = await import('../../src/sandbox.ts');
  for (const n of ['CLAUDE_CODE_OAUTH_TOKEN', 'CLAUDE_CODE_API_KEY_HELPER', 'CLAUDE_CODE_CLIENT_CERT', 'CLAUDE_CODE_PROXY_URL', 'CLAUDE_CODE_SECRET', 'LC_TOKEN', 'CLAUDE_CODE_REMOTE_MCP_CONFIG', 'CLAUDE_CODE_HOST_CREDS_FILE', 'CLAUDE_CODE_EXTRA_BODY', 'CLAUDE_CODE_SHELL_PREFIX', 'CLAUDE_CODE_NEW_THING', 'ANTHROPIC_API_KEY', 'ANTHROPIC_BASE_URL', 'ANTHROPIC_CUSTOM_HEADERS', 'THREADBEAR_THREAD_KEY', 'THREADBEAR_HOME', 'SSH_AUTH_SOCK', 'https_proxy', 'claude_code_x'])
    assert.equal(sandboxEnvAllowed(n), false, n);
  for (const n of ['CLAUDE_CONFIG_DIR', 'CLAUDE_CODE_ENTRYPOINT', 'ANTHROPIC_MODEL', 'ANTHROPIC_DEFAULT_OPUS_MODEL', 'LC_CTYPE', 'THREADBEAR_THREAD_ID'])
    assert.equal(sandboxEnvAllowed(n), true, n);
});
