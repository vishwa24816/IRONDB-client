// Import flow with preview (parity: file, format, table, header row, delimiter).
import { isSupportedFormat, pollJob, tauriInvoke } from './exportOptions.js';
import { previewSummary } from './preview.js';

export function defaultImportForm() {
  return { file: '', format: 'csv', table: '', hasHeader: true, delimiter: ',' };
}

export function validateImportForm(f) {
  if (!f.file) return { ok: false, error: 'file required' };
  if (!isSupportedFormat(f.format)) return { ok: false, error: 'unsupported format' };
  if (!f.table?.trim()) return { ok: false, error: 'table required' };
  return { ok: true };
}

export async function startImport(form, { connectionId, invokeFn = tauriInvoke, pollFn = null } = {}) {
  const v = validateImportForm(form);
  if (!v.ok) return { ok: false, error: v.error };
  const res = await invokeFn('import_run', { connectionId, table: form.table, format: form.format });
  if (!res?.success) return { ok: false, error: res?.message ?? 'import failed' };
  const jobId = res?.data?.job_id;
  const summary = previewSummary([], 0);
  if (!pollFn) return { ok: true, jobId, summary };
  const final = await pollJob(jobId, pollFn);
  return { ok: final?.status !== 'failed', jobId, status: final?.status, summary };
}
