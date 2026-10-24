import { act, renderHook, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ProviderRequestCounts } from "@/adapters/shared/market-data";
import {
  requestCountDays,
  requestTrackingDay,
  todayRequestCount,
  useProviderRequestCounts,
} from "./use-provider-request-counts";

const mocks = vi.hoisted(() => ({ get: vi.fn(), profile: "profile-a" }));
vi.mock("@/adapters", () => ({ getProviderRequestCounts: mocks.get }));
vi.mock("@/features/profiles/session", () => ({ selectedProfileId: () => mocks.profile }));

export const counts: ProviderRequestCounts = {
  timezone: "Asia/Shanghai",
  trackingSince: "2026-10-22T17:00:00Z",
  asOf: "2026-10-24T04:00:00Z",
  startDate: "2026-10-22",
  endDate: "2026-10-24",
  rows: [{ providerId: "YAHOO", day: "2026-10-24", requestCount: 12 }],
};

describe("provider request calendar rows", () => {
  it("never zero fills pretracking days, marks first day partial and fills tracked sparse days", () => {
    expect(requestCountDays(counts, "YAHOO")).toEqual([
      { day: "2026-10-24", count: 12, partial: true },
      { day: "2026-10-23", count: 0, partial: true },
      { day: "2026-10-22", count: null, partial: false },
    ]);
    expect(todayRequestCount(undefined, "YAHOO")).toBeUndefined();
    expect(todayRequestCount(counts, "CUSTOM_SCRAPER:test")).toBe(0);
  });
  it("derives the start day in the returned timezone, not the browser timezone", () => {
    expect(requestTrackingDay(counts)).toBe("2026-10-23");
    expect(requestTrackingDay({ ...counts, timezone: "UTC" })).toBe("2026-10-22");
  });
  it("uses calendar days across DST and isolates custom-provider identities", () => {
    const data = {
      ...counts,
      timezone: "America/New_York",
      trackingSince: "2026-10-01T00:00:00Z",
      startDate: "2026-10-31",
      endDate: "2026-11-02",
      rows: [{ providerId: "CUSTOM_SCRAPER:one", day: "2026-11-01", requestCount: 2 }],
    };
    expect(requestCountDays(data, "CUSTOM_SCRAPER:one").map((row) => [row.day, row.count])).toEqual(
      [
        ["2026-11-02", 0],
        ["2026-11-01", 2],
        ["2026-10-31", 0],
      ],
    );
    expect(requestCountDays(data, "CUSTOM_SCRAPER:two").every((row) => row.count === 0)).toBe(true);
  });
});

describe("provider request query", () => {
  beforeEach(() => {
    mocks.get.mockReset();
    mocks.profile = "profile-a";
  });
  function setup() {
    const client = new QueryClient({ defaultOptions: { queries: { retry: false, gcTime: 0 } } });
    const wrapper = ({ children }: { children: ReactNode }) => (
      <QueryClientProvider client={client}>{children}</QueryClientProvider>
    );
    return { client, wrapper };
  }
  it("queries relative days again on refresh so midnight is recalculated by the server", async () => {
    mocks.get.mockResolvedValueOnce(counts).mockResolvedValue({ ...counts, endDate: "2026-10-25" });
    const { wrapper, client } = setup();
    const { result, rerender, unmount } = renderHook(({ days }) => useProviderRequestCounts(days), {
      wrapper,
      initialProps: { days: 30 },
    });
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    await waitFor(() => expect(result.current.isFetching).toBe(false));
    mocks.get.mockResolvedValue({ ...counts, endDate: "2026-10-25" });
    await act(async () => {
      await result.current.refetch();
    });
    await waitFor(() => expect(result.current.data?.endDate).toBe("2026-10-25"));
    expect(mocks.get).toHaveBeenLastCalledWith(30);
    rerender({ days: 7 });
    await waitFor(() => expect(mocks.get).toHaveBeenLastCalledWith(7));
    unmount();
    client.clear();
  });
  it("isolates a newly selected timezone from old cached aggregates", async () => {
    mocks.get.mockResolvedValueOnce(counts).mockImplementation(
      () =>
        new Promise(() => {
          /* Keep the replacement query pending. */
        }),
    );
    const { wrapper, client } = setup();
    const { result, rerender, unmount } = renderHook(
      ({ timezone }) => useProviderRequestCounts(30, timezone),
      { wrapper, initialProps: { timezone: "Asia/Shanghai" } },
    );
    await waitFor(() => expect(result.current.data).toEqual(counts));
    rerender({ timezone: "UTC" });
    expect(result.current.data).toBeUndefined();
    unmount();
    client.clear();
  });
  it("does not reuse a previous profile's counts", async () => {
    mocks.get.mockResolvedValueOnce(counts).mockImplementation(
      () =>
        new Promise(() => {
          /* Keep the replacement query pending. */
        }),
    );
    const { wrapper, client } = setup();
    const { result, rerender, unmount } = renderHook(() => useProviderRequestCounts(30), {
      wrapper,
    });
    await waitFor(() => expect(result.current.data).toEqual(counts));
    mocks.profile = "profile-b";
    rerender();
    expect(result.current.data).toBeUndefined();
    unmount();
    client.clear();
  });
  it("ignores a late response from a previous profile", async () => {
    let finishOld!: (value: ProviderRequestCounts) => void;
    mocks.get
      .mockImplementationOnce(
        () =>
          new Promise<ProviderRequestCounts>((resolve) => {
            finishOld = resolve;
          }),
      )
      .mockResolvedValue({ ...counts, rows: [] });
    const { wrapper, client } = setup();
    const { result, rerender, unmount } = renderHook(() => useProviderRequestCounts(30), {
      wrapper,
    });
    await waitFor(() => expect(mocks.get).toHaveBeenCalledTimes(1));
    mocks.profile = "profile-b";
    rerender();
    await waitFor(() => expect(result.current.data?.rows).toEqual([]));
    await act(async () => {
      finishOld(counts);
      await Promise.resolve();
    });
    expect(result.current.data?.rows).toEqual([]);
    unmount();
    client.clear();
  });
  it("returns an error without fabricating zero counts", async () => {
    mocks.get.mockRejectedValue(new Error("unavailable"));
    const { wrapper, client } = setup();
    const { result, unmount } = renderHook(() => useProviderRequestCounts(30), { wrapper });
    await waitFor(() => expect(result.current.isError).toBe(true));
    expect(result.current.data).toBeUndefined();
    unmount();
    client.clear();
  });
});
