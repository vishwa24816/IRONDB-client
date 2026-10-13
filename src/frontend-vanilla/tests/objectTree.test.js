import test from 'node:test';
import assert from 'node:assert/strict';
import {
  levelVisible, expandNode, contextMenuFor, feedCompletionCache,
  getCompletions, clearCompletions, clearPending, CAP_SCHEMAS,
} from '../src/panels/browser/objectTree.js';

test('schemas hidden without SCHEMAS cap, visible with it', () => {
  assert.equal(levelVisible('schemas', 0), false);
  assert.equal(levelVisible('schemas', CAP_SCHEMAS), true);
  assert.equal(levelVisible('tables', 0), true);
});

test('expand gated: no invoke for missing caps', async () => {
  clearPending();
  let calls = 0;
  const res = await expandNode({
    connectionId: 'c1', level: 'schemas', caps: 0,
    invokeFn: async () => { calls += 1; return { success: true, data: { children: [] } }; },
  });
  assert.deepEqual(res, []);
  assert.equal(calls, 0);
});

test('expand single-flight shares one invoke', async () => {
  clearPending();
  let calls = 0;
  const invokeFn = async () => {
    calls += 1;
    await new Promise((r) => setTimeout(r, 5));
    return { success: true, data: { children: [{ kind: 'table', name: 'users' }] } };
  };
  const args = { connectionId: 'c1', level: 'tables', parent: 'main', caps: 0, invokeFn };
  const [a, b] = await Promise.all([expandNode(args), expandNode(args)]);
  assert.equal(calls, 1);
  assert.deepEqual(a, b);
});

test('context menu + completion cache', () => {
  clearCompletions();
  assert.deepEqual(contextMenuFor({ kind: 'column', name: 'id' }).map((i) => i.id), ['copy-name']);
  assert.ok(contextMenuFor({ kind: 'table', name: 'users' }).some((i) => i.id === 'view-data'));
  feedCompletionCache([{ kind: 'table', name: 'users' }, { kind: 'column', name: 'id' }]);
  assert.deepEqual(getCompletions(), [{ name: 'users', kind: 'table' }, { name: 'id', kind: 'column' }]);
});
