import { spawn } from "node:child_process";
import { realpathSync } from "node:fs";
import { readFile, writeFile } from "node:fs/promises";
import { dirname, join } from "node:path";
import { setTimeout } from "node:timers/promises";
import { fileURLToPath } from "node:url";
import { stopServer } from "./e2e-server.mjs";
import { prepE2eEnv } from "./prep-e2e.mjs";

const DEV_SERVER_URL = process.env.WF_E2E_BASE_URL || "http://localhost:1420";
const BACKEND_URL = process.env.WF_E2E_BACKEND_URL || "http://localhost:8088";
const REPO_ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
const QUOTE_FIXTURE_DIR = join(REPO_ROOT, "e2e", "fixtures", "quotes");
const DEFAULT_RUST_LOG = ["error", "wealthfolio_core::quotes::sync=warn"].join(",");
const cliArgs = process.argv.slice(2);

const buildHealthUrl = (base, path = "/") =>
  new URL(path, `${base.replace(/\/$/, "")}/`).toString();

export const waitForServer = async (
  url,
  serverProcess,
  { path = "/", timeout = 60_000, interval = 500, requestTimeout = 5_000 } = {},
) => {
  const deadline = Date.now() + timeout;
  const healthUrl = buildHealthUrl(url, path);
  let lastStatus;
  let lastError;
  const stopped = new AbortController();
  const onExit = (code, signal) =>
    stopped.abort(new Error(`Dev server exited prematurely (${signal || code})`));
  const onError = (error) => stopped.abort(error);
  serverProcess.once("exit", onExit);
  serverProcess.once("error", onError);

  try {
    while (Date.now() < deadline) {
      if (serverProcess.exitCode !== null || serverProcess.signalCode) {
        throw new Error(
          `Dev server exited prematurely (${serverProcess.signalCode || serverProcess.exitCode})`,
        );
      }
      if (stopped.signal.aborted) throw stopped.signal.reason;

      try {
        const response = await fetch(healthUrl, {
          method: "GET",
          signal: AbortSignal.any([
            stopped.signal,
            AbortSignal.timeout(Math.max(1, Math.min(requestTimeout, deadline - Date.now()))),
          ]),
        });
        await response.body?.cancel();
        if (response.ok) return;
        lastStatus = response.status;
        lastError = undefined;
      } catch (error) {
        if (stopped.signal.aborted) throw stopped.signal.reason;
        lastError = error.message;
      }

      const remaining = deadline - Date.now();
      if (remaining > 0) await setTimeout(Math.min(interval, remaining));
    }

    const details = [lastStatus && `HTTP ${lastStatus}`, lastError].filter(Boolean).join("; ");
    throw new Error(`Timed out waiting for ${healthUrl}: ${details || "no response"}`);
  } finally {
    serverProcess.removeListener("exit", onExit);
    serverProcess.removeListener("error", onError);
  }
};

// `/` needs a web build in dist/; the health route answers without one.
export const waitForBackend = (url, serverProcess, options = {}) =>
  waitForServer(url, serverProcess, {
    path: "/api/v1/healthz",
    timeout: 120_000,
    interval: 1000,
    ...options,
  });

const spawnCommand = (command, args, extraEnv = {}) =>
  spawn(command, args, {
    stdio: "inherit",
    env: { ...process.env, ...extraEnv },
  });

const devServerPort = (() => {
  try {
    return new URL(DEV_SERVER_URL).port || "1420";
  } catch {
    return "1420";
  }
})();

const runPlaywrightTests = (extraArgs = []) =>
  new Promise((resolve, reject) => {
    const tests = spawnCommand("pnpm", ["exec", "playwright", "test", ...extraArgs]);
    tests.once("error", reject);
    tests.once("exit", (code) => {
      if (code === 0) {
        resolve();
      } else {
        reject(new Error(`Playwright exited with code ${code}`));
      }
    });
  });

const run = async () => {
  const envPath = join(REPO_ROOT, ".env.web");
  const originalEnv = await readFile(envPath, "utf8");
  await prepE2eEnv();

  // Signal the existing supervisor directly, rather than a pnpm script wrapper.
  // Its shutdown handler already owns and stops the Rust/Vite child processes.
  const devServer = spawnCommand(process.execPath, [join(REPO_ROOT, "scripts", "dev-web.mjs")], {
    WEALTHFOLIO_E2E: "1",
    WEALTHFOLIO_FIXTURE_DIR: QUOTE_FIXTURE_DIR,
    WEALTHFOLIO_FIXTURE_AS_OF: process.env.WEALTHFOLIO_FIXTURE_AS_OF || "2026-05-12",
    // dev:web keeps a shell value over .env.web; the agent access specs need MCP on.
    WF_MCP_ENABLED: "true",
    RUST_LOG: process.env.WF_E2E_RUST_LOG || DEFAULT_RUST_LOG,
    VITE_DEV_PORT: devServerPort,
  });

  let cleanupPromise;
  const cleanup = () => (cleanupPromise ??= stopServer(devServer, "SIGTERM"));

  const handleSignal = () => {
    cleanup().catch(() => {});
  };

  process.once("SIGINT", handleSignal);
  process.once("SIGTERM", handleSignal);

  try {
    // Wait for both frontend and backend to be ready
    console.log("Waiting for frontend server...");
    await waitForServer(buildHealthUrl(DEV_SERVER_URL), devServer);
    console.log("Frontend ready. Waiting for backend server...");
    await waitForBackend(BACKEND_URL, devServer);
    console.log("Backend ready. Starting tests...");
    await runPlaywrightTests(cliArgs);
  } catch (error) {
    // Preserve the actual startup/test failure even if shutdown also fails.
    console.error(error);
    process.exitCode = 1;
  } finally {
    try {
      await cleanup();
    } finally {
      await writeFile(envPath, originalEnv);
    }
  }
};

if (
  process.argv[1] &&
  realpathSync(process.argv[1]) === realpathSync(fileURLToPath(import.meta.url))
) {
  run().catch((error) => {
    console.error(error);
    process.exitCode = 1;
  });
}
