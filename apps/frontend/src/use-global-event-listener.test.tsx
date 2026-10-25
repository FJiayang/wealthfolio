import { act, renderHook, waitFor } from "@/test/render";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { ReactNode } from "react";
import { MemoryRouter } from "react-router-dom";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  updatePortfolio: vi.fn(),
  listen: vi.fn(() => Promise.resolve(vi.fn())),
}));

vi.mock("@/adapters", () => ({
  isTauri: false,
  updatePortfolio: mocks.updatePortfolio,
  logger: { error: vi.fn() },
  listenAssetClassificationsChanged: mocks.listen,
  listenBrokerSyncComplete: mocks.listen,
  listenBrokerSyncError: mocks.listen,
  listenDatabaseRestored: mocks.listen,
  listenMarketSyncComplete: mocks.listen,
  listenMarketSyncError: mocks.listen,
  listenMarketSyncStart: mocks.listen,
  listenPortfolioUpdateComplete: mocks.listen,
  listenPortfolioUpdateError: mocks.listen,
  listenPortfolioUpdateStart: mocks.listen,
}));
vi.mock("@/hooks/use-platform", () => ({ useIsMobileViewport: () => false }));
vi.mock("@/context/portfolio-sync-context", () => ({ usePortfolioSyncOptional: () => null }));
vi.mock("@/lib/asset-logo-registry", () => ({ assetLogoRegistry: { clear: vi.fn() } }));

import useGlobalEventListener, { usePortfolioResumeCheck } from "./use-global-event-listener";

function wrapper({ children }: { children: ReactNode }) {
  return (
    <MemoryRouter>
      <QueryClientProvider client={new QueryClient()}>{children}</QueryClientProvider>
    </MemoryRouter>
  );
}

beforeEach(() => {
  mocks.updatePortfolio.mockReset().mockResolvedValue(undefined);
  mocks.listen.mockClear();
});
afterEach(() => vi.restoreAllMocks());

describe("automatic local portfolio rebuilds", () => {
  it("rebuilds on startup without requesting market data", async () => {
    const { result } = renderHook(useGlobalEventListener, { wrapper });
    await waitFor(() => expect(result.current).toBe(true));
    expect(mocks.updatePortfolio).toHaveBeenCalledExactlyOnceWith(false);
  });

  it("rebuilds after a long suspension without changing the automatic market schedule", () => {
    let now = 1000;
    const visibility = vi.spyOn(document, "visibilityState", "get");
    vi.spyOn(Date, "now").mockImplementation(() => now);
    renderHook(usePortfolioResumeCheck);
    visibility.mockReturnValue("hidden");
    act(() => document.dispatchEvent(new Event("visibilitychange")));
    now += 6 * 60 * 1000;
    visibility.mockReturnValue("visible");
    act(() => document.dispatchEvent(new Event("visibilitychange")));
    expect(mocks.updatePortfolio).toHaveBeenCalledExactlyOnceWith(false);
    act(() => document.dispatchEvent(new Event("visibilitychange")));
    expect(mocks.updatePortfolio).toHaveBeenCalledTimes(1);
  });

  it("does not rebuild for a short visibility change", () => {
    let now = 1000;
    const visibility = vi.spyOn(document, "visibilityState", "get");
    vi.spyOn(Date, "now").mockImplementation(() => now);
    renderHook(usePortfolioResumeCheck);
    visibility.mockReturnValue("hidden");
    act(() => document.dispatchEvent(new Event("visibilitychange")));
    now += 60 * 1000;
    visibility.mockReturnValue("visible");
    act(() => document.dispatchEvent(new Event("visibilitychange")));
    expect(mocks.updatePortfolio).not.toHaveBeenCalled();
  });
});
