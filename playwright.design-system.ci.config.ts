import { defineConfig } from "@playwright/test";
import config from "./playwright.design-system.config";

// CI compares to reviewed Linux snapshots, never to snapshots it just generated.
// Keep the original config's same-machine local diagnostic workflow available.
export default defineConfig(config, {
  snapshotPathTemplate: "e2e/design-system/baselines/{projectName}/{arg}{ext}",
  updateSnapshots: "none",
  forbidOnly: true,
  retries: 0,
  webServer: {
    ...config.webServer,
    command: "pnpm --filter frontend exec vite --host 127.0.0.1 --port 4176",
    url: "http://127.0.0.1:4176/e2e/design-system/",
    reuseExistingServer: false,
  },
});
