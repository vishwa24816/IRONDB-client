// Table overview fields (port of TableOverview.tsx: name, columns, row hint).
export function overviewFor(table, columns = [], { estimatedRows = null } = {}) {
  return {
    table,
    columnCount: columns.length,
    columns: columns.map((c) => (typeof c === 'string' ? { name: c } : c)),
    estimatedRows,
  };
}

export function renderOverview(el, overview) {
  el.replaceChildren();
  const h = document.createElement('h3');
  h.textContent = overview.table;
  el.appendChild(h);
  const p = document.createElement('p');
  p.textContent = `${overview.columnCount} columns`;
  el.appendChild(p);
  const ul = document.createElement('ul');
  for (const c of overview.columns) {
    const li = document.createElement('li');
    li.textContent = c.name; // textContent only
    ul.appendChild(li);
  }
  el.appendChild(ul);
}
