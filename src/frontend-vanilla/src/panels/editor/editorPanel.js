// Per-tab editor panel: hosts one Monaco model per tab, wires toolbar + tx bar.
import { createEditorForTab, disposeTab, getModel } from './monacoHost.js';
import { mountToolbar, runQuery } from './editorToolbar.js';
import { mountTxBar } from './txBar.js';

export function createEditorPanel(root, { connectionId, readonly = false, onResult } = {}) {
  const tabs = new Map(); // tabId -> { container, editor }
  let current = null;
  const panel = document.createElement('div');
  panel.className = 'editor-panel';
  const tabStrip = document.createElement('div');
  const host = document.createElement('div');
  host.className = 'editor-host';
  const toolbarHost = document.createElement('div');
  panel.append(tabStrip, toolbarHost, host);

  const toolbar = mountToolbar(toolbarHost, {
    readonly,
    onRun: ({ maxRows, txMode }) => {
      const model = current ? getModel(current) : null;
      if (!model) return;
      runQuery({ connectionId, sql: model.getValue(), maxRows, timeoutMs: 30000, onResult, txMode });
    },
  });
  mountTxBar(toolbarHost, { connectionId });
  host.addEventListener('keydown', (e) => toolbar.keyHandler(e));
  root.appendChild(panel);

  return {
    element: panel,
    async openTab(tabId, sql = '') {
      const container = document.createElement('div');
      host.replaceChildren(container);
      const editor = await createEditorForTab(container, tabId, { value: sql });
      tabs.set(tabId, { container, editor });
      current = tabId;
      const btn = document.createElement('button');
      btn.textContent = tabId;
      btn.onclick = () => this.openTab(tabId, getModel(tabId)?.getValue() ?? sql);
      tabStrip.appendChild(btn);
    },
    closeTab(tabId) {
      tabs.get(tabId)?.editor?.dispose();
      disposeTab(tabId);
      tabs.delete(tabId);
      if (current === tabId) current = null;
    },
  };
}
