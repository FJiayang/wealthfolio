import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { once, EventEmitter } from "node:events";
import { existsSync } from "node:fs";
import { chmod, copyFile, mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { stopServer, waitForServer } from "./run-e2e.mjs";

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
  test(
    `health readiness launches Playwright and restores the environment after exit ${testExitCode}`,
    { timeout: 10_000 },
    async (t) => {
      const directory = await mkdtemp(join(tmpdir(), "wealthfolio-e2e-env-"));
      t.after(() => rm(directory, { recursive: true, force: true }));
      await mkdir(join(directory, "scripts"));
      await mkdir(join(directory, "bin"));
      for (const name of ["run-e2e.mjs", "prep-e2e.mjs", "dev-web.mjs"])
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
      await writeFile(envPath, original);
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
          },
          stdio: ["ignore", "ignore", "pipe"],
        },
      );
      t.after(() => stopServer(runner, { timeout: 100 }));
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
  await stopServer(child, { timeout: 100 });
  assert.equal(child.signalCode, "SIGKILL");
  assert.ok(Date.now() - started < 2_000);
  // A signaled process has exitCode=null; do not wait for an already emitted exit.
  await stopServer(child, { timeout: 100 });
});
