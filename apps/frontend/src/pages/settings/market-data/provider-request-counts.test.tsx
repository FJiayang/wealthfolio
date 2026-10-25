import { fireEvent, render, screen, waitFor } from "@/test/render";
import { describe, expect, it, vi, beforeEach } from "vitest";
import { FormattingProvider } from "@wealthfolio/ui";
import { createInstance } from "i18next";
import { I18nextProvider } from "react-i18next";
import enSettings from "@/i18n/locales/en/settings.json";
import zhSettings from "@/i18n/locales/zh/settings.json";
import zhHantSettings from "@/i18n/locales/zh-Hant/settings.json";
import { ProviderRequestCountsPanel, ProviderTodayRequests } from "./provider-request-counts";
import type { useProviderRequestCounts } from "./use-provider-request-counts";

const mocks = vi.hoisted(() => ({ update: vi.fn(), timezone: "Asia/Shanghai" }));
vi.mock("@/lib/settings-provider", () => ({
  useSettingsContext: () => ({
    settings: { providerRequestTimezone: mocks.timezone },
    updateSettings: mocks.update,
  }),
}));
vi.mock("../general/timezone-input", () => ({
  TimezoneInput: ({ value, onChange }: { value: string; onChange: (value: string) => void }) => (
    <select
      aria-label="Statistics timezone"
      value={value}
      onChange={(event) => onChange(event.target.value)}
    >
      <option>Asia/Shanghai</option>
      <option>UTC</option>
    </select>
  ),
}));
const data = {
  timezone: "Asia/Shanghai",
  trackingSince: "2026-10-22T17:00:00Z",
  asOf: "2026-10-24T04:00:00Z",
  startDate: "2026-10-22",
  endDate: "2026-10-24",
  rows: [{ providerId: "YAHOO", day: "2026-10-24", requestCount: 1234 }],
};
function panel(overrides = {}) {
  const query = {
    data,
    isError: false,
    isFetching: false,
    refetch: vi.fn(),
    ...overrides,
  } as unknown as ReturnType<typeof useProviderRequestCounts>;
  render(
    <ProviderRequestCountsPanel
      query={query}
      days={30}
      onDaysChange={vi.fn()}
      providers={[{ id: "YAHOO", name: "Yahoo" }]}
    />,
  );
  return query;
}
describe("provider request counts panel", () => {
  beforeEach(() => {
    mocks.timezone = "Asia/Shanghai";
    mocks.update.mockReset().mockResolvedValue(undefined);
  });
  it("shows unknown days, partial first day and localized counts", () => {
    panel();
    expect(screen.getByText("Not tracked")).toBeInTheDocument();
    expect(screen.getAllByText("Partial day")).toHaveLength(2);
    expect(screen.getByText("1,234")).toBeInTheDocument();
    expect(screen.getByText("0")).toBeInTheDocument();
  });
  it("saves only the independent timezone field", async () => {
    panel();
    fireEvent.change(screen.getByRole("combobox", { name: "Statistics timezone" }), {
      target: { value: "UTC" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Save timezone" }));
    await waitFor(() =>
      expect(mocks.update).toHaveBeenCalledWith({ providerRequestTimezone: "UTC" }),
    );
  });
  it("does not present an old timezone's aggregates as new results", () => {
    mocks.timezone = "UTC";
    panel();
    expect(screen.queryByText("1,234")).not.toBeInTheDocument();
    expect(screen.getByRole("status")).toHaveTextContent("Loading request counts");
  });
  it("keeps refresh available on errors and does not display zeros", () => {
    const query = panel({ data: undefined, isError: true });
    expect(screen.getByRole("alert")).toHaveTextContent("unavailable");
    expect(screen.queryByText("0")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Refresh" }));
    expect(query.refetch).toHaveBeenCalled();
  });
  it("labels retained data as stale after a refresh failure", () => {
    panel({ isError: true });
    expect(screen.getByRole("alert")).toHaveTextContent("out of date");
  });
  it.each([
    ["en", enSettings, "Today’s request count: 1,234"],
    ["zh", zhSettings, "今日调用次数：1,234"],
    ["zh-Hant", zhHantSettings, "今日呼叫次數：1,234"],
  ])("explicitly labels today's request count in %s", async (locale, settings, expected) => {
    const i18n = createInstance();
    await i18n.init({ lng: locale, resources: { [locale]: { settings } } });
    render(
      <I18nextProvider i18n={i18n}>
        <ProviderTodayRequests count={1234} />
      </I18nextProvider>,
    );
    expect(screen.getByText(expected)).toBeInTheDocument();
  });
  it("formats today counts and never substitutes zero for unavailable", () => {
    const { rerender } = render(
      <FormattingProvider locale="ar-EG">
        <ProviderTodayRequests count={1234} />
      </FormattingProvider>,
    );
    expect(screen.getByText(/١٬٢٣٤/)).toBeInTheDocument();
    rerender(<ProviderTodayRequests count={undefined} />);
    expect(screen.getByText(/—/)).toBeInTheDocument();
  });
});
