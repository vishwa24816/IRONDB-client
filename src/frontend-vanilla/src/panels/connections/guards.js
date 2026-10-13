export const READONLY_BLOCK_KEY = "query_editor.message.connection_readonly_blocked";
export const PROD_CONFIRM_KEY = "connection.production_risk.message";
export const DELETE_KEY = "connection.delete";

const WRITE_RE = /\b(insert|update|delete|drop|alter|truncate|create|grant|revoke)\b/i;

export function isWriteSql(sql) {
  return WRITE_RE.test(String(sql ?? ""));
}

// Frozen semantics (contracts/golden/guard_semantics.json):
// readonly write/DDL -> blocked pre-invoke, never reaches invoke.
export function connectionReadOnly(conn, sql) {
  if (!conn) return { blocked: false };
  const forced = conn.readOnly === true || conn.protection?.restrictScriptExecution === true;
  if (!forced) return { blocked: false };
  if (sql !== undefined && !isWriteSql(sql)) return { blocked: false };
  return { blocked: true, messageKey: READONLY_BLOCK_KEY, toast: true };
}

export function needsProdConfirm(conn, action = "run") {
  if (!conn || conn.environmentType !== "production") return false;
  if (action === "run" || action === "destructive") return true;
  return false;
}

// Type-to-confirm: input must equal connection name exactly.
export function productionRiskConfirm(conn, typedName) {
  if (!needsProdConfirm(conn)) return { confirmed: true };
  const ok = typedName === conn.name;
  return ok
    ? { confirmed: true }
    : { confirmed: false, messageKey: PROD_CONFIRM_KEY };
}

// Audit entry queued per gated execution (sessionAlerts parity note).
const auditQueue = [];
export function queueAudit(entry) {
  auditQueue.push({ ...entry, at: new Date().toISOString() });
}
export function drainAudit() {
  return auditQueue.splice(0, auditQueue.length);
}
