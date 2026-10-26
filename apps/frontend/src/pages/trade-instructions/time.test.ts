import { describe, expect, it } from "vitest";
import { inputTime, instantTime } from "./time";
import { applyScopeDependencies } from "@/pages/settings/agent-access/scopes";
describe("trade instruction times and permissions", () => {
  it("round-trips wall time in the application timezone, independently of the browser", () => {
    expect(instantTime("2026-10-26T10:30", "Asia/Shanghai")).toBe("2026-10-26T02:30:00.000Z");
    expect(inputTime("2026-10-26T02:30:00Z", "Asia/Shanghai")).toBe("2026-10-26T10:30");
    expect(() => instantTime("bad", "Asia/Shanghai")).toThrow();
  });
  it("grants the prerequisites without bookkeeping or quote writes", () => {
    const scopes = applyScopeDependencies(["trade-instructions:create"]);
    expect(scopes).toEqual(
      expect.arrayContaining([
        "trade-instructions:read",
        "trade-instructions:create",
        "accounts:read",
        "financial-planning:read",
        "assets:write",
      ]),
    );
    expect(scopes).not.toContain("activities:write");
    expect(scopes).not.toContain("market-data:write");
  });
});
