export const MASKED = "••••••";
// Mirror of ConnectionStep2MongoRedisFields (redis section): topology,
// database index list, password write-only.
export const REDIS_FIELDS = [
  { name: "host", type: "text", required: true },
  { name: "port", type: "number" },
  { name: "redisTopology", type: "select", options: ["standalone", "cluster", "sentinel"] },
  { name: "redisDb", type: "number" },
  { name: "username", type: "text" },
  { name: "password", type: "secret", placeholder: MASKED },
];
export function redisSection(form = {}) {
  return REDIS_FIELDS.map((f) => ({ ...f, value: f.type === "secret" ? "" : (form[f.name] ?? "") }));
}
