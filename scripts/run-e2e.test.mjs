import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { once, EventEmitter } from "node:events";
import { existsSync } from "node:fs";
import { chmod, copyFile, mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { stopServer } from "./e2e-server.mjs";
import { waitForBackend, waitForServer } from "./run-e2e.mjs";
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

for (const [testExitCode, existing] of [
  [0, true],
  [1, true],
  [0, false],
]) {
  test(
    `health readiness restores ${existing ? "existing" : "new"} environment after exit ${testExitCode}`,
    { timeout: 10_000 },
    async (t) => {
      const directory = await mkdtemp(join(tmpdir(), "wealthfolio-e2e-env-"));
      t.after(() => rm(directory, { recursive: true, force: true }));
      await mkdir(join(directory, "scripts"));
      await mkdir(join(directory, "bin"));
      for (const name of ["run-e2e.mjs", "prep-e2e.mjs", "dev-web.mjs", "e2e-server.mjs"])
        await copyFile(new URL(name, import.meta.url), join(directory, "scripts", name));

      const original = [
        "WF_DATA_DIR=/daily-data",
        "WF_DB_PATH=/daily-data/app.db",
        "WF_SECRET_FILE=/daily-data/vault.bin",
        "WF_ADDONS_DIR=/daily-data/addons",
        "WF_AUTH_REQUIRED=true",
        "WF_AUTH_PASSWORD_HASH=synthetic-hash",
        "",
      ].join("\n");
      const envPath = join(directory, ".env.web");
      await writeFile(existing ? envPath : `${envPath}.example`, original);
      // Run the real supervisor with owned synthetic cargo/Vite children. No
      // application, browser, or production database is started by this unit test.
      for (const name of ["cargo", "pnpm"]) {
        const command = join(directory, "bin", name);
        await writeFile(
          command,
          `#!${process.execPath}
const fs = require("node:fs");
const path = require("node:path");
const root = process.env.FIXTURE_ROOT;
if (${JSON.stringify(name)} === "pnpm" && process.argv[2] === "exec") {
  if (!fs.existsSync(path.join(root, "health-ready"))) process.exit(99);
  fs.writeFileSync(path.join(root, "playwright-args.json"), JSON.stringify(process.argv.slice(2)));
  process.exit(${testExitCode});
}
if (${JSON.stringify(name)} === "pnpm")
  fs.writeFileSync(path.join(root, "vite-api-target"), process.env.VITE_API_TARGET || "");
process.on("SIGTERM", () => {
  fs.writeFileSync(path.join(root, "${name}-stopped"), "stopped");
  process.exit(0);
});
fs.writeFileSync(path.join(root, "${name}-ready"), "ready");
setInterval(() => {}, 1000);
`,
        );
        await chmod(command, 0o755);
      }
      const frontend = await listen(t, (_request, response) => {
        response.statusCode =
          existsSync(join(directory, "cargo-ready")) && existsSync(join(directory, "pnpm-ready"))
            ? 200
            : 503;
        response.end("frontend");
      });
      const backendRequests = [];
      const backend = await listen(t, (request, response) => {
        backendRequests.push(request.url);
        response.statusCode = request.url === "/api/v1/healthz" ? 200 : 404;
        if (response.statusCode === 200) {
          writeFile(join(directory, "health-ready"), "ready").then(() => response.end("ok"));
        } else response.end("no production dist in dev mode");
      });
      const flags = ["--workers=1", "--retries=0", "--forbid-only"];
      const runner = spawn(
        process.execPath,
        [join(directory, "scripts", "run-e2e.mjs"), ...flags],
        {
          cwd: directory,
          env: {
            ...process.env,
            PATH: `${join(directory, "bin")}:${process.env.PATH}`,
            FIXTURE_ROOT: directory,
            WF_E2E_BASE_URL: frontend,
            WF_E2E_BACKEND_URL: backend,
            VITE_API_TARGET: "http://127.0.0.1:9999",
          },
          stdio: ["ignore", "ignore", "pipe"],
        },
      );
      t.after(() => stopServer(runner, "SIGTERM", 100));
      let errors = "";
      runner.stderr.on("data", (chunk) => (errors += chunk));
      const [code] = await once(runner, "exit");
      assert.equal(code, testExitCode, errors);
      assert.deepEqual(backendRequests, ["/api/v1/healthz"]);
      assert.deepEqual(
        JSON.parse(await readFile(join(directory, "playwright-args.json"), "utf8")),
        ["exec", "playwright", "test", ...flags],
      );
      assert.equal(await readFile(envPath, "utf8"), original);
      assert.equal(await readFile(join(directory, "vite-api-target"), "utf8"), backend);
      for (const name of ["cargo", "pnpm"])
        assert.equal(await readFile(join(directory, `${name}-stopped`), "utf8"), "stopped");
      if (testExitCode) assert.match(errors, /Playwright exited with code 1/);
    },
  );
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

test(
  "stopping a server reaches a child its launcher doesn't signal",
  { skip: process.platform === "win32", timeout: 10_000 },
  async (t) => {
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
  },
);
