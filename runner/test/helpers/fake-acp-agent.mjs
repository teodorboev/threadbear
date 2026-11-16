#!/usr/bin/env node
// A pretend ACP agent for tests, which follows what OpenCode 1.18.23 was seen
// to do on 9 Nov 2026: by default it runs tools without asking; config in
// OPENCODE_CONFIG_CONTENT makes it ask, a folder's opencode.json outranks that
// unless OPENCODE_DISABLE_PROJECT_CONFIG is set, and a folder's plugin runs
// when a session opens. A prompt's words say what it does:
//   "write <path> <text>"  a write through its tool (asks or not, as above)
//   "exec <command>"       the same for a shell command
//   "hello"                just answers
// FAKE_ACP_LOG: where each request is appended, as JSON lines.
//
// FAKE_ACP_NAME=Grok follows Grok 1.0.77 (15 Nov 2026) instead: it runs every
// tool unasked (as its auto mode did) unless a PreToolUse hook from the plugin
// named by --plugin-dir says ask or deny, and runs that hook as Grok would,
// so the real guard script is what decides. More words for it:
//   "read <path>"          its read_file tool
//   "mcp <server__tool>"   an MCP tool, through its use_tool
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import readline from 'node:readline';

const env = process.env;
const logFile = env.FAKE_ACP_LOG;
const log = (m) => {
  if (logFile) fs.appendFileSync(logFile, `${JSON.stringify(m)}\n`);
};
const send = (m) => process.stdout.write(`${JSON.stringify({ jsonrpc: '2.0', ...m })}\n`);
const AGENT = env.FAKE_ACP_NAME ?? 'OpenCode';
let nextOwn = 0; // from 0, as OpenCode's are
const waiting = new Map();
const ask = (method, params) =>
  new Promise((resolve) => {
    const id = nextOwn++;
    waiting.set(id, resolve);
    send({ id, method, params });
  });

const GROK = AGENT === 'Grok';
const sessions = new Map();
let mode = 'build';
let model = GROK ? 'grok-4.7' : 'fake/one';
let effort = 'high';
const configOptions = () =>
  GROK
    ? [
        { id: 'model', name: 'Model', category: 'model', type: 'select', currentValue: model, options: [{ value: 'grok-4.7', name: 'Grok 4.7' }, { value: 'grok-4.5', name: 'Grok 4.5' }] },
        { id: 'reasoning_effort', name: 'Reasoning Effort', category: 'thought_level', type: 'select', currentValue: effort, options: [{ value: 'xhigh' }, { value: 'high' }, { value: 'medium' }, { value: 'low' }] },
      ]
    : [
        { id: 'model', name: 'Model', category: 'model', type: 'select', currentValue: model, options: [{ value: 'fake/one', name: 'Fake One' }, { value: 'fake/two', name: 'Fake Two' }] },
        { id: 'mode', name: 'Mode', category: 'mode', type: 'select', currentValue: mode, options: [{ value: 'build', name: 'build' }, { value: 'plan', name: 'plan' }, { value: 'ask', name: 'ask' }, { value: 'agent', name: 'agent' }] },
      ];
const levels = (ids) => ids.map((id) => ({ id, value: id, label: id, default: id === 'high' }));
const grokModels = () => ({
  currentModelId: model,
  availableModels: [
    { modelId: 'grok-4.7', name: 'Grok 4.7', _meta: { supportsReasoningEffort: true, reasoningEfforts: levels(['xhigh', 'high', 'medium', 'low']) } },
    { modelId: 'grok-4.5', name: 'Grok 4.5', _meta: { supportsReasoningEffort: true, reasoningEfforts: levels(['high', 'medium', 'low', 'minimal']) } },
  ],
});

// The plugin from --plugin-dir and its hooks, unless the folder's
// .grok/config.toml turns it off by name, as Grok lets any folder do.
function grokPlugin(cwd) {
  const at = process.argv.indexOf('--plugin-dir');
  if (at < 0) return undefined;
  const plugin = process.argv[at + 1];
  const name = path.basename(plugin);
  try {
    if (fs.readFileSync(path.join(cwd, '.grok', 'config.toml'), 'utf8').includes(`"${name}"`)) return undefined;
  } catch {
    // no folder config
  }
  try {
    return { name, hooks: JSON.parse(fs.readFileSync(path.join(plugin, 'hooks', 'hooks.json'), 'utf8')).hooks ?? {} };
  } catch {
    return undefined;
  }
}

// Grok reports the hooks it ran for an event, by name.
function reportHooks(sessionId, event, plugin) {
  const runs = [{ name: 'global/settings:x[0].hooks[0]', status: { status: 'success' } }];
  const key = event === 'user_prompt_submit' ? 'UserPromptSubmit' : 'PreToolUse';
  if (plugin?.hooks[key]?.length) runs.push({ name: `plugin/${plugin.name}/hooks:${event}[0].hooks[0]`, status: { status: 'success' } });
  send({ method: '_x.ai/session_notification', params: { sessionId, update: { sessionUpdate: 'hook_execution', event_name: event, runs } } });
}

// Grok's PreToolUse hooks from --plugin-dir: the first deny wins, then ask.
// A hook that can't run, with failClosed, denies.
function grokHook(sessionId, cwd, toolName, toolInput) {
  const plugin = grokPlugin(cwd);
  reportHooks(sessionId, 'pre_tool_use', plugin);
  if (!plugin) return { decision: 'none' };
  const conf = { hooks: plugin.hooks };
  let out = { decision: 'none' };
  for (const group of conf.hooks?.PreToolUse ?? []) {
    for (const h of group.hooks ?? []) {
      const r = spawnSync(h.command, h.args ?? [], { input: JSON.stringify({ hookEventName: 'pre_tool_use', cwd, toolName, toolInput, toolInputTruncated: false, tool_name: toolName, tool_input: toolInput }), encoding: 'utf8' });
      let reply;
      try {
        reply = JSON.parse(r.stdout);
      } catch {
        reply = undefined;
      }
      if (r.status !== 0 || !reply) {
        if (h.failClosed) return { decision: 'deny', reason: 'hook failed' };
        continue;
      }
      if (reply.decision === 'deny') return { decision: 'deny', reason: reply.reason };
      if (reply.decision === 'ask') out = { decision: 'ask', reason: reply.reason, hookName: `plugin/${plugin.name}/hooks:pre_tool_use[0].hooks[0]` };
    }
  }
  return out;
}

async function grokTool(sessionId, cwd, kind, title, toolName, input, run) {
  const toolCallId = `call-${Math.random().toString(36).slice(2)}`;
  update(sessionId, { sessionUpdate: 'tool_call', toolCallId, title: toolName, rawInput: input });
  const hook = grokHook(sessionId, cwd, toolName, input);
  log({ hook: { toolName, decision: hook.decision } });
  if (hook.decision === 'deny') {
    update(sessionId, { sessionUpdate: 'tool_call_update', toolCallId, status: 'failed', content: [{ type: 'content', content: { type: 'text', text: `Hook denied: ${hook.reason}` } }] });
    return;
  }
  if (hook.decision === 'ask') {
    const r = await ask('session/request_permission', {
      sessionId,
      toolCall: { toolCallId, kind, title: `${title} — hook '${hook.hookName}' asks: ${hook.reason}`, rawInput: { variant: kind === 'edit' ? 'Write' : kind === 'execute' ? 'Bash' : toolName === 'use_tool' ? 'UseTool' : 'Read', ...input }, ...(input.file_path ? { locations: [{ path: input.file_path }] } : {}) },
      options: [
        { optionId: 'allow-edits-session', kind: 'allow_always', name: 'Yes, allow all edits during this session' },
        { optionId: 'allow-once', kind: 'allow_once', name: 'Yes, proceed' },
        { optionId: 'reject-once', kind: 'reject_once', name: 'No' },
      ],
      _meta: { hookAsk: { hookName: hook.hookName, reason: hook.reason } },
    });
    log({ answered: r });
    if (r.outcome?.outcome !== 'selected' || r.outcome.optionId !== 'allow-once') {
      update(sessionId, { sessionUpdate: 'tool_call_update', toolCallId, status: 'failed', content: [{ type: 'content', content: { type: 'text', text: 'User rejected' } }] });
      return 'rejected';
    }
  }
  run();
  update(sessionId, { sessionUpdate: 'tool_call_update', toolCallId, status: 'completed', content: [{ type: 'content', content: { type: 'text', text: 'done' } }] });
}

async function grokPrompt(id, p, s, word, rest) {
  reportHooks(p.sessionId, 'user_prompt_submit', grokPlugin(s.cwd));
  // A model starts with a thought, and a stopped process stops here.
  update(p.sessionId, { sessionUpdate: 'agent_thought_chunk', content: { type: 'text', text: 'hmm' } });
  await new Promise((r) => setTimeout(r, 50));
  let rejected;
  if (word === 'write') rejected = await grokTool(p.sessionId, s.cwd, 'edit', `Write \`${rest[0]}\``, 'write', { file_path: rest[0], content: rest.slice(1).join(' ') }, () => fs.writeFileSync(rest[0], rest.slice(1).join(' ')));
  else if (word === 'exec') rejected = await grokTool(p.sessionId, s.cwd, 'execute', `Execute \`${rest.join(' ')}\``, 'run_terminal_command', { command: rest.join(' ') }, () => log({ ran: rest.join(' ') }));
  else if (word === 'read') rejected = await grokTool(p.sessionId, s.cwd, 'read', `Read \`${rest[0]}\``, 'read_file', { target_file: rest[0] }, () => log({ read: rest[0] }));
  else if (word === 'mcp') rejected = await grokTool(p.sessionId, s.cwd, 'other', rest[0], 'use_tool', { tool_name: rest[0], tool_input: {} }, () => log({ mcp: rest[0] }));
  update(p.sessionId, { sessionUpdate: 'agent_message_chunk', messageId: 'm1', content: { type: 'text', text: 'done' } });
  // A rejected call ends Grok's turn, as the real one does.
  send({ id, result: { stopReason: rejected ? 'cancelled' : 'end_turn' } });
}

function asksFor(cwd) {
  if (env.OPENCODE_DISABLE_PROJECT_CONFIG !== '1') {
    try {
      if (JSON.parse(fs.readFileSync(path.join(cwd, 'opencode.json'), 'utf8')).permission) return false; // the folder's config wins
    } catch {
      // no such file
    }
  }
  try {
    const conf = JSON.parse(env.OPENCODE_CONFIG_CONTENT ?? '{}');
    return conf.permission?.['*'] === 'ask' || conf.permission?.edit === 'ask';
  } catch {
    return false;
  }
}

function deniesAll() {
  try {
    return JSON.parse(env.OPENCODE_CONFIG_CONTENT ?? '{}').permission?.['*'] === 'deny';
  } catch {
    return false;
  }
}

function openedSession(cwd) {
  if (env.OPENCODE_DISABLE_PROJECT_CONFIG !== '1' && !process.argv.includes('--pure')) {
    const plugin = path.join(cwd, '.opencode', 'plugins', 'run.txt');
    if (fs.existsSync(plugin)) fs.writeFileSync(fs.readFileSync(plugin, 'utf8').trim(), 'plugin ran');
  }
}

const update = (sessionId, u) => send({ method: 'session/update', params: { sessionId, update: u } });

async function runTool(sessionId, cwd, kind, input, locations, run) {
  const toolCallId = `call-${Math.random().toString(36).slice(2)}`;
  update(sessionId, { sessionUpdate: 'tool_call', toolCallId, title: kind === 'edit' ? 'write' : 'bash', kind, status: 'pending', locations: [], rawInput: {} });
  update(sessionId, { sessionUpdate: 'tool_call_update', toolCallId, status: 'in_progress', kind, locations, rawInput: input });
  if (deniesAll()) {
    update(sessionId, { sessionUpdate: 'tool_call_update', toolCallId, status: 'failed', kind, rawOutput: { error: 'denied by config' } });
    return;
  }
  if (asksFor(cwd)) {
    const r = await ask('session/request_permission', {
      sessionId,
      toolCall: { toolCallId, title: locations[0]?.path ?? input.command, kind, status: 'pending', locations, rawInput: input, ...(kind === 'edit' ? { content: [{ type: 'diff', path: locations[0].path, oldText: '', newText: input.content }] } : {}) },
      options: env.FAKE_ACP_NO_ONCE
        ? [{ optionId: 'always', kind: 'allow_always', name: 'Always allow' }]
        : [
            { optionId: 'once', kind: 'allow_once', name: 'Allow once' },
            { optionId: 'always', kind: 'allow_always', name: 'Always allow' },
            { optionId: 'reject', kind: 'reject_once', name: 'Reject' },
          ],
    });
    log({ answered: r });
    if (r.outcome?.outcome !== 'selected' || r.outcome.optionId === 'reject') {
      update(sessionId, { sessionUpdate: 'tool_call_update', toolCallId, status: 'failed', kind, content: [{ type: 'content', content: { type: 'text', text: 'rejected' } }], rawOutput: { error: 'rejected' } });
      return;
    }
  }
  run();
  update(sessionId, { sessionUpdate: 'tool_call_update', toolCallId, status: 'completed', kind, content: [{ type: 'content', content: { type: 'text', text: 'done' } }] });
}

async function prompt(id, p) {
  const s = sessions.get(p.sessionId);
  const text = p.prompt.filter((b) => b.type === 'text').map((b) => b.text).join('\n');
  log({ prompt: text, blocks: p.prompt.map((b) => b.type) });
  const last = text.split('\n').at(-1).trim();
  const [word, ...rest] = last.split(' ');
  if (word === 'hang') return; // answers only session/cancel
  if (GROK) return grokPrompt(id, p, s, word, rest);
  if (word === 'write') await runTool(p.sessionId, s.cwd, 'edit', { filePath: rest[0], content: rest.slice(1).join(' ') }, [{ path: rest[0] }], () => fs.writeFileSync(rest[0], rest.slice(1).join(' ')));
  else if (word === 'exec') await runTool(p.sessionId, s.cwd, 'execute', { command: rest.join(' ') }, [], () => log({ ran: rest.join(' ') }));
  update(p.sessionId, { sessionUpdate: 'agent_message_chunk', messageId: 'm1', content: { type: 'text', text: 'all ' } });
  update(p.sessionId, { sessionUpdate: 'agent_message_chunk', messageId: 'm1', content: { type: 'text', text: 'done' } });
  update(p.sessionId, { sessionUpdate: 'usage_update', used: 1000, size: 200000, cost: { amount: 0.25, currency: 'USD' } });
  send({ id, result: { stopReason: 'end_turn' } });
}

const handlers = {
  initialize: () => ({
    ...(env.FAKE_ACP_VERSION === 'none' ? {} : { protocolVersion: env.FAKE_ACP_VERSION ? Number(env.FAKE_ACP_VERSION) : 1 }),
    agentCapabilities: { loadSession: true, mcpCapabilities: { http: true }, promptCapabilities: { image: true }, sessionCapabilities: { resume: {}, close: {} } },
    ...(GROK
      ? { authMethods: [{ id: 'cached_token', name: 'cached_token' }, { id: 'grok.com', name: 'Grok' }], _meta: { grokShell: true, agentVersion: '1.0.77' } }
      : { authMethods: AGENT === 'Cursor' ? [{ id: 'cursor_login', name: 'Log in' }] : [{ id: 'opencode-login', name: 'Log in' }], agentInfo: { name: AGENT, version: '9.9.9' } }),
  }),
  'session/new': (p) => {
    if (env.FAKE_ACP_NEEDS_LOGIN) throw Object.assign(new Error('Authentication required'), { code: -32000 });
    const sessionId = `ses-${Math.random().toString(36).slice(2)}`;
    sessions.set(sessionId, { cwd: p.cwd });
    openedSession(p.cwd);
    log({ newSession: { cwd: p.cwd, mcpServers: p.mcpServers } });
    return { sessionId, configOptions: configOptions(), ...(GROK ? { models: grokModels() } : {}) };
  },
  'session/resume': (p) => {
    if (p.sessionId === 'gone') throw Object.assign(new Error('session not found'), { code: -32002 });
    sessions.set(p.sessionId, { cwd: p.cwd });
    openedSession(p.cwd);
    log({ resumed: p.sessionId });
    return { configOptions: configOptions() };
  },
  'session/close': () => ({}),
  'session/set_config_option': (p) => {
    if (env.FAKE_ACP_NO_SET_MODE && p.configId === 'mode') throw new Error('no such mode');
    if (p.configId === 'mode' && !env.FAKE_ACP_IGNORE_MODE) mode = p.value;
    if (p.configId === 'model') model = p.value;
    if (p.configId === 'reasoning_effort') effort = p.value;
    log({ set: { [p.configId]: p.value } });
    return { configOptions: configOptions() };
  },
};

readline.createInterface({ input: process.stdin }).on('line', (line) => {
  if (!line.trim()) return;
  const m = JSON.parse(line);
  if (m.id !== undefined && !m.method) {
    waiting.get(m.id)?.(m.result ?? m.error);
    waiting.delete(m.id);
    return;
  }
  if (m.method === 'session/cancel') {
    log({ cancelled: true });
    return;
  }
  if (m.method === 'session/prompt') {
    void prompt(m.id, m.params);
    if (m.params.prompt.some((b) => b.type === 'text' && b.text.trim().split('\n').at(-1) === 'hang')) hanging.push(m.id);
    return;
  }
  const h = handlers[m.method];
  if (m.id === undefined) return;
  if (!h) return send({ id: m.id, error: { code: -32601, message: `no ${m.method}` } });
  try {
    const result = h(m.params);
    if (m.method === 'session/set_config_option' && env.FAKE_ACP_SLOW_SET) return void setTimeout(() => send({ id: m.id, result }), Number(env.FAKE_ACP_SLOW_SET));
    send({ id: m.id, result });
  } catch (e) {
    send({ id: m.id, error: { code: e.code ?? -32603, message: e.message } });
  }
});
const hanging = [];
process.on('SIGTERM', () => process.exit(0));
