// t(key, vars) over shared catalogs by reference — never fork keys.
// Catalogs live in irondb-rs/shared/i18n/*.json (copied from original).
import enUS from "../../shared/i18n/en-US.json" with { type: "json" };
import zhCN from "../../shared/i18n/zh-CN.json" with { type: "json" };
import zhTW from "../../shared/i18n/zh-TW.json" with { type: "json" };
import deDE from "../../shared/i18n/de-DE.json" with { type: "json" };
import jaJP from "../../shared/i18n/ja-JP.json" with { type: "json" };
import ruRU from "../../shared/i18n/ru-RU.json" with { type: "json" };

export const catalogs = { "en-US": enUS, en: enUS, "zh-CN": zhCN, "zh-TW": zhTW, "de-DE": deDE, "ja-JP": jaJP, "ru-RU": ruRU };
export const SUPPORTED_LANGS = ["en-US", "zh-CN", "zh-TW", "de-DE", "ja-JP", "ru-RU"];
export const FALLBACK_LANG = "en-US";

let lang = "en-US";
export function setLang(l) {
  lang = catalogs[l] ? l : FALLBACK_LANG;
}
export function getLang() {
  return lang;
}

export function t(key, vars = {}) {
  const table = catalogs[lang] ?? {};
  let s = table[key] ?? catalogs[FALLBACK_LANG][key] ?? key;
  s = String(s);
  for (const [k, v] of Object.entries(vars)) s = s.replaceAll(`{{${k}}}`, String(v));
  return s;
}
