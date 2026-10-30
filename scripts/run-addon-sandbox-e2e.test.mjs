import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { chmod, copyFile, mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

for (const exitCode of [0, 1]) {
  test(`forwards CI flags and preserves the browser exit code ${exitCode}`, async (t) => {
    const directory = await mkdtemp(join(tmpdir(), "wealthfolio-sandbox-runner-"));
    t.after(() => rm(directory, { recursive: true, force: true }));
    await mkdir(join(directory, "bin"));
    const runner = join(directory, "run-addon-sandbox-e2e.mjs");
    await copyFile(new URL("run-addon-sandbox-e2e.mjs", import.meta.url), runner);
    const command = join(directory, "bin", "pnpm");
    await writeFile(
      command,
      `#!${process.execPath}
const fs = require("node:fs");
if (process.argv[2] === "--filter") {
  process.on("SIGTERM", () => process.exit(0));
  setInterval(() => {}, 1000);
} else {
  fs.writeFileSync(process.env.ARGS_PATH, JSON.stringify(process.argv.slice(2)));
  process.exit(${exitCode});
}
`,
    );
    await chmod(command, 0o755);
    const server = createServer((_request, response) => response.end("ready"));
    await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
    t.after(() => new Promise((resolve) => server.close(resolve)));
    const flags = [
      "--retries=0",
      "--forbid-only",
      "--trace=retain-on-failure",
      "--reporter=line,html",
    ];
    const argsPath = join(directory, "args.json");
    const child = spawn(process.execPath, [runner, ...flags], {
      cwd: directory,
      env: {
        ...process.env,
        PATH: `${join(directory, "bin")}:${process.env.PATH}`,
        ARGS_PATH: argsPath,
        WF_ADDON_SANDBOX_SKIP_BUILD: "true",
        WF_ADDON_SANDBOX_BASE_URL: `http://127.0.0.1:${server.address().port}`,
      },
      stdio: ["ignore", "ignore", "pipe"],
    });
    let errors = "";
    child.stderr.on("data", (data) => (errors += data));
    const [code] = await once(child, "exit");
    assert.equal(code, exitCode, errors);
    assert.deepEqual(JSON.parse(await readFile(argsPath, "utf8")), [
      "exec",
      "playwright",
      "test",
      "--config",
      "playwright.addon-sandbox.config.ts",
      ...flags,
    ]);
  });
}
