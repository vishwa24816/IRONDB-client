// Run/Cancel toolbar + stale-response guard (T-03-10).
// New Run cancels the prior query; results render only if query_id matches activeQueryId.
let activeQueryId = 0;
let seq = 0;

async function tauriInvoke(cmd, args) {
  const { invoke } = await import('@tauri-apps/api/core');
  return invoke(cmd, args);
}

export function getActiveQueryId() {
  return activeQueryId;
}

// Pure guard for tests: true when this result may render.
export function shouldRender(resultQueryId, currentActive) {
  return resultQueryId === currentActive;
}

function nextQueryId() {
  seq += 1;
  return `q-${Date.now()}-${seq}`;
}

export async function runQuery({ connectionId, sql, maxRows = 1000, timeoutMs = 30000, onResult }) {
  if (activeQueryId) {
    try { await tauriInvoke('db_cancel', { queryId: activeQueryId }); } catch { /* best effort */ }
  }
  const queryId = nextQueryId();
  activeQueryId = queryId;
  let result;
  try {
    result = await tauriInvoke('DBQueryWithCancel', {
      connectionId, sql, query_id: queryId, maxRows, timeoutMs,
    });
  } catch (e) {
    result = { success: false, message: String(e?.message || e), data: null, query_id: queryId };
  }
  if (!shouldRender(result?.query_id ?? queryId, activeQueryId)) return { stale: true, result: null };
  onResult?.(result);
  return { stale: false, result };
}

export async function cancelActive() {
  if (!activeQueryId) return false;
  try {
    await tauriInvoke('db_cancel', { queryId: activeQueryId });
    return true;
  } catch {
    return false;
  }
}

export function mountToolbar(el, opts) {
  const bar = document.createElement('div');
  bar.className = 'editor-toolbar';
  bar.innerHTML = '';
  const run = document.createElement('button');
  run.textContent = 'Run';
  const cancel = document.createElement('button');
  cancel.textContent = 'Cancel';
  const maxRows = document.createElement('select');
  for (const n of [100, 500, 1000, 5000]) {
    const o = document.createElement('option');
    o.value = String(n);
    o.textContent = `${n} rows`;
    if (n === 1000) o.selected = true;
    maxRows.appendChild(o);
  }
  const tx = document.createElement('select');
  for (const m of ['auto', 'manual']) {
    const o = document.createElement('option');
    o.value = m;
    o.textContent = m;
    tx.appendChild(o);
  }
  const badge = document.createElement('span');
  if (opts?.readonly) badge.textContent = 'readonly';
  run.onclick = () => opts?.onRun?.({ maxRows: Number(maxRows.value), txMode: tx.value });
  cancel.onclick = () => cancelActive();
  bar.append(run, cancel, maxRows, tx, badge);
  el.appendChild(bar);
  return {
    element: bar,
    keyHandler(e) {
      if ((e.ctrlKey || e.metaKey) && e.key === 'Enter') opts?.onRun?.({ maxRows: Number(maxRows.value), txMode: tx.value });
      else if (e.key === 'Escape') cancelActive();
    },
  };
}
