import { act, render, screen, waitFor } from "@/test/render";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { MarketDataSyncSettings } from "./market-data-sync-settings";

const mocks = vi.hoisted(() => ({
  updateSettings: vi.fn(),
  hours: 6,
  platform: { isMobile: false, isWeb: false, loading: false },
}));

vi.mock("@/lib/settings-provider", () => ({
  useSettingsContext: () => ({
    settings: { marketDataSyncIntervalHours: mocks.hours },
    updateSettings: mocks.updateSettings,
  }),
}));

vi.mock("@/hooks/use-platform", () => ({ usePlatform: () => mocks.platform }));

async function selectHours(hours: number) {
  const user = userEvent.setup();
  await user.click(screen.getByRole("combobox", { name: "Automatic sync interval" }));
  await user.click(screen.getByRole("option", { name: `Every ${hours} hours` }));
}

describe("MarketDataSyncSettings", () => {
  beforeEach(() => {
    mocks.hours = 6;
    mocks.platform = { isMobile: false, isWeb: false, loading: false };
    mocks.updateSettings.mockReset().mockResolvedValue(undefined);
  });

  it("shows all four intervals and saves the selected interval", async () => {
    const view = render(<MarketDataSyncSettings />);
    expect(screen.getByRole("combobox")).toHaveTextContent("Every 6 hours");
    await userEvent.click(screen.getByRole("combobox"));
    expect(screen.getAllByRole("option").map((option) => option.textContent)).toEqual([
      "Every 1 hour",
      "Every 6 hours",
      "Every 12 hours",
      "Every 24 hours",
    ]);
    await userEvent.click(screen.getByRole("option", { name: "Every 24 hours" }));
    expect(mocks.updateSettings).toHaveBeenCalledWith({ marketDataSyncIntervalHours: 24 });
    mocks.hours = 24;
    view.rerender(<MarketDataSyncSettings />);
    expect(screen.getByRole("combobox")).toHaveTextContent("Every 24 hours");
  });

  it("disables saving and keeps the stored value when saving fails", async () => {
    let rejectSave!: (error: Error) => void;
    mocks.updateSettings.mockReturnValue(
      new Promise((_, reject) => {
        rejectSave = reject;
      }),
    );
    render(<MarketDataSyncSettings />);
    await selectHours(12);
    expect(screen.getByRole("combobox")).toBeDisabled();
    await act(async () => {
      rejectSave(new Error("Save failed"));
      await Promise.resolve();
    });
    await waitFor(() => expect(screen.getByRole("combobox")).not.toBeDisabled());
    expect(screen.getByRole("combobox")).toHaveTextContent("Every 6 hours");
  });

  it("hides native mobile controls but shows them for a mobile web client", () => {
    mocks.platform.isMobile = true;
    const view = render(<MarketDataSyncSettings />);
    expect(screen.queryByRole("combobox")).not.toBeInTheDocument();
    mocks.platform.isWeb = true;
    view.rerender(<MarketDataSyncSettings />);
    expect(screen.getByRole("combobox")).toBeInTheDocument();
  });
});
