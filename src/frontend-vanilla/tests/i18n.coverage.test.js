// UI-05 lockstep: every t() key must exist in all language files.
// Scans JS sources for t('key') usage + asserts key presence across catalogs.
import { describe, it } from "node:test";
import assert from "node:assert/strict";
import { readdirSync, readFileSync, existsSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { createRequire } from "node:module";

const here = dirname(fileURLToPath(import.meta.url));
const require = createRequire(import.meta.url);
const sharedDir = join(here, "..", "..", "shared", "i18n");
const srcDir = join(here, "..", "src");
const LANGS = ["en-US", "zh-CN", "zh-TW", "de-DE", "ja-JP", "ru-RU"];

function loadLang(l) {
  return require(join(sharedDir, `${l}.json`));
}

function collectTKeys(dir, out = new Set()) {
  if (!existsSync(dir)) return out;
  for (const e of readdirSync(dir, { withFileTypes: true })) {
    const p = join(dir, e.name);
    if (e.isDirectory()) collectTKeys(p, out);
    else if (/\.js$/.test(e.name)) {
      const src = readFileSync(p, "utf8");
      for (const m of src.matchAll(/\bt\(\s*['"]([^'"]+)['"]/g)) out.add(m[1]);
    }
  }
  return out;
}

describe("i18n coverage (UI-05)", () => {
  it("all catalogs load with identical key sets", () => {
    const base = loadLang("en-US");
    const baseKeys = new Set(Object.keys(base));
    assert.ok(baseKeys.size > 1000, `en-US catalog unexpectedly small: ${baseKeys.size}`);
    for (const l of LANGS) {
      const cat = loadLang(l);
      const missing = [...baseKeys].filter((k) => !(k in cat));
      assert.deepEqual(missing.slice(0, 10), [], `${l} missing ${missing.length} keys`);
    }
  });

  it("every t() key used in src exists in all languages", () => {
    const used = collectTKeys(srcDir);
    assert.ok(used.size > 0, "no t() keys found in src — test is vacuous");
    const cats = Object.fromEntries(LANGS.map((l) => [l, loadLang(l)]));
    for (const key of used) {
      if (key.includes("{")) continue; // dynamic keys exempt
      for (const l of LANGS) {
        assert.ok(key in cats[l] || key in cats["en-US"], `key "${key}" missing in ${l} (and en-US fallback)`);
      }
    }
  });

  it("t() interpolates {{vars}} and falls back to en-US", async () => {
    const { t, setLang } = await import("../src/i18n.js");
    setLang("en-US");
    const keys = Object.keys(loadLang("en-US"));
    const withVar = keys.find((k) => /\{\{\w+\}\}/.test(loadLang("en-US")[k]));
    assert.ok(withVar, "no interpolatable key found");
    const raw = loadLang("en-US")[withVar];
    const name = raw.match(/\{\{(\w+)\}\}/)[1];
    const out = t(withVar, { [name]: "X" });
    assert.ok(!out.includes(`{{${name}}}`), `interpolation failed for ${withVar}`);
    setLang("xx-UNKNOWN");
    assert.equal(typeof t(keys[0]), "string");
    setLang("en-US");
  });
});
