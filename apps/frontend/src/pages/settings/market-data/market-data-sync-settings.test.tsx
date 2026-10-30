import { act, render, screen, waitFor } from "@/test/render";
import userEvent from "@testing-library/user-event";
import { beforeEach, afterEach, describe, expect, it, vi } from "vitest";
import { MarketDataSyncSettings } from "./market-data-sync-settings";

const mocks = vi.hoisted(() => ({
  updateSettings: vi.fn(),
  time: "22:00",
  timezone: "Asia/Shanghai",
}));
vi.mock("@/lib/settings-provider", () => ({
  useSettingsContext: () => ({
    settings: { marketDataSyncTime: mocks.time, marketDataSyncTimezone: mocks.timezone },
    updateSettings: mocks.updateSettings,
  }),
}));

// Exercise real timezone controls and saved fields with a small scenario catalog.
// Full runtime catalog normalization is covered by timezone-settings.test.ts.
vi.mock("../general/timezone-settings", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../general/timezone-settings")>()),
  getSupportedTimezones: () => ["Asia/Shanghai", "Asia/Tokyo", "UTC"],
}));

describe("MarketDataSyncSettings", () => {
  afterEach(() => vi.unstubAllGlobals());
  beforeEach(() => {
    Element.prototype.scrollIntoView = vi.fn();
    vi.stubGlobal(
      "ResizeObserver",
      class {
        observe() {}
        unobserve() {}
        disconnect() {}
      },
    );
    mocks.time = "22:00";
    mocks.updateSettings.mockReset().mockResolvedValue(undefined);
  });
  it("saves hours and minutes independently, preserving the other part", async () => {
    const user = userEvent.setup();
    const view = render(<MarketDataSyncSettings />);
    expect(screen.getByRole("combobox", { name: "Hour" })).toHaveTextContent("22");
    await user.click(screen.getByRole("combobox", { name: "Hour" }));
    expect(await screen.findAllByRole("option")).toHaveLength(24);
    await user.click(screen.getByRole("option", { name: "21" }));
    expect(mocks.updateSettings).toHaveBeenCalledWith({ marketDataSyncTime: "21:00" });
    mocks.time = "21:00";
    view.rerender(<MarketDataSyncSettings />);
    await user.click(screen.getByRole("combobox", { name: "Minute" }));
    expect(await screen.findAllByRole("option")).toHaveLength(60);
    await user.click(screen.getByRole("option", { name: "35" }));
    expect(mocks.updateSettings).toHaveBeenCalledWith({ marketDataSyncTime: "21:35" });
  });
  it("keeps the stored time after a save failure", async () => {
    let reject!: (error: Error) => void;
    mocks.updateSettings.mockReturnValue(
      new Promise((_, fail) => {
        reject = fail;
      }),
    );
    render(<MarketDataSyncSettings />);
    await userEvent.click(screen.getByRole("combobox", { name: "Hour" }));
    await userEvent.click(await screen.findByRole("option", { name: "20" }));
    await waitFor(() => expect(screen.getByRole("combobox", { name: "Hour" })).toBeDisabled());
    await act(async () => reject(new Error("Write failed")));
    await waitFor(() => expect(screen.getByRole("combobox", { name: "Hour" })).not.toBeDisabled());
    expect(screen.getByRole("combobox", { name: "Hour" })).toHaveTextContent("22");
  });
  it("saves a timezone without changing the scheduled time", async () => {
    render(<MarketDataSyncSettings />);
    await userEvent.click(screen.getByRole("combobox", { name: "Timezone" }));
    await userEvent.click(screen.getByRole("option", { name: "Asia/Tokyo" }));
    expect(mocks.updateSettings).toHaveBeenCalledWith({ marketDataSyncTimezone: "Asia/Tokyo" });
  });
});
