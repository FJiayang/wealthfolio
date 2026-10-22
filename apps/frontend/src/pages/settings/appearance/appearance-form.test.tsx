import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, expect, it, vi } from "vitest";
import { AppearanceForm } from "./appearance-form";

const updateSettings = vi.hoisted(() => vi.fn(() => Promise.resolve()));
const platform = vi.hoisted(() => ({ isMobile: false, isMacOS: false, isTauri: true }));
vi.mock("@/hooks/use-platform", () => ({ usePlatform: () => platform }));
vi.mock("@/lib/settings-provider", () => ({
  useSettingsContext: () => ({
    settings: { theme: "light", font: "font-mono", menuBarVisible: true },
    updateSettings,
  }),
}));
vi.mock("@/pages/layouts/navigation/navigation-mode-context", () => ({
  useNavigationMode: () => ({ mode: "sidebar", setMode: vi.fn() }),
}));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));

beforeEach(() => {
  updateSettings.mockReset().mockResolvedValue(undefined);
  platform.isMobile = false;
  platform.isMacOS = false;
  platform.isTauri = true;
});

it("offers the menu bar setting in the Windows and Linux app", () => {
  render(<AppearanceForm />);
  expect(screen.getByText("settings:appearance_menu_bar")).toBeInTheDocument();
});

it("hides the menu bar setting in the macOS app, which always keeps its menu", () => {
  platform.isMacOS = true;
  render(<AppearanceForm />);
  expect(screen.queryByText("settings:appearance_menu_bar")).not.toBeInTheDocument();
});

it("leaves the web app's menu bar setting alone on macOS", () => {
  platform.isMacOS = true;
  platform.isTauri = false;
  render(<AppearanceForm />);
  expect(screen.getByText("settings:appearance_menu_bar")).toBeInTheDocument();
});

it("defaults to green up and saves only the selected color scheme", async () => {
  render(<AppearanceForm />);
  const select = screen.getByTestId("gain-loss-color-select");
  expect(select).toHaveTextContent("settings:appearance_gain_loss_green_up");
  await userEvent.click(select);
  await userEvent.click(
    await screen.findByRole("option", { name: "settings:appearance_gain_loss_red_up" }),
  );
  await waitFor(() =>
    expect(updateSettings).toHaveBeenCalledWith({ gainLossColorScheme: "red-up-green-down" }),
  );
  expect(select).toHaveTextContent("settings:appearance_gain_loss_red_up");
});

it("restores the saved color scheme if saving fails", async () => {
  updateSettings.mockRejectedValueOnce(new Error("save failed"));
  render(<AppearanceForm />);
  const select = screen.getByTestId("gain-loss-color-select");
  await userEvent.click(select);
  await userEvent.click(
    await screen.findByRole("option", { name: "settings:appearance_gain_loss_red_up" }),
  );
  await waitFor(() => expect(updateSettings).toHaveBeenCalledOnce());
  await waitFor(() => expect(select).toHaveTextContent("settings:appearance_gain_loss_green_up"));
  expect(select).not.toBeDisabled();
});
