import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { once, EventEmitter } from "node:events";
import {
  chmod,
  copyFile,
  mkdir,
  mkdtemp,
  readFile,
  realpath,
  rm,
  writeFile,
} from "node:fs/promises";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { stopServer } from "./e2e-server.mjs";
import { splitWorkersArg, waitForBackend, waitForServer } from "./run-e2e.mjs";
import { setTimeout } from "node:timers/promises";

async function listen(t, handler) {
  const server = createServer(handler);
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  t.after(async () => {
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  });
  return `http://127.0.0.1:${server.address().port}`;
}

for (const testExitCode of [0, 1]) {
  test(`the production runner restores the environment after test exit ${testExitCode}`, async (t) => {
    const directory = await mkdtemp(join(tmpdir(), "wealthfolio-e2e-env-"));
    t.after(() => rm(directory, { recursive: true, force: true }));
    await mkdir(join(directory, "scripts"));
    await mkdir(join(directory, "bin"));
    for (const name of ["run-e2e.mjs", "prep-e2e.mjs", "dev-web.mjs", "e2e-server.mjs"]) {
      await copyFile(new URL(name, import.meta.url), join(directory, "scripts", name));
    }

    const envPath = join(directory, ".env.web");
    const original = "WF_DATA_DIR=/daily-data\nWF_AUTH_REQUIRED=true\n";
    await writeFile(envPath, original);
    // The runner gets deterministic responses without starting a real app server.
    const origin = "http://127.0.0.1:8188";
    const mockFetch = join(directory, "mock-fetch.mjs");
    await writeFile(
      mockFetch,
      `globalThis.fetch = async (url) => {
  const { origin, pathname } = new URL(url);
  if (origin !== "${origin}") throw new Error("Unexpected server URL");
  if (pathname === "/api/v1/settings") return Response.json({ onboardingCompleted: true });
  if (pathname === "/api/v1/healthz" || pathname === "/") return new Response("ready");
  throw new Error("Unexpected server route");
};`,
    );
    await writeFile(
      join(directory, "bin", "cargo"),
      `#!${process.execPath}
const fs = require("node:fs");
if (process.argv[2] === "build") process.exit(0);
fs.writeFileSync("worker-env.json", JSON.stringify({
  data: process.env.WF_DATA_DIR,
  static: process.env.WF_STATIC_DIR,
  listen: process.env.WF_LISTEN_ADDR,
}));
process.on("SIGTERM", () => process.exit(0));
setInterval(() => {}, 1000);
`,
    );
    await writeFile(
      join(directory, "bin", "pnpm"),
      `#!${process.execPath}
const fs = require("node:fs");
if (process.argv[2] !== "exec" || process.argv.includes("--list")) process.exit(0);
(async () => {
  for (let attempts = 0; attempts < 100 && !fs.existsSync("worker-env.json"); attempts++) {
    await new Promise(resolve => setTimeout(resolve, 10));
  }
  if (!fs.existsSync("worker-env.json")) process.exit(99);
  fs.appendFileSync("playwright-runs", process.argv.slice(2).join(" ") + "\\n");
  process.exit(${testExitCode});
})();
`,
    );
    for (const name of ["cargo", "pnpm"]) await chmod(join(directory, "bin", name), 0o755);
    const runner = spawn(
      process.execPath,
      [
        "--import",
        mockFetch,
        join(directory, "scripts", "run-e2e.mjs"),
        "--workers=1",
        "--retries=0",
        "--forbid-only",
      ],
      {
        cwd: directory,
        env: {
          ...process.env,
          PATH: `${join(directory, "bin")}:${process.env.PATH}`,
          WF_E2E_BACKEND_URL: origin,
        },
        stdio: ["ignore", "ignore", "pipe"],
      },
    );
    t.after(() => stopServer(runner, "SIGTERM", 100));
    let errors = "";
    runner.stderr.on("data", (chunk) => (errors += chunk));
    const [code] = await once(runner, "exit");
    assert.equal(code, testExitCode, errors);
    assert.equal(await readFile(envPath, "utf8"), original);
    const worker = JSON.parse(await readFile(join(directory, "worker-env.json"), "utf8"));
    assert.match(worker.data, /app-testing-[^/]+\/worker-0$/);
    assert.equal(worker.static, join(await realpath(directory), "dist"));
    assert.equal(worker.listen, `127.0.0.1:${new URL(origin).port}`);
    const runs = await readFile(join(directory, "playwright-runs"), "utf8");
    assert.match(runs, /--project onboarding --workers 1 .*--retries=0 --forbid-only/);
    assert.match(runs, /--project chromium --workers 1 .*--retries=0 --forbid-only/);
    if (testExitCode) assert.match(errors, /Playwright exited with code 1/);
  });
}

test("a stalled HTTP response cannot outlive the readiness deadline", async (t) => {
  const origin = await listen(t, () => {});
  const child = Object.assign(new EventEmitter(), { exitCode: null, signalCode: null });
  const started = Date.now();
  await assert.rejects(
    waitForServer(origin, child, { timeout: 100, interval: 10, requestTimeout: 50 }),
    /Timed out waiting/,
  );
  assert.ok(Date.now() - started < 2_000);
  assert.equal(child.listenerCount("exit"), 0);
  assert.equal(child.listenerCount("error"), 0);
});

test("backend HTTP failures remain failures and identify the response status", async (t) => {
  const origin = await listen(t, (_request, response) => {
    response.statusCode = 503;
    response.end("not ready");
  });
  const child = Object.assign(new EventEmitter(), { exitCode: null, signalCode: null });
  await assert.rejects(
    waitForServer(origin, child, { timeout: 100, interval: 10 }),
    /Timed out waiting.*HTTP 503/,
  );
});

test("spawn failure interrupts readiness instead of waiting for its deadline", async (t) => {
  const origin = await listen(t, () => {});
  const child = spawn("/does-not-exist/wealthfolio-dev-server");
  const started = Date.now();
  await assert.rejects(waitForServer(origin, child), /ENOENT|prematurely/);
  assert.ok(Date.now() - started < 2_000);
});

test("shutdown is bounded even when the owned child ignores termination", async () => {
  const child = spawn(process.execPath, [
    "-e",
    'process.on("SIGTERM", () => {}); console.log("ready"); setInterval(() => {}, 1000);',
  ]);
  await once(child.stdout, "data");
  const started = Date.now();
  await stopServer(child, "SIGTERM", 100);
  assert.equal(child.signalCode, "SIGKILL");
  assert.ok(Date.now() - started < 2_000);
  // A signaled process has exitCode=null; do not wait for an already emitted exit.
  await stopServer(child, "SIGTERM", 100);
});

const BACKEND_URL = "http://127.0.0.1:8188";
const runningServer = Object.assign(new EventEmitter(), { exitCode: null, signalCode: null });

// A backend without a web build: the API answers, `/` has no index.html.
const withBackendWithoutWebBuild = async (callback) => {
  const originalFetch = globalThis.fetch;
  const requested = [];
  globalThis.fetch = async (url) => {
    requested.push(url);
    return { ok: url === `${BACKEND_URL}/api/v1/healthz` };
  };
  try {
    await callback(requested);
  } finally {
    globalThis.fetch = originalFetch;
  }
};

test("the backend counts as ready without a web build", async () => {
  await withBackendWithoutWebBuild(async (requested) => {
    for (const url of [BACKEND_URL, `${BACKEND_URL}/`]) {
      await waitForBackend(url, runningServer, { timeout: 100, interval: 1 });
    }
    assert.deepEqual(requested, [`${BACKEND_URL}/api/v1/healthz`, `${BACKEND_URL}/api/v1/healthz`]);
  });
});

test("a timeout names the URL that was polled", async () => {
  await withBackendWithoutWebBuild(async () => {
    await assert.rejects(
      waitForServer(BACKEND_URL, runningServer, { timeout: 20, interval: 1 }),
      /Timed out waiting/,
    );
  });
});

// Like a pnpm that hands off to the pinned version: it waits on its child
// synchronously and doesn't pass a signal on.
const HANDOFF = `
  process.on("SIGINT", () => {});
  require("node:child_process").spawnSync(process.execPath, [
    "-e",
    "console.log(process.pid); setInterval(() => {}, 1000);",
  ], { stdio: "inherit" });
`;

const stopsTheChild = async (t) => {
  const server = spawn(process.execPath, ["-e", HANDOFF], {
    stdio: ["ignore", "pipe", "inherit"],
  });
  let childPid;
  try {
    const [output] = await once(server.stdout, "data", { signal: t.signal });
    childPid = Number(String(output).trim());
    const stopped = await Promise.race([
      stopServer(server, "SIGINT").then(() => true),
      setTimeout(5000, false, { ref: false }),
    ]);
    assert.equal(stopped, true);
    assert.throws(() => process.kill(childPid, 0), { code: "ESRCH" });
  } finally {
    for (const pid of [childPid, server.pid].filter(Boolean)) {
      try {
        process.kill(pid, "SIGKILL");
      } catch {
        // already stopped
      }
    }
  }
};

test(
  "stopping a server reaches a child its launcher doesn't signal",
  { skip: process.platform === "win32", timeout: 10_000 },
  stopsTheChild,
);

test(
  "stopping a server reaches that child on Linux without ps",
  { skip: process.platform !== "linux", timeout: 10_000 },
  async (t) => {
    const path = process.env.PATH;
    // An empty PATH leaves `ps` unfound; the server is spawned by absolute path.
    process.env.PATH = "";
    try {
      await stopsTheChild(t);
    } finally {
      if (path === undefined) delete process.env.PATH;
      else process.env.PATH = path;
    }
  },
);

test("a --workers argument sets the server count and leaves the rest", () => {
  assert.deepEqual(splitWorkersArg(["--workers", "2", "e2e/a.spec.ts"]), {
    workers: 2,
    rest: ["e2e/a.spec.ts"],
  });
  assert.deepEqual(splitWorkersArg(["--workers=3", "--headed"]), {
    workers: 3,
    rest: ["--headed"],
  });
  assert.deepEqual(splitWorkersArg(["-j", "1"]), { workers: 1, rest: [] });
  assert.deepEqual(splitWorkersArg(["--ui"]), { workers: undefined, rest: ["--ui"] });
});

test("a worker count that isn't a whole number is rejected", () => {
  for (const args of [["--workers=50%"], ["--workers", "50%"], ["-j", "2.5"], ["--workers"]]) {
    assert.throws(() => splitWorkersArg(args), /--workers takes a whole number of workers/);
  }
});
