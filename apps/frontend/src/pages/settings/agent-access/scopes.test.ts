import { describe, expect, it } from "vitest";
import { applyScopeDependencies, matchPreset, SCOPES, READ_SCOPES, SCOPE_PRESETS } from "./scopes";

describe("Spending permission composition", () => {
  it("keeps fork permissions explicit and scope metadata unique", () => {
    expect(new Set(SCOPES.map((scope) => scope.key)).size).toBe(SCOPES.length);
    for (const key of [
      "accounts:write",
      "assets:write",
      "holdings:write",
      "financial-planning:write",
      "market-data:read",
      "trade-instructions:read",
      "trade-instructions:write",
    ] as const) {
      expect(SCOPES.some((scope) => scope.key === key)).toBe(true);
    }
    expect(applyScopeDependencies(["accounts:read", "market-data:read"])).not.toContain(
      "spending:read",
    );
    expect(applyScopeDependencies(["spending:read", "spending:write"])).not.toContain(
      "budget:write",
    );
  });
  it("keeps organization and budgets independent", () => {
    expect(applyScopeDependencies(["spending:write"])).toEqual(["spending:read", "spending:write"]);
    expect(applyScopeDependencies(["budget:write"])).toEqual(["spending:read", "budget:write"]);
    expect(applyScopeDependencies(["spending:write", "budget:write"])).toEqual([
      "spending:read",
      "spending:write",
      "budget:write",
    ]);
    expect(applyScopeDependencies(["classification:write"])).toEqual([
      "classification:suggest",
      "classification:write",
    ]);
  });
  it("does not interpret legacy read tokens as broader new presets", () => {
    const legacy = READ_SCOPES.filter((scope) => scope !== "spending:read");
    expect(matchPreset(legacy)).toBeUndefined();
    expect(legacy).not.toContain("spending:read");
    expect(matchPreset(READ_SCOPES)?.key).toBe("read-only");
  });
  it("keeps every write grant explicit in presets", () => {
    for (const preset of SCOPE_PRESETS) {
      expect(applyScopeDependencies(preset.scopes)).toEqual(
        applyScopeDependencies(new Set(preset.scopes)),
      );
      if (!preset.key.startsWith("spending-")) {
        expect(preset.scopes).not.toContain("spending:write");
        expect(preset.scopes).not.toContain("budget:write");
      }
    }
    expect(matchPreset(["spending:read"])?.key).toBe("spending-view");
    expect(matchPreset(["spending:read", "budget:write"])?.key).toBe("spending-budgets");
  });
});
