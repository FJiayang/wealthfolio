import { spawn } from "node:child_process";
import { once } from "node:events";
import { readFile, writeFile } from "node:fs/promises";
import { dirname, join } from "node:path";
import { setTimeout } from "node:timers/promises";
import { fileURLToPath } from "node:url";
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
  { timeout = 60_000, interval = 500, requestTimeout = 5_000 } = {},
) => {
  const deadline = Date.now() + timeout;
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
        const response = await fetch(url, {
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
    throw new Error(`Timed out waiting for ${url}: ${details || "no response"}`);
  } finally {
    serverProcess.removeListener("exit", onExit);
    serverProcess.removeListener("error", onError);
  }
};

export const stopServer = async (serverProcess, { timeout = 5_000 } = {}) => {
  if (serverProcess.exitCode !== null || serverProcess.signalCode) return;
  const exited = once(serverProcess, "exit").then(() => true);
  const waitForExit = () => Promise.race([exited, setTimeout(timeout, false, { ref: false })]);
  serverProcess.kill("SIGTERM");
  if (await waitForExit()) return;
  serverProcess.kill("SIGKILL");
  if (!(await waitForExit())) throw new Error("Dev server did not exit after forced shutdown");
};

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
    RUST_LOG: process.env.WF_E2E_RUST_LOG || DEFAULT_RUST_LOG,
    VITE_DEV_PORT: devServerPort,
  });

  let cleanupPromise;
  const cleanup = () => (cleanupPromise ??= stopServer(devServer));

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
    // The backend's root serves dist/index.html, which dev mode does not build.
    // Probe the same API health endpoint as wait-for-both-servers-to-be-ready.sh.
    await waitForServer(buildHealthUrl(BACKEND_URL, "/api/v1/healthz"), devServer, {
      timeout: 120_000,
      interval: 1000,
    });
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

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  run().catch((error) => {
    console.error(error);
    process.exitCode = 1;
  });
}
