import { invoke } from "@tauri-apps/api/core";
import { mountShell } from "./shell/shell.js";

const root = document.getElementById("root");
mountShell(root);

// Bridge health check preserved from the scaffold seam.
const status = document.getElementById("status");
invoke("ping")
  .then((result) => {
    if (status) status.textContent = String(result);
  })
  .catch((err) => {
    if (status) status.textContent = `ping failed: ${err}`;
  });
