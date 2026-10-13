// Global confirm host with APP_NESTED_MODAL_Z_INDEX parity.
import { APP_NESTED_MODAL_Z_INDEX } from "./tabManager.js";

export function confirmDialog({ title, body, okText, cancelText }) {
  return new Promise((resolve) => {
    const overlay = document.createElement("div");
    overlay.className = "wb-modal-overlay";
    overlay.style.zIndex = String(APP_NESTED_MODAL_Z_INDEX);
    const box = document.createElement("div");
    box.className = "wb-modal";
    box.setAttribute("role", "dialog");
    const h = document.createElement("h2");
    h.textContent = title;
    const p = document.createElement("p");
    p.textContent = body;
    const ok = document.createElement("button");
    ok.textContent = okText;
    ok.className = "danger";
    const cancel = document.createElement("button");
    cancel.textContent = cancelText;
    const done = (v) => {
      overlay.remove();
      resolve(v);
    };
    ok.addEventListener("click", () => done(true));
    cancel.addEventListener("click", () => done(false));
    box.append(h, p, ok, cancel);
    overlay.append(box);
    document.body.append(overlay);
    cancel.focus();
  });
}
