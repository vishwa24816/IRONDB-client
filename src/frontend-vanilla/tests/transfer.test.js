import test from 'node:test';
import assert from 'node:assert/strict';
import { secretsAllowed, pollJob, isSupportedFormat } from '../src/panels/transfer/exportOptions.js';
import { validateImportForm, startImport } from '../src/panels/transfer/importPanel.js';
import { validateExportForm, exportGate, startExport } from '../src/panels/transfer/exportDialog.js';
import { previewRows, previewSummary } from '../src/panels/transfer/preview.js';

test('secrets excluded unless checkbox plus password', () => {
  assert.equal(secretsAllowed({ includeSecretsChecked: false, password: 'x' }), false);
  assert.equal(secretsAllowed({ includeSecretsChecked: true, password: '' }), false);
  assert.equal(secretsAllowed({ includeSecretsChecked: true, password: 'pw' }), true);
});

test('pollJob resolves on done', async () => {
  const seq = [{ status: 'queued' }, { status: 'done' }];
  const r = await pollJob('j1', async () => seq.shift(), { intervalMs: 1 });
  assert.equal(r.status, 'done');
});

test('import/export validation + polled run', async () => {
  assert.equal(validateImportForm({ file: '', format: 'csv', table: 't' }).ok, false);
  assert.equal(isSupportedFormat('xml'), false);
  const invokeFn = async (cmd) => ({
    success: true, data: { job_id: 'j-1', status: 'queued', secretsExcluded: cmd === 'export_run' },
  });
  const pollFn = async () => ({ status: 'done' });
  const im = await startImport(
    { file: 'a.csv', format: 'csv', table: 'users' }, { connectionId: 'c1', invokeFn, pollFn });
  assert.equal(im.ok, true);
  assert.equal(im.status, 'done');
  const ex = await startExport(
    { file: 'b.sql', format: 'sql', table: 'users' }, { connectionId: 'c1', invokeFn, pollFn });
  assert.equal(ex.ok, true);
  assert.equal(ex.secretsExcluded, true);
  assert.equal(validateExportForm({ file: '', format: 'sql', table: 't' }).ok, false);
});

test('export gate routes through guards.js', () => {
  const ro = exportGate({ conn: { readOnly: true }, sql: 'DROP TABLE t' });
  assert.equal(ro.blocked, true);
  const prod = exportGate({ conn: { environmentType: 'production', name: 'p' }, typedName: 'wrong' });
  assert.equal(prod.blocked, true);
  assert.equal(prod.needsConfirm, true);
  assert.equal(exportGate({ conn: { environmentType: 'production', name: 'p' }, typedName: 'p' }).blocked, false);
});

test('preview caps rows', () => {
  const rows = Array.from({ length: 10 }, (_, i) => ({ a: i }));
  assert.equal(previewRows(rows).length, 5);
  assert.deepEqual(previewSummary(['a'], 10), { columns: ['a'], totalRows: 10, shown: 5 });
});
