import { describe, expect, it } from "vitest";
import { providerRequestDateBounds } from "./provider-request-dates";

describe("provider request calendar filters", () => {
  it("uses the statistics timezone for the inclusive calendar dates", () => {
    expect(providerRequestDateBounds("2026-10-27", "2026-10-27", "Asia/Shanghai")).toEqual({
      start: "2026-10-26T16:00:00.000Z",
      end: "2026-10-27T16:00:00.000Z",
    });
  });
  it.each([
    ["2026-03-08", 23],
    ["2026-11-01", 25],
  ])("preserves DST boundaries on %s", (day, hours) => {
    const range = providerRequestDateBounds(day as string, day as string, "America/New_York")!;
    expect((Date.parse(range.end) - Date.parse(range.start)) / 3_600_000).toBe(hours);
  });
  it.each([
    ["", "2026-10-27"],
    ["bad", "2026-10-27"],
    ["2026-02-30", "2026-10-27"],
    ["2026-10-28", "2026-10-27"],
  ])("rejects invalid input %s to %s", (from, to) => {
    expect(providerRequestDateBounds(from, to, "Asia/Shanghai")).toBeNull();
  });
});
