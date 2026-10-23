import { act, renderHook } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useDeleteApiKey, useSetApiKey } from "./use-market-data-settings";

const adapters = vi.hoisted(() => ({
  setSecret: vi.fn(),
  deleteSecret: vi.fn(),
  getMarketDataProviderSettings: vi.fn(),
  updateMarketDataProviderSettings: vi.fn(),
}));
vi.mock("@/adapters", () => adapters);
vi.mock("@wealthfolio/ui/components/ui/use-toast", () => ({ toast: vi.fn() }));

function wrapper({ children }: { children: ReactNode }) {
  return <QueryClientProvider client={new QueryClient()}>{children}</QueryClientProvider>;
}

describe("market data credentials", () => {
  beforeEach(() => vi.resetAllMocks());

  it("removes an existing provider key", async () => {
    const { result } = renderHook(() => useDeleteApiKey(), { wrapper });
    await act(() => result.current.mutateAsync({ providerId: "ALPHA_VANTAGE" }));
    expect(adapters.deleteSecret).toHaveBeenCalledWith("ALPHA_VANTAGE");
  });

  it("trims and reloads IFIND after saving or replacing its key", async () => {
    adapters.getMarketDataProviderSettings.mockResolvedValue([
      { id: "IFIND", priority: 13, enabled: true },
    ]);
    const { result } = renderHook(() => useSetApiKey(), { wrapper });
    for (const apiKey of [" first ", " replacement "]) {
      await act(() => result.current.mutateAsync({ providerId: "IFIND", apiKey }));
      expect(adapters.setSecret).toHaveBeenLastCalledWith("IFIND", apiKey.trim());
      expect(adapters.updateMarketDataProviderSettings).toHaveBeenLastCalledWith({
        providerId: "IFIND",
        priority: 13,
        enabled: true,
      });
    }
  });

  it("reloads IFIND after deleting its key", async () => {
    adapters.getMarketDataProviderSettings.mockResolvedValue([
      { id: "IFIND", priority: 13, enabled: false },
    ]);
    const { result } = renderHook(() => useDeleteApiKey(), { wrapper });
    await act(() => result.current.mutateAsync({ providerId: "IFIND" }));
    expect(adapters.deleteSecret).toHaveBeenCalledWith("IFIND");
    expect(adapters.updateMarketDataProviderSettings).toHaveBeenCalledWith({
      providerId: "IFIND",
      priority: 13,
      enabled: false,
    });
  });

  it("preserves the existing credential flow for other providers", async () => {
    const { result } = renderHook(() => useSetApiKey(), { wrapper });
    await act(() => result.current.mutateAsync({ providerId: "ALPHA_VANTAGE", apiKey: " key " }));
    expect(adapters.setSecret).toHaveBeenCalledWith("ALPHA_VANTAGE", " key ");
    expect(adapters.updateMarketDataProviderSettings).not.toHaveBeenCalled();
  });
});
