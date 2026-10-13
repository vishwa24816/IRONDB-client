// Virtualized row-window core (T-03-09: textContent only, never innerHTML).
export const ROW_HEIGHT = 28;
export const OVERSCAN = 10;

export function visibleWindow({ scrollTop, viewportHeight, rowCount, rowHeight = ROW_HEIGHT, overscan = OVERSCAN }) {
  const end = Math.min(rowCount, Math.ceil((scrollTop + viewportHeight) / rowHeight) + overscan);
  const start = Math.min(Math.max(0, Math.floor(scrollTop / rowHeight) - overscan), end);
  return { start, end };
}

export function autoWidth(columns, rows, { min = 60, max = 400, charPx = 8, pad = 16 } = {}) {
  const widths = {};
  for (const c of columns) {
    let w = String(c).length;
    for (let i = 0; i < Math.min(rows.length, 200); i++) {
      w = Math.max(w, String(rows[i]?.[c] ?? '').length);
    }
    widths[c] = Math.min(max, Math.max(min, w * charPx + pad));
  }
  return widths;
}

export function copySelection(rows, selected) {
  return selected.map(([r, c]) => String(rows[r]?.[c] ?? '')).join('\n');
}

// Render rows [start, end) as absolute-positioned <tr>; cell values via textContent only.
export function renderGrid(tbody, { columns, rows, start = 0, end = rows.length, rowHeight = ROW_HEIGHT, onEdit, onDelete, canWrite = true }) {
  tbody.replaceChildren();
  for (let i = start; i < Math.min(end, rows.length); i++) {
    const tr = document.createElement('tr');
    tr.style.position = 'absolute';
    tr.style.top = `${i * rowHeight}px`;
    for (const c of columns) {
      const td = document.createElement('td');
      td.textContent = String(rows[i]?.[c] ?? ''); // textContent only (T-03-09)
      if (canWrite && onEdit) {
        const col = c;
        const row = i;
        td.ondblclick = () => onEdit(row, col, rows[row]?.[col]);
      }
      tr.appendChild(td);
    }
    if (canWrite && onDelete) {
      const del = document.createElement('td');
      const b = document.createElement('button');
      b.textContent = '×';
      const row = i;
      b.onclick = () => onDelete(row);
      del.appendChild(b);
      tr.appendChild(del);
    }
    tbody.appendChild(tr);
  }
}

// Render only the visible window; returns { start, end } for scroll handlers.
export function renderWindow(tbody, opts) {
  const { start, end } = visibleWindow(opts);
  renderGrid(tbody, { ...opts, start, end });
  return { start, end };
}
