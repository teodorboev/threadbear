import { test } from 'node:test';
import assert from 'node:assert/strict';
import { startedByFlags } from '../../src/started-by-flags.ts';

test('tb new: a tool names itself, an agent is an agent, and no flags could be Alex', () => {
  assert.equal(startedByFlags({}), undefined);
  assert.deepEqual(startedByFlags({ from: 'Sentry', why: '1 event', icon: 'ladybug' }), { kind: 'tool', label: 'Sentry', icon: 'ladybug', why: '1 event' });
  assert.deepEqual(startedByFlags({}, 'abc'), { kind: 'thread', label: 'Agent', threadId: 'abc' });
  assert.deepEqual(startedByFlags({ why: 'fixing a hook' }, 'abc'), { kind: 'thread', label: 'Agent', threadId: 'abc', why: 'fixing a hook' });
  assert.deepEqual(startedByFlags({ why: 'tidying' }), { kind: 'tool', label: 'A script', why: 'tidying' });
});
