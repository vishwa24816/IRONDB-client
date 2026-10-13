// S6 shell mount: #titlebar (32px) / #main { #sidebar 240px | #workbench } / #statusbar.
import { getState, subscribe, setLang as storeSetLang } from "../store.js";
import { setLang as i18nSetLang, t } from "../i18n.js";
import { renderTitlebar } from "./titlebar.js";
import { renderTabs } from "./tabManager.js";
import { renderSettingsEntry } from "./settingsEntry.js";
import "../theme/v2-theme.css";
import "../theme/v2-theme-workbench.css";

export function mountShell(root) {
  root.textContent = "";
  const titlebar = document.createElement("div");
  const main = document.createElement("div");
  main.id = "main";
  const sidebar = document.createElement("aside");
  sidebar.id = "sidebar";
  const workbench = document.createElement("section");
  workbench.id = "workbench";
  const tabstrip = document.createElement("div");
  tabstrip.id = "tabstrip";
  const panel = document.createElement("div");
  panel.id = "panel";
  const statusbar = document.createElement("div");
  statusbar.id = "statusbar";
  workbench.append(tabstrip, panel);
  main.append(sidebar, workbench);
  root.append(titlebar, main, statusbar);

  const render = (state) => {
    i18nSetLang(state.lang);
    document.documentElement.dataset.theme = state.theme;
    sidebar.style.width = `${state.sidebarWidth ?? 240}px`;
    renderTitlebar(titlebar, state);
    renderTabs(tabstrip);
    const active = state.tabs.find((tb) => tb.id === state.activeTabId);
    panel.textContent = "";
    const empty = document.createElement("p");
    if (active?.placeholder) {
      empty.textContent = t("query_editor.empty_state.title");
    } else if (!active) {
      empty.textContent = t("query_editor.empty_state.description");
    } else {
      empty.textContent = active.title ?? active.id;
    }
    panel.append(empty);
    statusbar.textContent = "";
    const st = document.createElement("span");
    st.textContent = active ? `${active.title ?? active.id}` : "";
    statusbar.append(st);
    renderSettingsEntry(sidebar);
  };
  subscribe(render);
  render(getState());
  storeSetLang(getState().lang);
}
