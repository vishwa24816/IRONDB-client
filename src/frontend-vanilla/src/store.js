// Single root store: connections, tabs, editor drafts, results cache, tree, lang, theme.
// Actions mutate store only; panels re-render via subscribe/renderAll.
const listeners = new Set();
const state = {
  connections: [],
  tabs: [],
  activeTabId: null,
  drafts: {},
  results: {},
  tree: {},
  lang: "en-US",
  theme: "dark",
  sidebarWidth: 240,
};

export function getState() {
  return state;
}

export function setState(patch) {
  Object.assign(state, patch);
  for (const fn of listeners) fn(state);
}

export function subscribe(fn) {
  listeners.add(fn);
  return () => listeners.delete(fn);
}

export function renderAll() {
  for (const fn of listeners) fn(state);
}

// --- tabs ---
let tabSeq = 0;
export function openTab(tab) {
  const id = tab.id ?? `tab-${++tabSeq}`;
  state.tabs = [...state.tabs, { pinned: false, ...tab, id }];
  state.activeTabId = id;
  renderAll();
  return id;
}

export function closeTab(id) {
  state.tabs = state.tabs.filter((t) => t.id !== id);
  delete state.drafts[id];
  delete state.results[id];
  if (state.activeTabId === id) state.activeTabId = state.tabs.at(-1)?.id ?? null;
  renderAll();
}

export function activateTab(id) {
  state.activeTabId = id;
  renderAll();
}

// --- drafts / results / tree (local UI state only; secrets never stored here) ---
export function setDraft(tabId, sql) {
  state.drafts = { ...state.drafts, [tabId]: sql };
  renderAll();
}

export function setResult(tabId, result) {
  // ponytail: stale-response guard lives in callers — only render if query_id matches active execution
  state.results = { ...state.results, [tabId]: result };
  renderAll();
}

export function setTree(connId, nodes) {
  state.tree = { ...state.tree, [connId]: nodes };
  renderAll();
}

export function setLang(lang) {
  state.lang = lang;
  renderAll();
}

export function setTheme(theme) {
  state.theme = theme;
  renderAll();
}
