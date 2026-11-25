import { spawn } from "node:child_process";
import { realpathSync } from "node:fs";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { dirname, join } from "node:path";
import { setTimeout } from "node:timers/promises";
import { fileURLToPath } from "node:url";
import { stopServer } from "./e2e-server.mjs";
import { prepE2eEnv } from "./prep-e2e.mjs";

// Each Playwright worker gets its own server and database, on consecutive ports
// from this one. The server serves the built app, so app and API share an origin.
const FIRST_URL = new URL(process.env.WF_E2E_BACKEND_URL || "http://localhost:8088");
const REPO_ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
const QUOTE_FIXTURE_DIR = join(REPO_ROOT, "e2e", "fixtures", "quotes");
const DEFAULT_RUST_LOG = ["error", "wealthfolio_core::quotes::sync=warn"].join(",");
// `--workers N` (also `--workers=N`, `-j N`) sets how many servers start, so it
// never exceeds them; it is passed on to Playwright as the runner's own flag.
// Each worker needs a server started before Playwright runs, so the count must
// be a whole number: Playwright's percentages are rejected.
const parseWorkers = (value) => {
  if (!/^\d+$/.test(value ?? "")) {
    throw new Error(`--workers takes a whole number of workers, got "${value ?? ""}"`);
  }
  return Number(value);
};

export const splitWorkersArg = (args) => {
  const rest = [];
  let workers;
  for (let index = 0; index < args.length; index += 1) {
    const arg = args[index];
    if (arg.startsWith("--workers=")) workers = parseWorkers(arg.slice("--workers=".length));
    else if (arg === "--workers" || arg === "-j") {
      workers = parseWorkers(args[index + 1]);
      index += 1;
    } else rest.push(arg);
  }
  return { workers, rest };
};

const { workers: workersArg, rest: cliArgs } = splitWorkersArg(process.argv.slice(2));
const WORKERS = Math.max(
  1,
  workersArg ||
    (process.env.WF_E2E_WORKERS ? parseWorkers(process.env.WF_E2E_WORKERS) : undefined) ||
    4,
);
const shouldUseUi = cliArgs.includes("--ui");

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

const runCommand = (command, args, extraEnv = {}) =>
  new Promise((resolve, reject) => {
    const child = spawnCommand(command, args, extraEnv);
    child.once("error", reject);
    child.once("exit", (code) => {
      if (code === 0) resolve();
      else reject(new Error(`${command} ${args.join(" ")} exited with code ${code}`));
    });
  });

const workerUrl = (index) => {
  const url = new URL(FIRST_URL);
  url.port = String(Number(FIRST_URL.port || 80) + index);
  return url.origin;
};

// What the onboarding screens save, for servers whose specs don't onboard.
const onboard = async (url) => {
  const settings = await fetch(`${url}/api/v1/settings`).then((response) => response.json());
  if (settings.onboardingCompleted) return;
  const response = await fetch(`${url}/api/v1/settings`, {
    method: "PUT",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({
      language: "en",
      baseCurrency: "CAD",
      timezone: Intl.DateTimeFormat().resolvedOptions().timeZone,
      theme: "light",
      autoUpdateCheckEnabled: false,
      onboardingCompleted: true,
    }),
  });
  if (!response.ok) throw new Error(`Onboarding ${url} failed with ${response.status}`);
};

const runPlaywrightTests = (extraArgs = [], extraEnv = {}) =>
  new Promise((resolve, reject) => {
    const tests = spawnCommand("pnpm", ["exec", "playwright", "test", ...extraArgs], extraEnv);
    tests.once("error", reject);
    tests.once("exit", (code) => {
      if (code === 0) {
        resolve();
      } else {
        reject(new Error(`Playwright exited with code ${code}`));
      }
    });
  });

// Each project's run passes with no matching tests, so check once that the
// filters match at least one test across both before building anything.
const checkTestsMatch = () =>
  new Promise((resolve, reject) => {
    const list = spawn(
      "pnpm",
      [
        "exec",
        "playwright",
        "test",
        ...cliArgs,
        // After the arguments: `--project` takes every value up to the next flag.
        "--list",
        "--reporter",
        "list",
        "--project",
        "onboarding",
        "--project",
        "chromium",
      ],
      { stdio: ["ignore", "pipe", "pipe"], env: process.env },
    );
    let output = "";
    list.stdout.on("data", (chunk) => (output += chunk));
    list.stderr.on("data", (chunk) => (output += chunk));
    list.once("error", reject);
    list.once("exit", (code) => {
      if (code === 0) resolve();
      else {
        process.stderr.write(output);
        reject(new Error("No tests match the given arguments"));
      }
    });
  });

const runWithPreparedEnv = async (dataDirectory) => {
  // Tests run against the built app, not the Vite dev server: every test opens
  // a fresh browser, and a cold dev-server load fetches ~1,400 modules (1.7-7 s)
  // where the bundle takes about 1 s. `VITE_E2E` keeps the `?dir=` and `?swipe=`
  // test switches in this build.
  console.log("Building the web app and the server...");
  await runCommand("pnpm", ["build:types"]);
  await runCommand("pnpm", ["build"], { VITE_E2E: "true" });
  await runCommand("cargo", ["build", "--manifest-path", "apps/server/Cargo.toml"]);

  // Specs change app-wide settings and read whole-database totals, so workers
  // can't share a database: each gets its own server, data folder and port.
  const urls = Array.from({ length: WORKERS }, (_, index) => workerUrl(index));
  const servers = await Promise.all(
    urls.map(async (url, index) => {
      const folder = join(dataDirectory, `worker-${index}`);
      await mkdir(folder, { recursive: true });
      return spawnCommand("node", [join(REPO_ROOT, "scripts", "dev-web.mjs"), "--server-only"], {
        WEALTHFOLIO_E2E: "1",
        WEALTHFOLIO_FIXTURE_DIR: QUOTE_FIXTURE_DIR,
        WEALTHFOLIO_FIXTURE_AS_OF: process.env.WEALTHFOLIO_FIXTURE_AS_OF || "2026-05-12",
        // dev:web keeps a shell value over .env.web; the agent access specs need MCP on.
        WF_MCP_ENABLED: "true",
        RUST_LOG: process.env.WF_E2E_RUST_LOG || DEFAULT_RUST_LOG,
        WF_STATIC_DIR: join(REPO_ROOT, "dist"),
        WF_LISTEN_ADDR: `127.0.0.1:${new URL(url).port}`,
        WF_DATA_DIR: folder,
        WF_DB_PATH: join(folder, "app.db"),
        WF_SECRET_FILE: join(folder, "vault.bin"),
        WF_ADDONS_DIR: join(folder, "addons"),
      });
    }),
  );

  let cleanupPromise;
  const cleanup = () =>
    (cleanupPromise ??= Promise.all(servers.map((server) => stopServer(server, "SIGTERM"))));

  const handleSignal = () => {
    process.exitCode = 1;
    cleanup().catch(console.error);
  };

  process.once("SIGINT", handleSignal);
  process.once("SIGTERM", handleSignal);

  // Helpers pick a worker's server from Playwright's parallel index.
  const testEnv = {
    WF_E2E_WORKER_URLS: urls.join(","),
    WF_E2E_BASE_URL: "",
    WF_E2E_BACKEND_URL: "",
  };

  try {
    console.log(`Waiting for ${WORKERS} servers...`);
    await Promise.all(
      urls.map(async (url, index) => {
        await waitForBackend(url, servers[index]);
        await waitForServer(url, servers[index]);
      }),
    );
    // The first server stays new: 01-happy-path onboards it through the UI.
    await Promise.all(urls.slice(1).map(onboard));
    console.log("Servers ready. Starting tests...");
    if (shouldUseUi) {
      await runPlaywrightTests(["--workers", String(WORKERS), ...cliArgs], testEnv);
      return;
    }
    let failure;
    await runPlaywrightTests(
      [
        "--project",
        "onboarding",
        "--workers",
        "1",
        "--pass-with-no-tests",
        "--output",
        join("test-results", "onboarding"),
        ...cliArgs,
      ],
      // Each run keeps its results and report in its own folder, since Playwright
      // empties those folders when it starts. This report doesn't open and wait
      // while the next run is due.
      {
        ...testEnv,
        PLAYWRIGHT_HTML_OUTPUT_DIR: join("playwright-report", "onboarding"),
        PLAYWRIGHT_HTML_OPEN: "never",
      },
    ).catch((error) => {
      failure = error;
    });
    await onboard(urls[0]);
    await runPlaywrightTests(
      [
        "--project",
        "chromium",
        "--workers",
        String(WORKERS),
        "--pass-with-no-tests",
        "--output",
        join("test-results", "app"),
        ...cliArgs,
      ],
      { ...testEnv, PLAYWRIGHT_HTML_OUTPUT_DIR: join("playwright-report", "app") },
    ).catch((error) => {
      failure ??= error;
    });
    if (failure) throw failure;
  } catch (error) {
    process.exitCode = 1;
    throw error;
  } finally {
    try {
      await cleanup();
    } catch (error) {
      if (!process.exitCode) throw error;
      console.error(error);
    } finally {
      process.removeListener("SIGINT", handleSignal);
      process.removeListener("SIGTERM", handleSignal);
    }
  }
};

const run = async () => {
  if (!shouldUseUi) await checkTestsMatch();
  const envPath = join(REPO_ROOT, ".env.web");
  let originalEnv;
  try {
    originalEnv = await readFile(envPath, "utf8");
  } catch (error) {
    if (error.code !== "ENOENT") throw error;
    originalEnv = await readFile(`${envPath}.example`, "utf8");
  }
  try {
    await runWithPreparedEnv(await prepE2eEnv());
  } finally {
    await writeFile(envPath, originalEnv);
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
