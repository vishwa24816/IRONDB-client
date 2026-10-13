import { MYSQL_FIELDS } from "./connectionModalMysql.js";
import { MONGO_FIELDS } from "./connectionModalMongo.js";
import { REDIS_FIELDS } from "./connectionModalRedis.js";

export const MASKED = "••••••";
export const ENGINES = ["mysql", "postgres", "mongodb", "redis", "sqlite"];

// Dispatch per-engine sections mirroring ConnectionModal field parity.
export function sectionFor(engine, form = {}) {
  const pick = engine === "mongodb" ? MONGO_FIELDS : engine === "redis" ? REDIS_FIELDS : MYSQL_FIELDS;
  return pick.map((f) => ({ ...f, value: f.type === "secret" ? "" : (form[f.name] ?? "") }));
}

// Save: strip secrets from the persisted payload; write secret via
// invoke secret_store only (never in store JSON / list payloads).
export async function saveConnection(invoke, form, secret) {
  const { password: _pw, secret: _s, ...rest } = form;
  const payload = { ...rest, keyringRef: Boolean(secret), password: undefined };
  delete payload.password;
  await invoke("conn_save", payload);
  if (secret) await invoke("secret_store", { connId: form.id, secret });
  return { ...payload, password: MASKED };
}
export async function deleteConnection(invoke, connId) {
  await invoke("conn_delete", { connId });
  await invoke("secret_delete", { connId });
}
// Export: secrets excluded unless explicit checkbox + password prompt.
export function exportPackage(conns, { includeSecrets = false, packagePassword = "" } = {}) {
  if (includeSecrets && !packagePassword) throw new Error("package password required");
  return conns.map((c) => {
    const { password: _p, secret: _s, ...rest } = c;
    return includeSecrets ? { ...rest, sealed: true } : { ...rest, password: MASKED };
  });
}
