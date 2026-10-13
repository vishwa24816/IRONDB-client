// Shared transfer helpers: formats, polling, secret gate (T-03-13).
export const FORMATS = ['csv', 'json', 'sql'];

export function isSupportedFormat(f) {
  return FORMATS.includes(f);
}

async function tauriInvoke(cmd, args) {
  const { invoke } = await import('@tauri-apps/api/core');
  return invoke(cmd, args);
}

// Poll `pollFn(jobId)` until status is done/failed or attempts run out.
export async function pollJob(jobId, pollFn, { intervalMs = 50, attempts = 40 } = {}) {
  let last = null;
  for (let i = 0; i < attempts; i++) {
    last = await pollFn(jobId);
    if (last?.status === 'done' || last?.status === 'failed') return last;
    await new Promise((r) => setTimeout(r, intervalMs));
  }
  return last ?? { job_id: jobId, status: 'unknown' };
}

// Secrets excluded unless the user checked the box AND supplied a password.
export function secretsAllowed({ includeSecretsChecked, password }) {
  return includeSecretsChecked === true && Boolean(password);
}

export { tauriInvoke };
