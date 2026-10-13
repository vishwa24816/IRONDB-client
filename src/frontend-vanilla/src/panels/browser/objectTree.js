// Capability-gated lazy object tree (T-03-14: single-flight expand per node,
// capability gate avoids dead calls; unsupported nodes hidden, not disabled).
export const CAP_IN_MEMORY = 0x01;
export const CAP_TRANSACTIONS = 0x02;
export const CAP_SCHEMAS = 0x04;

// Levels gated by a required cap bit; absent bit => level never requested.
const LEVEL_CAP = { schemas: CAP_SCHEMAS };

export function levelVisible(level, caps) {
  const need = LEVEL_CAP[level];
  return need === undefined || (caps & need) !== 0;
}

async function tauriInvoke(cmd, args) {
  const { invoke } = await import('@tauri-apps/api/core');
  return invoke(cmd, args);
}

// Single-flight: concurrent expands of the same node share one invoke.
const pending = new Map();
function flightKey(connectionId, level, parent) {
  return `${connectionId}|${level}|${parent ?? ''}`;
}

export function clearPending() {
  pending.clear();
}

export async function expandNode({ connectionId, level, parent = null, caps = 0, invokeFn = tauriInvoke }) {
  if (!levelVisible(level, caps)) return [];
  const key = flightKey(connectionId, level, parent);
  if (pending.has(key)) return pending.get(key);
  const p = (async () => {
    try {
      const res = await invokeFn('db_objects', { connectionId, level, parent, caps });
      if (!res?.success) return [];
      return res?.data?.children ?? [];
    } finally {
      pending.delete(key);
    }
  })();
  pending.set(key, p);
  return p;
}

// Context menu descriptors; host wires Design/View data/Copy name.
export function contextMenuFor(node) {
  const items = [{ id: 'copy-name', label: 'Copy name' }];
  if (node?.kind === 'table') {
    items.unshift({ id: 'view-data', label: 'View data' }, { id: 'design', label: 'Design' });
  }
  return items;
}

// Monaco completion cache fed on every successful expand.
const completionCache = new Map();
export function feedCompletionCache(nodes) {
  for (const n of nodes ?? []) {
    if (n?.kind === 'table' || n?.kind === 'column') completionCache.set(n.name, n.kind);
  }
}
export function getCompletions() {
  return [...completionCache.entries()].map(([name, kind]) => ({ name, kind }));
}
export function clearCompletions() {
  completionCache.clear();
}
