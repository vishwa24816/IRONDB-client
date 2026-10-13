export function newGroup(name = "") {
  return { id: `g-${Date.now()}`, name };
}
export function renameGroup(groups, id, name) {
  return groups.map((g) => (g.id === id ? { ...g, name } : g));
}
export function renderGroupModal(group = { name: "" }) {
  return `<div class="group-modal"><input name="groupName" value="${group.name ?? ""}"/></div>`;
}
