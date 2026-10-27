import { render, screen, within, waitFor, fireEvent } from "@/test/render";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import userEvent from "@testing-library/user-event";
import { MemoryRouter } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ProviderRequestEvent, ProviderRequestPage } from "@/adapters/shared/market-data";
import { ProviderRequestsPage } from "./provider-requests-page";

const mocks = vi.hoisted(() => ({
  list: vi.fn(),
  updateSettings: vi.fn(),
  profile: "one",
  timezone: "Asia/Shanghai",
}));
vi.mock("@/adapters", async (original) => ({
  ...(await original<typeof import("@/adapters")>()),
  listProviderRequests: mocks.list,
}));
vi.mock("@/features/profiles/session", () => ({ selectedProfileId: () => mocks.profile }));
vi.mock("@/lib/settings-provider", () => ({
  useSettingsContext: () => ({
    settings: { providerRequestTimezone: mocks.timezone },
    updateSettings: mocks.updateSettings,
  }),
}));
vi.mock("../general/timezone-input", () => ({
  TimezoneInput: ({
    value,
    onChange,
    ariaLabel,
  }: {
    value: string;
    onChange: (value: string) => void;
    ariaLabel: string;
  }) => (
    <select aria-label={ariaLabel} value={value} onChange={(event) => onChange(event.target.value)}>
      <option>Asia/Shanghai</option>
      <option>America/New_York</option>
    </select>
  ),
}));
vi.mock("./use-market-data-settings", () => ({
  useMarketDataProviderSettings: () => ({ data: [{ id: "YAHOO", name: "Yahoo Finance" }] }),
}));
vi.mock("@/hooks/use-custom-providers", () => ({ useCustomProviders: () => ({ data: [] }) }));

const event: ProviderRequestEvent = {
  id: 101,
  providerId: "YAHOO",
  occurredAt: "2026-10-27T04:00:00Z",
  context: {
    recordKind: "http",
    operation: "chart_history",
    symbols: ["AAPL", "MSFT"],
    parameters: { symbols: ["AAPL", "MSFT"], interval: "1d", from: "2026-10-01" },
  },
  result: {
    durationMs: 123,
    outcome: "failure",
    httpStatus: 429,
    errorCategory: "http_status",
    errorMessage: "HTTP non-success status",
  },
};
function page(rows = [event], nextCursor: string | null = null): ProviderRequestPage {
  return {
    rows,
    nextCursor,
    availableSince: "2026-09-27T04:00:00Z",
    diagnosticsSince: "2026-10-27T00:00:00Z",
  };
}
function setup(search = "?providerId=YAHOO&from=2026-10-27&to=2026-10-27") {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false, gcTime: 0 } } });
  return render(
    <QueryClientProvider client={client}>
      <MemoryRouter initialEntries={[`/settings/market-data/requests${search}`]}>
        <ProviderRequestsPage />
      </MemoryRouter>
    </QueryClientProvider>,
  );
}
beforeEach(() => {
  mocks.list.mockReset().mockResolvedValue(page());
  mocks.profile = "one";
  mocks.timezone = "Asia/Shanghai";
  mocks.updateSettings.mockReset().mockResolvedValue(undefined);
});

describe("provider request records", () => {
  it("preserves timezone saving and reports a failed save", async () => {
    mocks.updateSettings.mockRejectedValueOnce(new Error("unavailable"));
    setup();
    await userEvent
      .setup()
      .selectOptions(screen.getByLabelText("Statistics timezone"), "America/New_York");
    await userEvent.setup().click(screen.getByRole("button", { name: "Save timezone" }));
    expect(mocks.updateSettings).toHaveBeenCalledWith({
      providerRequestTimezone: "America/New_York",
    });
    expect(await screen.findByRole("alert")).toHaveTextContent(/save/i);
    await userEvent.setup().click(screen.getByRole("button", { name: "Save timezone" }));
    await waitFor(() => expect(screen.queryByRole("alert")).not.toBeInTheDocument());
  });
  it("shows individual fields and complete parameters in the per-row detail sheet", async () => {
    setup();
    expect(await screen.findByText("123 ms")).toBeInTheDocument();
    expect(screen.getByText("AAPL, MSFT")).toBeInTheDocument();
    expect(screen.getByText("429")).toBeInTheDocument();
    expect(screen.getByText("HTTP non-success status")).toBeInTheDocument();
    expect(mocks.list).toHaveBeenCalledWith(
      expect.objectContaining({
        providerId: "YAHOO",
        start: "2026-10-26T16:00:00.000Z",
        end: "2026-10-27T16:00:00.000Z",
        limit: 50,
      }),
    );
    await userEvent.setup().click(screen.getByRole("button", { name: "View details #101" }));
    const sheet = screen.getByRole("dialog", { name: "Request details" });
    expect(within(sheet).getByText("http_status")).toBeInTheDocument();
    expect(within(sheet).getByText(/"interval": "1d"/)).toBeInTheDocument();
    expect(within(sheet).getByText(/"from": "2026-10-01"/)).toBeInTheDocument();
    expect(within(sheet).getByText("2026-10-27T04:00:00Z")).toBeInTheDocument();
  });
  it("uses the cursor for subsequent pages and resets pagination when filters change", async () => {
    mocks.list
      .mockResolvedValueOnce(page([event], "cursor-one"))
      .mockResolvedValueOnce(page([{ ...event, id: 102 }]))
      .mockResolvedValue(page([]));
    setup();
    await userEvent.setup().click(await screen.findByRole("button", { name: "Load more" }));
    await waitFor(() =>
      expect(mocks.list).toHaveBeenLastCalledWith(
        expect.objectContaining({ cursor: "cursor-one" }),
      ),
    );
    await screen.findByRole("button", { name: "View details #102" });
    fireEvent.change(screen.getByLabelText("Security code (exact)"), { target: { value: "XYZ" } });
    await waitFor(() =>
      expect(mocks.list).toHaveBeenLastCalledWith(
        expect.objectContaining({ symbol: "XYZ", cursor: undefined }),
      ),
    );
    expect(await screen.findByText("No matching requests.")).toBeInTheDocument();
  });
  it("distinguishes historical records from an unfinished SDK call", async () => {
    mocks.list.mockResolvedValue(
      page([
        { ...event, context: null, result: null },
        { ...event, id: 102, context: { ...event.context!, recordKind: "sdk" }, result: null },
      ]),
    );
    setup();
    await userEvent.setup().click(await screen.findByRole("button", { name: "View details #101" }));
    expect(
      screen.getAllByText("Historical record: detailed fields were not captured.").length,
    ).toBeGreaterThan(0);
    expect(
      screen.getByText("Outcome unavailable: the call may be pending or interrupted."),
    ).toBeInTheDocument();
    expect(screen.queryByText("0 ms")).not.toBeInTheDocument();
    await userEvent.setup().click(screen.getByRole("button", { name: "Close" }));
    await userEvent.setup().click(screen.getByRole("button", { name: "View details #102" }));
    expect(within(screen.getByRole("dialog")).getByText("SDK")).toBeInTheDocument();
    expect(
      within(screen.getByRole("dialog")).queryByText(
        "Historical record: detailed fields were not captured.",
      ),
    ).not.toBeInTheDocument();
  });
  it("does not fetch malformed date filters", () => {
    setup("?from=invalid&to=2026-10-27");
    expect(screen.getByRole("alert")).toHaveTextContent("Select valid dates");
    expect(mocks.list).not.toHaveBeenCalled();
  });
  it("shows a query error without pretending the result is empty", async () => {
    mocks.list.mockRejectedValue(new Error("unavailable"));
    setup();
    expect(await screen.findByRole("alert")).toHaveTextContent("Unable to load requests.");
    expect(screen.queryByText("No matching requests.")).not.toBeInTheDocument();
  });
});
