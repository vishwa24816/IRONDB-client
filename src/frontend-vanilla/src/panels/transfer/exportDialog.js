// Export flow with options (parity: format, table, file, SQL options,
// include-secrets checkbox + password prompt). Cell flows route through
// guards.js: readonly/prod checks happen before any invoke.
import { connectionReadOnly, needsProdConfirm } from '../connections/guards.js';
import { isSupportedFormat, pollJob, secretsAllowed, tauriInvoke } from './exportOptions.js';

export function defaultExportForm() {
  return {
    file: '', format: 'sql', table: '', includeData: true, includeSchema: true,
    includeSecretsChecked: false, password: '',
  };
}

export function validateExportForm(f) {
  if (!isSupportedFormat(f.format)) return { ok: false, error: 'unsupported format' };
  if (!f.table?.trim()) return { ok: false, error: 'table required' };
  if (!f.file) return { ok: false, error: 'file required' };
  return { ok: true };
}

// Gate via guards.js before invoking. Returns { blocked } or proceeds.
export function exportGate({ conn, sql = 'SELECT 1', typedName = '' } = {}) {
  const ro = connectionReadOnly(conn, sql);
  if (ro.blocked) return { blocked: true, messageKey: ro.messageKey };
  if (needsProdConfirm(conn, 'run') && typedName !== conn?.name) {
    return { blocked: true, needsConfirm: true };
  }
  return { blocked: false };
}

export async function startExport(form, { connectionId, conn = null, invokeFn = tauriInvoke, pollFn = null } = {}) {
  const v = validateExportForm(form);
  if (!v.ok) return { ok: false, error: v.error };
  const includeSecrets = secretsAllowed(form);
  const res = await invokeFn('export_run', {
    connectionId, table: form.table, format: form.format, includeSecrets,
  });
  if (!res?.success) return { ok: false, error: res?.message ?? 'export failed' };
  const jobId = res?.data?.job_id;
  const secretsExcluded = res?.data?.secretsExcluded ?? true;
  if (!pollFn) return { ok: true, jobId, secretsExcluded };
  const final = await pollJob(jobId, pollFn);
  return { ok: final?.status !== 'failed', jobId, status: final?.status, secretsExcluded };
}
