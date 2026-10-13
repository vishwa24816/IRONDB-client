// 32px titlebar (S6 shell chrome).
import { t } from "../i18n.js";

export function renderTitlebar(root, state) {
  root.textContent = "";
  root.id = "titlebar";
  const title = document.createElement("span");
  title.className = "wb-title";
  title.textContent = "irondb-rs";
  const lang = document.createElement("select");
  lang.setAttribute("aria-label", t("settings.title"));
  for (const l of ["en-US", "zh-CN", "zh-TW", "de-DE", "ja-JP", "ru-RU"]) {
    const opt = document.createElement("option");
    opt.value = l;
    opt.textContent = l;
    if (state.lang === l) opt.selected = true;
    lang.append(opt);
  }
  lang.addEventListener("change", () => {
    import("../store.js").then((m) => m.setLang(lang.value));
    import("../i18n.js").then((m) => m.setLang(lang.value));
  });
  root.append(title, lang);
}
