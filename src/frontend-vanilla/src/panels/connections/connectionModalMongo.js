export const MASKED = "••••••";
// Mirror of ConnectionStep2MongoRedisFields (mongo section): srv/standard,
// topology, auth mechanism, read preference, members discovery.
export const MONGO_FIELDS = [
  { name: "mongoSrv", type: "segment", options: ["standard", "srv"] },
  { name: "mongoTopology", type: "select", options: ["standalone", "replicaSet", "sharded"] },
  { name: "host", type: "text", required: true },
  { name: "port", type: "number" },
  { name: "database", type: "text" },
  { name: "username", type: "text" },
  { name: "password", type: "secret", placeholder: MASKED },
  { name: "mongoAuthMechanism", type: "select", options: ["default", "scram-sha-1", "scram-sha-256", "x509"] },
  { name: "mongoReadPreference", type: "select", options: ["primary", "secondary", "nearest"] },
];
export function mongoSection(form = {}) {
  return MONGO_FIELDS.map((f) => ({ ...f, value: f.type === "secret" ? "" : (form[f.name] ?? "") }));
}
