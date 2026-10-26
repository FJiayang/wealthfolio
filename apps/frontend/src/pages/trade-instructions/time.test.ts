import { CalendarDateTime } from "@internationalized/date";
import { describe, expect, it } from "vitest";
import { inputTime, instantTime, pickerTime } from "./time";
import { applyScopeDependencies } from "@/pages/settings/agent-access/scopes";
describe("trade instruction times and permissions", () => {
  it("round-trips wall time in the application timezone, independently of the browser", () => {
    expect(instantTime("2026-10-26T10:30", "Asia/Shanghai")).toBe("2026-10-26T02:30:00.000Z");
    expect(inputTime("2026-10-26T02:30:00Z", "Asia/Shanghai")).toBe("2026-10-26T10:30");
    expect(() => instantTime("bad", "Asia/Shanghai")).toThrow();
  });
  it("keeps picker wall time when converting to the application timezone and supports clearing", () => {
    const selected = new CalendarDateTime(2026, 10, 26, 10, 30).toDate("Asia/Shanghai");
    expect(pickerTime(selected, "Asia/Shanghai")).toBe("2026-10-26T10:30");
    expect(instantTime(pickerTime(selected, "Asia/Shanghai"), "Asia/Shanghai")).toBe(
      "2026-10-26T02:30:00.000Z",
    );
    expect(pickerTime(undefined, "Asia/Shanghai")).toBe("");
    // This wall time does not exist in New York, but is valid in the application's timezone.
    const dstDay = new CalendarDateTime(2026, 3, 8, 2, 30).toDate("Asia/Shanghai");
    expect(instantTime(pickerTime(dstDay, "Asia/Shanghai"), "Asia/Shanghai")).toBe(
      "2026-03-07T18:30:00.000Z",
    );
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
