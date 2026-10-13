// Pagination helper: pure page slicing.
export function paginate(rows, { page = 1, pageSize = 100 } = {}) {
  const total = rows.length;
  const pages = Math.max(1, Math.ceil(total / pageSize));
  const safe = Math.min(Math.max(1, page), pages);
  return { page: safe, pages, total, rows: rows.slice((safe - 1) * pageSize, safe * pageSize) };
}

export function mountPagination(el, { pageSize = 100, onPage } = {}) {
  let page = 1;
  let total = 0;
  const bar = document.createElement('div');
  bar.className = 'grid-pagination';
  const prev = document.createElement('button');
  prev.textContent = '‹';
  const next = document.createElement('button');
  next.textContent = '›';
  const label = document.createElement('span');
  const draw = () => { label.textContent = `Page ${page} of ${Math.max(1, Math.ceil(total / pageSize))}`; };
  prev.onclick = () => { if (page > 1) { page -= 1; onPage?.(page); draw(); } };
  next.onclick = () => { if (page < Math.ceil(total / pageSize)) { page += 1; onPage?.(page); draw(); } };
  bar.append(prev, label, next);
  el.appendChild(bar);
  draw();
  return {
    element: bar,
    setTotal(n) { total = n; page = 1; draw(); },
    setPage(p) { page = p; draw(); },
  };
}
