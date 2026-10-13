// Grid shell: toolbar + view switcher (grid/diff) + stale guard via activeQueryId.
import { getActiveQueryId } from '../editor/editorToolbar.js';
import { renderWindow } from './gridCore.js';
import { mountPagination, paginate } from './gridPagination.js';
import { renderDiff } from './diffView.js';

export function createGridShell(root, { onEdit, onDelete, canWrite = true } = {}) {
  const shell = document.createElement('div');
  shell.className = 'grid-shell';
  const toolbar = document.createElement('div');
  const view = document.createElement('select');
  for (const v of ['grid', 'diff']) {
    const o = document.createElement('option');
    o.value = v;
    o.textContent = v;
    view.appendChild(o);
  }
  toolbar.appendChild(view);
  const body = document.createElement('div');
  body.className = 'grid-body';
  const table = document.createElement('table');
  const tbody = document.createElement('tbody');
  table.appendChild(tbody);
  body.appendChild(table);
  shell.append(toolbar, body);
  root.appendChild(shell);

  let current = { columns: [], rows: [], queryId: null, before: null };
  const pager = mountPagination(shell, { onPage: (p) => draw(p) });

  function draw(page = 1) {
    // T-03-10: never render a result whose query_id is not the active one.
    if (current.queryId !== null && current.queryId !== getActiveQueryId()) return 'stale-ignored';
    if (view.value === 'diff' && current.before) {
      renderDiff(body, { before: current.before, after: current.rows });
      return 'diff';
    }
    const { rows } = paginate(current.rows, { page, pageSize: 100 });
    renderWindow(tbody, { columns: current.columns, rows, scrollTop: 0, viewportHeight: 600, rowCount: rows.length, onEdit, onDelete, canWrite });
    return 'grid';
  }

  view.onchange = () => draw();
  return {
    element: shell,
    setResult({ columns, rows, queryId, before = null }) {
      current = { columns, rows, queryId, before };
      pager.setTotal(rows.length);
      return draw();
    },
  };
}
