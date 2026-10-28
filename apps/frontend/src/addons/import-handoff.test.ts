// @vitest-environment node
import { afterEach, describe, expect, it, vi } from "vitest";
import type { OpenActivityImportRequest } from "@wealthfolio/addon-sdk";
import { prepareImportHandoff, getImportHandoff, clearImportHandoff } from "./import-handoff";
import { createPermissionGuard, createSDKHostAPIBridge, type InternalHostAPI } from "./type-bridge";

function request(): OpenActivityImportRequest {
  return {
    sourceLabel: "synthetic.pdf",
    accountId: "account",
    rows: [
      {
        sourceLine: 42,
        rawRow: ["original"],
        excluded: false,
        activity: {
          activityType: "BUY",
          accountId: "untrusted",
          amount: "9007199254740993.12",
          symbol: "000032",
          quantity: "100",
          isDraft: false,
          isValid: false,
          forceImport: true,
          duplicateOfId: "untrusted",
          errors: { amount: ["untrusted"] },
        },
      },
    ],
  };
}
afterEach(() => {
  clearImportHandoff();
  vi.unstubAllGlobals();
});
describe("addon draft handoff", () => {
  it("preserves precision and provenance, and derives clean host draft state", () => {
    prepareImportHandoff("addon", request(), "/addons/addon");
    expect(getImportHandoff()?.drafts[0]).toMatchObject({
      rowIndex: 0,
      sourceLine: 42,
      rawRow: ["original"],
      accountId: "account",
      amount: "9007199254740993.12",
      status: "valid",
      errors: {},
      warnings: {},
      forceImport: false,
    });
    expect(getImportHandoff()?.drafts[0].duplicateOfId).toBeUndefined();
  });
  it("retains split source rows and maps excluded rows to skipped", () => {
    const input = request();
    input.rows.push({ ...input.rows[0], excluded: true });
    prepareImportHandoff("addon", input, "/addons/addon");
    expect(
      getImportHandoff()?.drafts.map((draft) => [draft.rowIndex, draft.sourceLine, draft.status]),
    ).toEqual([
      [0, 42, "valid"],
      [1, 42, "skipped"],
    ]);
  });
  it("rejects malformed input without replacing the current handoff", () => {
    prepareImportHandoff("addon", request(), "/addons/addon");
    expect(() =>
      prepareImportHandoff("other", { ...request(), rows: [] }, "/addons/other"),
    ).toThrow();
    expect(getImportHandoff()?.addonId).toBe("addon");
    clearImportHandoff("other");
    expect(getImportHandoff()).toBeDefined();
    clearImportHandoff("addon");
    expect(getImportHandoff()).toBeUndefined();
  });
  it("denies missing permissions before opening a host wizard", async () => {
    const navigateToRoute = vi.fn();
    const guard = createPermissionGuard("addon", []);
    const api = createSDKHostAPIBridge(
      { navigateToRoute } as unknown as InternalHostAPI,
      "addon",
      guard,
    );
    expect(() => api.activities.openImport(request())).toThrow("not allowed");
    expect(navigateToRoute).not.toHaveBeenCalled();
    expect(getImportHandoff()).toBeUndefined();
  });
  it("opens the host wizard without calling any write or market API", async () => {
    vi.stubGlobal("window", { location: { pathname: "/addons/addon", search: "" } });
    const navigateToRoute = vi.fn(async () => {});
    const importActivities = vi.fn();
    const checkActivitiesImport = vi.fn();
    const api = createSDKHostAPIBridge(
      { navigateToRoute, importActivities, checkActivitiesImport } as unknown as InternalHostAPI,
      "addon",
    );
    await api.activities.openImport(request());
    expect(navigateToRoute).toHaveBeenCalledWith("/import?addonImport=1");
    expect(importActivities).not.toHaveBeenCalled();
    expect(checkActivitiesImport).not.toHaveBeenCalled();
  });
});
