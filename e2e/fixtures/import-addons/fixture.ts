import { crc32 } from "node:zlib";
import type { ActivityType } from "../../../packages/addon-sdk/src/data-types";

// Small, uncompressed ZIPs keep the real installation boundary in the test without
// another repository, a package builder, or a test dependency. Names are fixed below.
function zip(files: Record<string, string>): Buffer {
  const local: Buffer[] = [];
  const central: Buffer[] = [];
  let offset = 0;
  for (const [path, source] of Object.entries(files)) {
    const name = Buffer.from(path);
    const content = Buffer.from(source);
    const checksum = crc32(content);
    const header = Buffer.alloc(30);
    header.writeUInt32LE(0x04034b50, 0);
    header.writeUInt16LE(20, 4);
    header.writeUInt32LE(checksum, 14);
    header.writeUInt32LE(content.length, 18);
    header.writeUInt32LE(content.length, 22);
    header.writeUInt16LE(name.length, 26);
    local.push(header, name, content);
    const directory = Buffer.alloc(46);
    directory.writeUInt32LE(0x02014b50, 0);
    directory.writeUInt16LE(20, 4);
    directory.writeUInt16LE(20, 6);
    directory.writeUInt32LE(checksum, 16);
    directory.writeUInt32LE(content.length, 20);
    directory.writeUInt32LE(content.length, 24);
    directory.writeUInt16LE(name.length, 28);
    directory.writeUInt32LE(offset, 42);
    central.push(directory, name);
    offset += header.length + name.length + content.length;
  }
  const directory = Buffer.concat(central);
  const end = Buffer.alloc(22);
  end.writeUInt32LE(0x06054b50, 0);
  end.writeUInt16LE(Object.keys(files).length, 8);
  end.writeUInt16LE(Object.keys(files).length, 10);
  end.writeUInt32LE(directory.length, 12);
  end.writeUInt32LE(offset, 16);
  return Buffer.concat([...local, directory, end]);
}

export function importAddonFixture(id: string, tab: "investments" | "spending", accountId: string) {
  const route = "import-drafts";
  const label = `Fixture ${tab} import`;
  const activityType: ActivityType = tab === "investments" ? "DEPOSIT" : "WITHDRAWAL";
  const request = {
    sourceLabel: label,
    accountId,
    rows: [
      {
        sourceLine: 1,
        rawRow: ["synthetic host import fixture"],
        excluded: false,
        activity: {
          activityType,
          date: "2026-01-06T00:00:00Z",
          amount: "1",
          currency: "USD",
        },
      },
    ],
  };
  const manifest = {
    id,
    name: label,
    version: "1.0.0",
    main: "dist/addon.js",
    permissions: [
      {
        category: "activities",
        functions: [{ name: "openImport", isDeclared: true, isDetected: false }],
        purpose: "Open the host wizard with one synthetic draft; never write activities",
      },
    ],
    contributes: {
      routes: [{ id: route }],
      links: {
        [tab === "investments" ? "investment-import" : "spending-import"]: [{ route, label }],
      },
    },
  };
  const code = `export async function enable(context) {
    await context.router.add({ id: ${JSON.stringify(route)}, path: ${JSON.stringify(`/addons/${id}`)}, render({ root }) {
      const button = document.createElement("button");
      button.textContent = "Open host import wizard";
      const status = document.createElement("p");
      status.setAttribute("role", "status");
      button.addEventListener("click", async () => {
        try { await context.api.activities.openImport(${JSON.stringify(request)}); }
        catch (error) { status.textContent = String(error); }
      });
      root.replaceChildren(button, status);
    }});
  }`;
  return {
    id,
    route,
    tab,
    label,
    zip: zip({ "manifest.json": JSON.stringify(manifest), "dist/addon.js": code }),
  };
}
