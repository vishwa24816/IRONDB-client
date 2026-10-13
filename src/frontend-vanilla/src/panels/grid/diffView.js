// Side-by-side diff of changed cells between two result sets.
export function diffRows(before, after, { key = null, columns = null } = {}) {
  const cols = columns || [...new Set([...Object.keys(before[0] || {}), ...Object.keys(after[0] || {})])];
  const index = new Map();
  if (key) before.forEach((r, i) => index.set(r[key], i));
  const changed = [];
  after.forEach((row, i) => {
    const b = key ? before[index.get(row[key]) ?? -1] : before[i];
    if (!b) {
      changed.push({ row: i, type: 'added', cells: cols.map((c) => ({ column: c, before: undefined, after: row[c] })) });
      return;
    }
    const cells = [];
    for (const c of cols) {
      if (!Object.is(b[c], row[c])) cells.push({ column: c, before: b[c], after: row[c] });
    }
    if (cells.length) changed.push({ row: i, type: 'modified', cells });
  });
  return { columns: cols, changed };
}

export function renderDiff(el, { before, after, ...opts } = {}) {
  const { columns, changed } = diffRows(before, after, opts);
  el.replaceChildren();
  const table = document.createElement('table');
  table.className = 'diff-view';
  const head = document.createElement('tr');
  for (const c of ['row', ...columns.flatMap((c) => [`before:${c}`, `after:${c}`])]) {
    const th = document.createElement('th');
    th.textContent = c;
    head.appendChild(th);
  }
  table.appendChild(head);
  for (const ch of changed) {
    const tr = document.createElement('tr');
    tr.className = `diff-${ch.type}`;
    const num = document.createElement('td');
    num.textContent = String(ch.row);
    tr.appendChild(num);
    const byCol = new Map(ch.cells.map((c) => [c.column, c]));
    for (const c of columns) {
      const cell = byCol.get(c) || { before: after[ch.row]?.[c], after: after[ch.row]?.[c] };
      const b = document.createElement('td');
      b.textContent = String(cell.before ?? '');
      const a = document.createElement('td');
      a.textContent = String(cell.after ?? '');
      if (byCol.has(c)) a.className = 'diff-changed';
      tr.append(b, a);
    }
    table.appendChild(tr);
  }
  el.appendChild(table);
  return { columns, changedCount: changed.length };
}
