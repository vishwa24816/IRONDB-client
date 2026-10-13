export const MASKED = "••••••";
// Mirror of ConnectionModalStep2 host/auth fields (mysql + generic SQL):
// host, port, database, user, password (write-only), ssl, readOnly flag,
// environment type, protection flags.
export const MYSQL_FIELDS = [
  { name: "host", type: "text", required: true },
  { name: "port", type: "number" },
  { name: "database", type: "text" },
  { name: "username", type: "text", required: true },
  { name: "password", type: "secret", placeholder: MASKED },
  { name: "useSSL", type: "boolean" },
  { name: "readOnly", type: "boolean" },
  { name: "environmentType", type: "select", options: ["development", "testing", "production"] },
];
export function mysqlSection(form = {}) {
  return MYSQL_FIELDS.map((f) => ({ ...f, value: f.type === "secret" ? "" : (form[f.name] ?? "") }));
}
