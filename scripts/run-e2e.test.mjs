import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { chmod, copyFile, mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

for (const testExitCode of [0, 1]) {
  test(`restores the complete local environment after E2E exits with ${testExitCode}`, async (t) => {
    const directory = await mkdtemp(join(tmpdir(), "wealthfolio-e2e-env-"));
    t.after(() => rm(directory, { recursive: true, force: true }));
    await mkdir(join(directory, "scripts"));
    await mkdir(join(directory, "bin"));
    for (const name of ["run-e2e.mjs", "prep-e2e.mjs"])
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
    const command = join(directory, "bin", "pnpm");
    await writeFile(
      command,
      `#!${process.execPath}
if (process.argv[2] === "run") {
  process.on("SIGINT", () => process.exit(0));
  setInterval(() => {}, 1000);
} else {
  process.exit(${testExitCode});
}
`,
    );
    await chmod(command, 0o755);
    const server = createServer((_request, response) => response.end("ready"));
    await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
    t.after(() => new Promise((resolve) => server.close(resolve)));
    const origin = `http://127.0.0.1:${server.address().port}`;
    const runner = spawn(process.execPath, [join(directory, "scripts", "run-e2e.mjs")], {
      cwd: directory,
      env: {
        ...process.env,
        PATH: `${join(directory, "bin")}:${process.env.PATH}`,
        WF_E2E_BASE_URL: origin,
        WF_E2E_BACKEND_URL: origin,
      },
      stdio: ["ignore", "ignore", "pipe"],
    });
    let errors = "";
    runner.stderr.on("data", (chunk) => {
      errors += chunk;
    });
    const [code] = await once(runner, "exit");
    assert.equal(code, testExitCode, errors);
    assert.equal(await readFile(envPath, "utf8"), original);
  });
}
