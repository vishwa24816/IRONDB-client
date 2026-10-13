// Per-tab Monaco lifecycle. Dynamic import keeps node tests green;
// ?worker ESM wires the editor worker in the vite build.
let monacoPromise = null;
const models = new Map(); // tabId -> model

function loadMonaco() {
  if (!monacoPromise) {
    self.MonacoEnvironment = {
      getWorker(_, label) {
        const makers = {
          editorWorkerService: () => import('monaco-editor/esm/vs/editor/editor.worker?worker'),
          css: () => import('monaco-editor/esm/vs/language/css/css.worker?worker'),
          html: () => import('monaco-editor/esm/vs/language/html/html.worker?worker'),
          json: () => import('monaco-editor/esm/vs/language/json/json.worker?worker'),
          typescript: () => import('monaco-editor/esm/vs/language/typescript/ts.worker?worker'),
        };
        return new (makers[label] || makers.editorWorkerService)();
      },
    };
    monacoPromise = import('monaco-editor').then((m) => {
      m.editor.defineTheme('irondb-dark', { base: 'vs-dark', inherit: true, rules: [], colors: { 'editor.background': '#141414' } });
      m.editor.defineTheme('irondb-light', { base: 'vs', inherit: true, rules: [], colors: {} });
      return m;
    });
  }
  return monacoPromise;
}

export async function createEditorForTab(container, tabId, { value = '', language = 'sql', theme = 'irondb-dark' } = {}) {
  const monaco = await loadMonaco();
  disposeTab(tabId);
  const model = monaco.editor.createModel(value, language);
  models.set(tabId, model);
  return monaco.editor.create(container, { model, theme, automaticLayout: true, minimap: { enabled: false } });
}

export function getModel(tabId) {
  return models.get(tabId) || null;
}

export function disposeTab(tabId) {
  const model = models.get(tabId);
  if (model) {
    model.dispose();
    models.delete(tabId);
  }
}

// Feed schema completion from the object-browser cache (table/column names).
export async function registerCompletion(items) {
  const monaco = await loadMonaco();
  return monaco.languages.registerCompletionItemProvider('sql', {
    provideCompletionItems(model, position) {
      const word = model.getWordUntilPosition(position);
      const suggestions = (items || []).map((t) => ({
        label: t,
        kind: monaco.languages.CompletionItemKind.Field,
        insertText: t,
        range: { startLineNumber: position.lineNumber, endLineNumber: position.lineNumber, startColumn: word.startColumn, endColumn: word.endColumn },
      }));
      return { suggestions };
    },
  });
}
