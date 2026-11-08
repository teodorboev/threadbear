// OpenCode's ACP updates (shaped like `opencode acp` 2.1's, written by hand)
// through OpencodeNormalizer, and the adapter's pure helpers.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { OpencodeNormalizer, codeModeCall, toolInput, toolName } from '../../src/adapters/opencode/normalize.ts';
import { opencodeModelChoices, permissionRules, promptBlocks } from '../../src/adapters/opencode/adapter.ts';
import { fileContext } from '../../src/adapters/context.ts';
import type { AdapterOutput } from '../../src/adapters/types.ts';
import { tmpDir } from '../helpers/fakes.ts';

function norm() {
  const root = tmpDir('tb-opencode-norm-');
  const cwd = path.join(root, 'proj');
  fs.mkdirSync(cwd, { recursive: true });
  return { cwd, n: new OpencodeNormalizer(fileContext(cwd, path.join(root, 'media'), path.join(root, 'blobs'))) };
}

const events = (outs: AdapterOutput[]) => outs.filter((o) => o.kind === 'event').map((o) => (o as { event: Record<string, unknown> }).event);

test('text chunks stream as deltas and close as one event when a tool starts', () => {
  const { n } = norm();
  const out = [
    ...n.update({ sessionUpdate: 'agent_thought_chunk', messageId: 'm1:reasoning:0', content: { type: 'text', text: 'Thinking ' } }),
    ...n.update({ sessionUpdate: 'agent_thought_chunk', messageId: 'm1:reasoning:0', content: { type: 'text', text: 'hard.' } }),
    ...n.update({ sessionUpdate: 'agent_message_chunk', messageId: 'm2', content: { type: 'text', text: 'Hello' } }),
    ...n.update({ sessionUpdate: 'agent_message_chunk', messageId: 'm2', content: { type: 'text', text: ' there' } }),
    ...n.update({ sessionUpdate: 'tool_call', toolCallId: 'c1', name: 'shell', title: 'shell', kind: 'execute', status: 'pending', rawInput: {} }),
  ];
  const deltas = out.filter((o) => o.kind === 'delta').map((o) => (o as { delta: { type: string; text: string } }).delta);
  assert.deepEqual(deltas.map((d) => d.type), ['reasoning.delta', 'reasoning.delta', 'assistant.text.delta', 'assistant.text.delta']);
  assert.deepEqual(events(out), [
    { type: 'reasoning', itemId: 'm1:reasoning:0', text: 'Thinking hard.' },
    { type: 'assistant.text', itemId: 'm2', text: 'Hello there' },
  ]);
  assert.equal(n.lastText?.text, 'Hello there');
});

test('a shell call goes out as Bash once it runs, with its output as the result', () => {
  const { n, cwd } = norm();
  const out = [
    ...n.update({ sessionUpdate: 'tool_call', toolCallId: 'c1', name: 'shell', title: 'shell', kind: 'execute', status: 'pending', locations: [{ path: cwd }], rawInput: { cwd } }),
    ...n.update({ sessionUpdate: 'tool_call_update', toolCallId: 'c1', status: 'in_progress', kind: 'execute', title: 'echo hi', rawInput: { command: 'echo hi', cwd } }),
    ...n.update({ sessionUpdate: 'tool_call_update', toolCallId: 'c1', status: 'in_progress', kind: 'execute', title: 'echo hi', rawInput: { command: 'echo hi', cwd } }),
    ...n.update({ sessionUpdate: 'tool_call_update', toolCallId: 'c1', status: 'completed', content: [{ type: 'content', content: { type: 'text', text: 'hi' } }], rawOutput: { metadata: { exit: 0 } } }),
  ];
  const evs = events(out);
  assert.equal(evs.length, 2);
  assert.deepEqual({ ...evs[0], input: undefined }, { type: 'tool.call', itemId: 'c1', name: 'Bash', kind: 'command', title: 'echo hi', input: undefined });
  assert.equal((evs[0].input as Record<string, unknown>).command, 'echo hi');
  assert.deepEqual(evs[1], { type: 'tool.result', itemId: 'c1', ok: true, text: 'hi', truncated: false, images: [] });
});

test('a failing command is a failed result', () => {
  const { n } = norm();
  n.update({ sessionUpdate: 'tool_call_update', toolCallId: 'c1', status: 'in_progress', kind: 'execute', name: 'shell', rawInput: { command: 'false' } });
  const evs = events(n.update({ sessionUpdate: 'tool_call_update', toolCallId: 'c1', status: 'completed', content: [{ type: 'content', content: { type: 'text', text: '(no output)' } }], rawOutput: { metadata: { exit: 1 } } }));
  assert.equal(evs[0].ok, false);
  assert.equal(evs[0].text, '');
});

test('an edit gives Edit with renamed arguments, a diff and a result', () => {
  const { n, cwd } = norm();
  const file = path.join(cwd, 'f.txt');
  n.update({ sessionUpdate: 'tool_call', toolCallId: 'e1', name: 'edit', title: 'edit', kind: 'edit', status: 'pending', rawInput: {} });
  const started = events(n.update({ sessionUpdate: 'tool_call_update', toolCallId: 'e1', status: 'in_progress', kind: 'edit', title: 'edit', rawInput: { path: file, oldString: 'line one', newString: 'line two' } }));
  assert.equal(started[0].name, 'Edit');
  assert.equal(started[0].title, 'Edit f.txt');
  assert.deepEqual(started[0].input, { file_path: file, old_string: 'line one', new_string: 'line two' });
  const done = events(
    n.update({
      sessionUpdate: 'tool_call_update',
      toolCallId: 'e1',
      status: 'completed',
      content: [
        { type: 'content', content: { type: 'text', text: 'Edited f.txt (1 replacement)' } },
        { type: 'diff', path: file, oldText: 'line one\n', newText: 'line two\n' },
      ],
    }),
  );
  assert.equal(done[0].type, 'edit.diff');
  assert.deepEqual(done[0].hunks, [{ oldStart: 1, oldLines: 1, newStart: 1, newLines: 1, lines: ['-line one', '+line two'] }]);
  assert.deepEqual(done[1], { type: 'tool.result', itemId: 'e1', ok: true, text: 'Edited f.txt (1 replacement)', truncated: false, images: [] });
});

test('threadbear tools go out under their MCP name', () => {
  assert.deepEqual(toolName('threadbear_end_turn'), { name: 'mcp__threadbear__end_turn', kind: 'mcp' });
  assert.deepEqual(toolName('context7_query-docs', 'other'), { name: 'context7_query-docs', kind: 'mcp' });
  assert.deepEqual(toolName('grep', 'search'), { name: 'Grep', kind: 'search' });
  assert.deepEqual(toolName('mystery', 'fetch'), { name: 'mystery', kind: 'web' });
  assert.deepEqual(toolInput({ filePath: 'a', other: 1 }), { file_path: 'a', other: 1 });
});

test('an approval sends the call row first and says what to show', () => {
  const { n, cwd } = norm();
  const a = n.forApproval({ toolCallId: 'c9', name: 'shell', title: 'rm -rf build', kind: 'execute', status: 'pending', locations: [{ path: cwd }], rawInput: { command: 'rm -rf build', cwd } });
  assert.equal(a.itemId, 'c9');
  assert.equal(a.name, 'Bash');
  assert.equal(a.title, 'rm -rf build');
  assert.equal(events(a.out)[0].type, 'tool.call');
  // The update that follows the approval doesn't send the row again.
  assert.deepEqual(events(n.update({ sessionUpdate: 'tool_call_update', toolCallId: 'c9', status: 'in_progress', rawInput: { command: 'rm -rf build' } })), []);
});

test('the end of a turn closes text and fails calls left running', () => {
  const { n } = norm();
  n.update({ sessionUpdate: 'tool_call_update', toolCallId: 'c1', status: 'in_progress', name: 'shell', kind: 'execute', rawInput: { command: 'sleep 100' } });
  n.update({ sessionUpdate: 'agent_message_chunk', messageId: 'm3', content: { type: 'text', text: 'Stopping' } });
  const evs = events(n.finishTurn());
  assert.deepEqual(evs.map((e) => e.type), ['assistant.text', 'tool.result']);
  assert.equal(evs[1].ok, false);
});

test('usage and plan updates', () => {
  const { n } = norm();
  assert.deepEqual(n.update({ sessionUpdate: 'usage_update', used: 19497, size: 200000, cost: { amount: 0, currency: 'USD' } }), [{ kind: 'context', contextTokens: 19497, contextWindow: 200000 }]);
  assert.deepEqual(events(n.update({ sessionUpdate: 'plan', entries: [{ content: 'Write it', status: 'in_progress', priority: 'high' }, { content: '', status: 'pending' }] })), [
    { type: 'plan.updated', entries: [{ text: 'Write it', status: 'in_progress' }] },
  ]);
  assert.deepEqual(n.update({ sessionUpdate: 'user_message_chunk', content: { type: 'text', text: 'replayed' } }), []);
});

test('permission rules: Full allows all, the rest ask for anything that changes things', () => {
  assert.deepEqual(permissionRules('full'), [{ action: '*', resource: '*', effect: 'allow' }]);
  for (const mode of ['ask', 'auto', 'plan'] as const) {
    const rules = permissionRules(mode);
    assert.deepEqual(rules[0], { action: '*', resource: '*', effect: 'ask' });
    const effect = (action: string) => rules.findLast((r) => r.action === action || r.action === '*' || (r.action.endsWith('*') && action.startsWith(r.action.slice(0, -1))))?.effect;
    assert.equal(effect('shell'), 'ask');
    assert.equal(effect('write'), 'ask');
    assert.equal(effect('read'), 'allow');
    assert.equal(effect('threadbear_end_turn'), 'allow');
  }
});

test('the model list keeps enabled models, as provider/model ids', () => {
  assert.deepEqual(
    opencodeModelChoices([
      { id: 'iceman-free', modelID: 'iceman-free', providerID: 'opencode', name: 'Iceman Free', enabled: true, status: 'active' },
      { modelID: 'glm-5.3', providerID: 'ollama-cloud', name: 'GLM-5.3' },
      { modelID: 'old', providerID: 'x', status: 'deprecated' },
      { modelID: 'off', providerID: 'x', enabled: false },
      { name: 'no ids' },
    ]),
    [
      { id: 'opencode/iceman-free', model: 'opencode/iceman-free', title: 'Iceman Free', description: 'opencode' },
      { id: 'ollama-cloud/glm-5.3', model: 'ollama-cloud/glm-5.3', title: 'GLM-5.3', description: 'ollama-cloud' },
    ],
  );
  assert.deepEqual(opencodeModelChoices(undefined), []);
});

test('a message becomes text and image blocks', () => {
  const dir = tmpDir('tb-opencode-img-');
  const img = path.join(dir, 'a.png');
  fs.writeFileSync(img, Buffer.from([1, 2, 3]));
  assert.deepEqual(promptBlocks({ text: 'look', images: [{ path: img, mediaType: 'image/png' }] }), [
    { type: 'text', text: 'look' },
    { type: 'image', mimeType: 'image/png', data: 'AQID' },
  ]);
});

test("a code-mode script that only calls one threadbear tool shows as that tool", () => {
  assert.deepEqual(codeModeCall('execute', { code: 'return await tools.threadbear.end_turn({"summary":"done","needs":"nothing"});' }), {
    name: 'mcp__threadbear__end_turn',
    input: { summary: 'done', needs: 'nothing' },
  });
  const js = 'const r = await tools.threadbear.ping({ word: "hello" });\nreturn r;';
  assert.deepEqual(codeModeCall('execute', { code: js }), { name: 'mcp__threadbear__ping', input: { code: js } });
  assert.equal(codeModeCall('execute', { code: 'await tools.threadbear.ping({}); await tools.threadbear.pin({})' }), undefined);
  assert.equal(codeModeCall('execute', { code: 'return tools.browser.tabs.list()' }), undefined);
  assert.equal(codeModeCall('shell', { code: 'tools.threadbear.ping({})' }), undefined);
  const { n } = norm();
  n.update({ sessionUpdate: 'tool_call', toolCallId: 'x1', name: 'execute', title: 'execute', kind: 'other', status: 'pending', rawInput: {} });
  const evs = events(n.update({ sessionUpdate: 'tool_call_update', toolCallId: 'x1', status: 'in_progress', kind: 'other', rawInput: { code: 'return await tools.threadbear.end_turn({"summary":"done"})' } }));
  assert.equal(evs[0].name, 'mcp__threadbear__end_turn');
  assert.equal(evs[0].kind, 'mcp');
});
