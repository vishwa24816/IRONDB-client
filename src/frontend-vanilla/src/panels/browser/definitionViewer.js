// Read-only DDL preview (DDL never editable; textContent only).
async function tauriInvoke(cmd, args) {
  const { invoke } = await import('@tauri-apps/api/core');
  return invoke(cmd, args);
}

export async function fetchDdl({ connectionId, table, invokeFn = tauriInvoke }) {
  const res = await invokeFn('db_table_ddl', { connectionId, table });
  if (!res?.success) return { ok: false, message: res?.message ?? 'failed' };
  return { ok: true, ddl: res?.data?.ddl ?? '' };
}

export function renderDdl(el, ddl) {
  el.replaceChildren();
  const pre = document.createElement('pre');
  pre.setAttribute('readonly', '');
  pre.textContent = ddl; // textContent only, never innerHTML
  el.appendChild(pre);
}
