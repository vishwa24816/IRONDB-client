import { describe, it } from "node:test";
import assert from "node:assert/strict";
import {
  connectionReadOnly, productionRiskConfirm, needsProdConfirm,
  queueAudit, drainAudit, READONLY_BLOCK_KEY, PROD_CONFIRM_KEY,
} from "../src/panels/connections/guards.js";
import { saveConnection, exportPackage } from "../src/panels/connections/connectionModal.js";

describe("guards", () => {
  it("readonly write blocked pre-invoke, reads pass", () => {
    const conn = { id: "c1", readOnly: true };
    assert.equal(connectionReadOnly(conn, "DROP TABLE t").blocked, true);
    assert.equal(connectionReadOnly(conn, "DROP TABLE t").messageKey, READONLY_BLOCK_KEY);
    assert.equal(connectionReadOnly(conn, "SELECT 1").blocked, false);
    assert.equal(connectionReadOnly({ id: "c2" }, "DROP TABLE t").blocked, false);
  });
  it("prod run needs typed-name confirm; audit queued", () => {
    const prod = { id: "p", name: "prod-db", environmentType: "production" };
    assert.equal(needsProdConfirm(prod), true);
    assert.equal(productionRiskConfirm(prod, "wrong").confirmed, false);
    assert.equal(productionRiskConfirm(prod, "wrong").messageKey, PROD_CONFIRM_KEY);
    assert.equal(productionRiskConfirm(prod, "prod-db").confirmed, true);
    queueAudit({ connId: "p", action: "run" });
    assert.equal(drainAudit().length, 1);
  });
  it("save strips secrets; export excludes by default", async () => {
    const calls = [];
    const invoke = async (m, a) => { calls.push(m); };
    const stored = await saveConnection(invoke, { id: "c1", password: "hunter2" }, "hunter2");
    assert.deepEqual(calls, ["conn_save", "secret_store"]);
    assert.ok(!("password" in stored) || stored.password === "••••••");
    assert.equal(stored.keyringRef, true);
    const out = exportPackage([{ id: "c1", password: "x" }]);
    assert.equal(out[0].password, "••••••");
    assert.throws(() => exportPackage([{ id: "c1" }], { includeSecrets: true }));
  });
});
