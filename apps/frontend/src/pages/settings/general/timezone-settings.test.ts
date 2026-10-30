import { describe, expect, it } from "vitest";
import { getSupportedTimezones, resolveInitialTimezone } from "./timezone-settings";

describe("resolveInitialTimezone", () => {
  it("uses configured timezone when present", () => {
    expect(resolveInitialTimezone("Europe/Paris")).toBe("Europe/Paris");
  });

  it("returns empty string when configured timezone is missing", () => {
    expect(resolveInitialTimezone("")).toBe("");
    expect(resolveInitialTimezone("   ")).toBe("");
    expect(resolveInitialTimezone(undefined)).toBe("");
  });

  it("keeps UTC when explicitly configured", () => {
    expect(resolveInitialTimezone("UTC")).toBe("UTC");
  });
});

describe("getSupportedTimezones", () => {
  it("preserves the complete real Intl catalog while adding UTC, deduplicating and sorting", () => {
    // This is a catalog logic check, not full-directory DOM search coverage.
    const runtime = Intl.supportedValuesOf("timeZone");
    expect(runtime.length).toBeGreaterThan(0);
    const actual = getSupportedTimezones();
    expect(actual).toEqual(
      Array.from(new Set(["UTC", ...runtime])).sort((a, b) => a.localeCompare(b)),
    );
    expect(actual).toContain("UTC");
    for (const timezone of runtime) expect(actual).toContain(timezone);
  });
});
