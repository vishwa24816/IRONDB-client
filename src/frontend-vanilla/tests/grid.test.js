// Pure-logic tests: windowing, pagination, diff, stale guard (no DOM, no Monaco).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { visibleWindow, autoWidth, copySelection } from '../src/panels/grid/gridCore.js';
import { paginate } from '../src/panels/grid/gridPagination.js';
import { diffRows } from '../src/panels/grid/diffView.js';
import { shouldRender } from '../src/panels/editor/editorToolbar.js';

test('visibleWindow covers viewport plus overscan', () => {
  const { start, end } = visibleWindow({ scrollTop: 280, viewportHeight: 280, rowCount: 100000 });
  assert.equal(start, 0);
  assert.equal(end, 30); // 20 visible + 10 overscan
  assert.ok(end - start <= 60, 'window stays small for 100k rows');
});

test('visibleWindow clamps at tail', () => {
  const { start, end } = visibleWindow({ scrollTop: 1e9, viewportHeight: 280, rowCount: 50 });
  assert.equal(end, 50);
  assert.ok(start <= end);
});

test('paginate slices pages', () => {
  const rows = Array.from({ length: 250 }, (_, i) => ({ a: i }));
  assert.equal(paginate(rows, { page: 1, pageSize: 100 }).rows.length, 100);
  assert.equal(paginate(rows, { page: 3, pageSize: 100 }).rows.length, 50);
  assert.equal(paginate(rows, { page: 99, pageSize: 100 }).page, 3);
});

test('diffRows reports only changed cells', () => {
  const before = [{ id: 1, v: 'a' }, { id: 2, v: 'b' }];
  const after = [{ id: 1, v: 'a' }, { id: 2, v: 'CHANGED' }];
  const { changed } = diffRows(before, after, { key: 'id' });
  assert.equal(changed.length, 1);
  assert.deepEqual(changed[0].cells, [{ column: 'v', before: 'b', after: 'CHANGED' }]);
});

test('shouldRender drops stale results', () => {
  assert.equal(shouldRender('q-1', 'q-2'), false);
  assert.equal(shouldRender('q-2', 'q-2'), true);
});

test('autoWidth and copySelection helpers', () => {
  const rows = [{ a: 'hi', b: '<script>x</script>' }];
  const w = autoWidth(['a', 'b'], rows);
  assert.ok(w.b > w.a);
  assert.equal(copySelection(rows, [[0, 'b']]), '<script>x</script>');
});
