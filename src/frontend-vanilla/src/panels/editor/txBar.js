// Transaction toolbar port: begin/commit/rollback buttons + mode badge.
import { invoke } from '@tauri-apps/api/core';

export function mountTxBar(el, { connectionId, onStatus } = {}) {
  const bar = document.createElement('div');
  bar.className = 'tx-bar';
  const status = document.createElement('span');
  status.textContent = 'auto';
  const mk = (label, cmd) => {
    const b = document.createElement('button');
    b.textContent = label;
    b.onclick = async () => {
      try {
        await invoke(cmd, { connectionId });
        status.textContent = label.toLowerCase();
        onStatus?.(status.textContent);
      } catch (e) {
        status.textContent = `error: ${e?.message || e}`;
      }
    };
    return b;
  };
  bar.append(mk('Begin', 'DBBegin'), mk('Commit', 'DBCommit'), mk('Rollback', 'DBRollback'), status);
  el.appendChild(bar);
  return bar;
}
