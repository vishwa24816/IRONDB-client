// Settings entry: opens settings modal shell (full settings deferred past Phase 3).
import { t } from "../i18n.js";

export function renderSettingsEntry(root) {
  root.textContent = "";
  const btn = document.createElement("button");
  btn.id = "settings-entry";
  btn.textContent = t("settings.title");
  btn.addEventListener("click", () => {
    import("./dialogs.js").then((m) =>
      m.confirmDialog({
        title: t("settings.title"),
        body: t("settings.language.description"),
        okText: t("common.ok"),
        cancelText: t("common.close"),
      }),
    );
  });
  root.append(btn);
}
