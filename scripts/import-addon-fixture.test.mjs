import assert from "node:assert/strict";
import test from "node:test";
import { crc32 } from "node:zlib";
import { importAddonFixture } from "../e2e/fixtures/import-addons/fixture.ts";
import { ActivityType } from "../packages/addon-sdk/src/data-types.ts";

// Inspect the actual uncompressed package, rather than testing a second copy
// of the fixture's manifest/code. No browser or host bridge is replaced in E2E.
function readFixtureZip(buffer) {
  const files = new Map();
  let offset = 0;
  while (buffer.readUInt32LE(offset) === 0x04034b50) {
    assert.equal(buffer.readUInt16LE(offset + 8), 0);
    const size = buffer.readUInt32LE(offset + 18);
    const nameLength = buffer.readUInt16LE(offset + 26);
    const extraLength = buffer.readUInt16LE(offset + 28);
    const name = buffer.subarray(offset + 30, offset + 30 + nameLength).toString();
    const start = offset + 30 + nameLength + extraLength;
    const content = buffer.subarray(start, start + size);
    assert.equal(crc32(content), buffer.readUInt32LE(offset + 14));
    files.set(name, content.toString());
    offset = start + size;
  }
  assert.deepEqual([...files.keys()], ["manifest.json", "dist/addon.js"]);
  return files;
}

for (const tab of ["investments", "spending"]) {
  test(`${tab} fixture registers its declared route at its own nonempty host path`, async () => {
    const id = `host-import-${tab}-contract-test`;
    const fixture = importAddonFixture(id, tab, "fixture-owned-account");
    const files = readFixtureZip(fixture.zip);
    const manifest = JSON.parse(files.get("manifest.json"));
    const handoff = files
      .get("dist/addon.js")
      .match(/context\.api\.activities\.openImport\((.*)\);/);
    assert.ok(handoff, "the generated addon must hand its draft to the real host API");
    const request = JSON.parse(handoff[1]);
    assert.equal(request.accountId, "fixture-owned-account");
    assert.ok(Object.values(ActivityType).includes(request.rows[0].activity.activityType));
    assert.equal(
      request.rows[0].activity.activityType,
      tab === "investments" ? "DEPOSIT" : "WITHDRAWAL",
    );
    const addon = await import(
      `data:text/javascript;base64,${Buffer.from(files.get("dist/addon.js")).toString("base64")}`
    );
    let registered;
    let acknowledge;
    const acknowledgement = new Promise((resolve) => (acknowledge = resolve));
    let enabled = false;
    const enabling = addon
      .enable({
        router: {
          add(route) {
            registered = route;
            return acknowledgement;
          },
        },
      })
      .then(() => (enabled = true));
    await Promise.resolve();
    assert.equal(enabled, false, "enable must wait for the real host registration acknowledgement");
    assert.equal(registered.path, `/addons/${id}`);
    assert.equal(registered.id, fixture.route);
    assert.equal(typeof registered.render, "function");
    assert.deepEqual(manifest.contributes.routes, [{ id: registered.id }]);
    assert.deepEqual(Object.keys(manifest.contributes.links), [
      tab === "investments" ? "investment-import" : "spending-import",
    ]);
    assert.equal(
      manifest.contributes.links[Object.keys(manifest.contributes.links)[0]][0].route,
      registered.id,
    );
    acknowledge();
    await enabling;
    assert.equal(enabled, true);
  });
}
