import { MASKED } from "./connectionModal.js";

// Tree with groups + health dot (Sidebar.tsx parity). List payloads show
// masked placeholder only — never a secret.
export function groupConnections(conns, groups = []) {
  const byId = new Map(groups.map((g) => [g.id, { ...g, items: [] }]));
  const ungrouped = { id: "", name: "", items: [] };
  for (const c of conns) {
    const view = { ...c, password: MASKED };
    delete view.secret;
    const g = c.groupId ? byId.get(c.groupId) : null;
    (g ?? ungrouped).items.push(view);
  }
  return [...byId.values(), ...(ungrouped.items.length ? [ungrouped] : [])];
}
export function healthDot(status) {
  return status === "connected" ? "●" : "○";
}
export async function loadConnections(invoke) {
  const list = await invoke("conn_list", {});
  return (Array.isArray(list) ? list : []).map((c) => {
    const { password: _p, secret: _s, ...rest } = c;
    return { ...rest, password: MASKED };
  });
}
