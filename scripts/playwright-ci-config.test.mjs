import assert from "node:assert/strict";
import { registerHooks } from "node:module";
import test from "node:test";

// Load the real configs and installed Playwright defineConfig implementation,
// without starting a server or browser. Node 24 handles TypeScript directly;
// this narrow resolver supplies Playwright's extensionless local-import behavior.
const hooks = registerHooks({
  resolve(specifier, context, nextResolve) {
    return nextResolve(
      specifier === "./playwright.design-system.config" ? `${specifier}.ts` : specifier,
      context,
    );
  },
});
let base;
let config;
try {
  base = (await import("../playwright.design-system.config.ts")).default;
  config = (await import("../playwright.design-system.ci.config.ts")).default;
} finally {
  hooks.deregister();
}

test("design CI starts exactly one isolated server", () => {
  const servers = Array.isArray(config.webServer) ? config.webServer : [config.webServer];
  assert.equal(servers.length, 1, "defineConfig must not append a second server on the same port");
  assert.equal(servers[0].reuseExistingServer, false);
  assert.equal(servers[0].url, base.webServer.url);
  assert.equal(servers[0].command, base.webServer.command);
});

test("design CI preserves every browser and assertion while failing closed on snapshots", () => {
  assert.deepEqual(config.projects, base.projects);
  assert.deepEqual(config.expect, base.expect);
  assert.deepEqual(config.use, base.use);
  assert.equal(config.testDir, base.testDir);
  assert.equal(config.updateSnapshots, "none");
  assert.equal(config.forbidOnly, true);
  assert.equal(config.retries, 0);
  assert.equal(config.snapshotPathTemplate, "e2e/design-system/baselines/{projectName}/{arg}{ext}");
});

test("CI overrides do not mutate the local diagnostic configuration", () => {
  assert.equal(base.webServer.reuseExistingServer, true);
  assert.equal(base.retries, 1);
  assert.equal(
    base.snapshotPathTemplate,
    ".local/design-system/baselines/{projectName}/{arg}{ext}",
  );
});
