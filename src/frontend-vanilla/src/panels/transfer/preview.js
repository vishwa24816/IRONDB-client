// Import preview: first-N-rows peek before a full import run.
export const PREVIEW_ROWS = 5;

export function previewRows(rows, n = PREVIEW_ROWS) {
  return (rows ?? []).slice(0, n);
}

export function previewSummary(columns = [], totalRows = 0) {
  return { columns: [...columns], totalRows, shown: Math.min(PREVIEW_ROWS, totalRows) };
}

export function renderPreview(el, { columns, rows }) {
  el.replaceChildren();
  const table = document.createElement('table');
  const head = document.createElement('tr');
  for (const c of columns) {
    const th = document.createElement('th');
    th.textContent = String(c); // textContent only
    head.appendChild(th);
  }
  table.appendChild(head);
  for (const r of previewRows(rows)) {
    const tr = document.createElement('tr');
    for (const c of columns) {
      const td = document.createElement('td');
      td.textContent = String(r?.[c] ?? '');
      tr.appendChild(td);
    }
    table.appendChild(tr);
  }
  el.appendChild(table);
}
