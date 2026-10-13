// Port of tabManager/ drag-reorder + pins + adaptive width (vanilla, no React).
import { getState, setState } from "../store.js";

export const APP_NESTED_MODAL_Z_INDEX = 1100;

export function reorderTabs(fromIndex, toIndex) {
  const tabs = [...getState().tabs];
  const [moved] = tabs.splice(fromIndex, 1);
  tabs.splice(toIndex, 0, moved);
  setState({ tabs });
}

export function togglePin(id) {
  const tabs = getState().tabs.map((t) => (t.id === id ? { ...t, pinned: !t.pinned } : t));
  // pinned tabs sort first, stable otherwise
  tabs.sort((a, b) => Number(b.pinned) - Number(a.pinned));
  setState({ tabs });
}

// Adaptive width: pinned tabs fixed 40px, rest share available width capped at 180px.
export function tabWidth(tabCount, pinnedCount, containerWidth) {
  if (tabCount <= 0) return 0;
  const pinnedW = 40;
  const rest = Math.max(0, containerWidth - pinnedCount * pinnedW);
  const flex = tabCount - pinnedCount;
  if (flex <= 0) return pinnedW;
  return Math.min(180, Math.floor(rest / flex));
}

export function renderTabs(root) {
  const { tabs, activeTabId } = getState();
  root.textContent = "";
  for (const tab of tabs) {
    const el = document.createElement("button");
    el.className = "wb-tab" + (tab.id === activeTabId ? " active" : "") + (tab.pinned ? " pinned" : "");
    el.draggable = true;
    el.dataset.tabId = tab.id;
    const label = document.createElement("span");
    label.textContent = `${tab.pinned ? "📌 " : ""}${tab.title ?? tab.id}`;
    el.append(label);
    el.addEventListener("click", () => setState({ activeTabId: tab.id }));
    el.addEventListener("dragstart", (e) => e.dataTransfer.setData("text/wb-tab", tab.id));
    el.addEventListener("dragover", (e) => e.preventDefault());
    el.addEventListener("drop", (e) => {
      e.preventDefault();
      const fromId = e.dataTransfer.getData("text/wb-tab");
      const ids = getState().tabs.map((t) => t.id);
      reorderTabs(ids.indexOf(fromId), ids.indexOf(tab.id));
    });
    root.append(el);
  }
}
