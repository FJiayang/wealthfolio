import { act, fireEvent, render, screen, waitFor } from "@/test/render";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { MarketDataSyncSettings } from "./market-data-sync-settings";

const mocks = vi.hoisted(() => ({
  updateSettings: vi.fn(),
  settings: { marketDataSyncIntervalHours: 6 } as {
    marketDataSyncIntervalHours: number;
    marketDataSyncMode?: "interval" | "daily";
    marketDataSyncTime?: string | null;
    marketDataSyncTimezone?: string | null;
  },
  platform: { isMobile: false, isWeb: false, loading: false },
}));

vi.mock("@/lib/settings-provider", () => ({
  useSettingsContext: () => ({ settings: mocks.settings, updateSettings: mocks.updateSettings }),
}));
vi.mock("@/hooks/use-platform", () => ({ usePlatform: () => mocks.platform }));
vi.mock("../general/timezone-settings", () => ({
  detectBrowserTimezone: () => "Asia/Shanghai",
  getSupportedTimezones: () => ["UTC", "Asia/Shanghai", "America/New_York"],
}));

async function selectMode(name: string) {
  await waitFor(() => expect(screen.queryByRole("listbox")).not.toBeInTheDocument());
  await userEvent.click(screen.getByRole("combobox", { name: "Automatic sync mode" }));
  await userEvent.click(await screen.findByRole("option", { name }));
}

async function selectHours(hours: number) {
  await waitFor(() => expect(screen.queryByRole("listbox")).not.toBeInTheDocument());
  await userEvent.click(screen.getByRole("combobox", { name: "Automatic sync interval" }));
  await userEvent.click(await screen.findByRole("option", { name: `Every ${hours} hours` }));
}

function saveButton() {
  return screen.getByRole("button", { name: "Save" });
}

describe("MarketDataSyncSettings", () => {
  beforeEach(() => {
    mocks.settings = { marketDataSyncIntervalHours: 6 };
    mocks.platform = { isMobile: false, isWeb: false, loading: false };
    mocks.updateSettings.mockReset().mockResolvedValue(undefined);
  });

  it("defaults old settings to interval mode, preserves the allowed hours, and saves explicitly", async () => {
    render(<MarketDataSyncSettings />);
    expect(screen.getByRole("combobox", { name: "Automatic sync mode" })).toHaveTextContent(
      "Hourly interval",
    );
    expect(saveButton()).toBeDisabled();
    await userEvent.click(screen.getByRole("combobox", { name: "Automatic sync interval" }));
    expect(screen.getAllByRole("option").map((option) => option.textContent)).toEqual([
      "Every 1 hour",
      "Every 6 hours",
      "Every 12 hours",
      "Every 24 hours",
    ]);
    await userEvent.click(screen.getByRole("option", { name: "Every 24 hours" }));
    expect(mocks.updateSettings).not.toHaveBeenCalled();
    await userEvent.click(saveButton());
    expect(mocks.updateSettings).toHaveBeenCalledWith({
      marketDataSyncMode: "interval",
      marketDataSyncIntervalHours: 24,
    });
  });

  it("requires a time and atomically saves daily mode with an independent device timezone", async () => {
    render(<MarketDataSyncSettings />);
    await selectMode("Daily at a fixed time");
    expect(
      screen.queryByRole("combobox", { name: "Automatic sync interval" }),
    ).not.toBeInTheDocument();
    expect(screen.getByRole("combobox", { name: "Sync timezone" })).toHaveTextContent(
      "Asia/Shanghai",
    );
    expect(screen.getByLabelText("Daily sync time")).toHaveValue("");
    expect(saveButton()).toBeDisabled();
    fireEvent.change(screen.getByLabelText("Daily sync time"), { target: { value: "17:45" } });
    await userEvent.click(saveButton());
    expect(mocks.updateSettings).toHaveBeenCalledExactlyOnceWith({
      marketDataSyncMode: "daily",
      marketDataSyncIntervalHours: 6,
      marketDataSyncTime: "17:45",
      marketDataSyncTimezone: "Asia/Shanghai",
    });
  });

  it("preserves inactive interval and daily settings while switching modes", async () => {
    mocks.settings = {
      marketDataSyncMode: "daily",
      marketDataSyncIntervalHours: 12,
      marketDataSyncTime: "09:30",
      marketDataSyncTimezone: "America/New_York",
    };
    render(<MarketDataSyncSettings />);
    await selectMode("Hourly interval");
    expect(screen.getByRole("combobox", { name: "Automatic sync interval" })).toHaveTextContent(
      "Every 12 hours",
    );
    await selectHours(24);
    await selectMode("Daily at a fixed time");
    expect(screen.getByLabelText("Daily sync time")).toHaveValue("09:30");
    expect(screen.getByRole("combobox", { name: "Sync timezone" })).toHaveTextContent(
      "America/New_York",
    );
    await selectMode("Hourly interval");
    await userEvent.click(saveButton());
    expect(mocks.updateSettings).toHaveBeenCalledWith({
      marketDataSyncMode: "interval",
      marketDataSyncIntervalHours: 24,
      marketDataSyncTime: "09:30",
      marketDataSyncTimezone: "America/New_York",
    });
  });

  it("keeps daily edits as a draft until both fields are valid", async () => {
    render(<MarketDataSyncSettings />);
    await selectMode("Daily at a fixed time");
    fireEvent.change(screen.getByLabelText("Daily sync time"), { target: { value: "25:00" } });
    expect(saveButton()).toBeDisabled();
    expect(mocks.updateSettings).not.toHaveBeenCalled();
  });

  it("disables controls during saving and retains the draft after a failed save", async () => {
    let rejectSave!: (error: Error) => void;
    mocks.updateSettings.mockReturnValue(
      new Promise((_, reject) => {
        rejectSave = reject;
      }),
    );
    render(<MarketDataSyncSettings />);
    await selectHours(12);
    await userEvent.click(saveButton());
    expect(saveButton()).toBeDisabled();
    expect(screen.getByRole("combobox", { name: "Automatic sync mode" })).toBeDisabled();
    await act(async () => {
      rejectSave(new Error("Save failed"));
      await Promise.resolve();
    });
    await waitFor(() => expect(saveButton()).not.toBeDisabled());
    expect(screen.getByRole("combobox", { name: "Automatic sync interval" })).toHaveTextContent(
      "Every 12 hours",
    );
  });

  it("hides native mobile controls but shows them for a mobile web client", () => {
    mocks.platform.isMobile = true;
    const view = render(<MarketDataSyncSettings />);
    expect(screen.queryByRole("combobox")).not.toBeInTheDocument();
    mocks.platform.isWeb = true;
    view.rerender(<MarketDataSyncSettings />);
    expect(screen.getByRole("combobox", { name: "Automatic sync mode" })).toBeInTheDocument();
  });

  it("keeps daily time legible in RTL without physical layout classes", async () => {
    render(
      <div dir="rtl">
        <MarketDataSyncSettings />
      </div>,
    );
    await selectMode("Daily at a fixed time");
    expect(screen.getByLabelText("Daily sync time")).toHaveAttribute("dir", "ltr");
    expect(screen.getByRole("combobox", { name: "Sync timezone" })).toBeVisible();
  });
});
