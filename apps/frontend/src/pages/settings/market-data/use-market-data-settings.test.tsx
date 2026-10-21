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

describe("HTTP API credentials", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    adapters.getMarketDataProviderSettings.mockResolvedValue([
      { id: "HTTP_API", priority: 7, enabled: true },
    ]);
  });

  it("refreshes the configured provider after replacing its key", async () => {
    const { result } = renderHook(() => useSetApiKey(), { wrapper });
    await act(() => result.current.mutateAsync({ providerId: "HTTP_API", apiKey: "new-key" }));
    expect(adapters.setSecret).toHaveBeenCalledWith("HTTP_API", "new-key");
    expect(adapters.updateMarketDataProviderSettings).toHaveBeenCalledWith({
      providerId: "HTTP_API",
      priority: 7,
      enabled: true,
    });
  });

  it("refreshes the configured provider after removing its optional key", async () => {
    const { result } = renderHook(() => useDeleteApiKey(), { wrapper });
    await act(() => result.current.mutateAsync({ providerId: "HTTP_API" }));
    expect(adapters.deleteSecret).toHaveBeenCalledWith("HTTP_API");
    expect(adapters.updateMarketDataProviderSettings).toHaveBeenCalledWith({
      providerId: "HTTP_API",
      priority: 7,
      enabled: true,
    });
  });

  it("preserves the existing credential flow for other providers", async () => {
    const { result } = renderHook(() => useSetApiKey(), { wrapper });
    await act(() => result.current.mutateAsync({ providerId: "ALPHA_VANTAGE", apiKey: "key" }));
    expect(adapters.setSecret).toHaveBeenCalledWith("ALPHA_VANTAGE", "key");
    expect(adapters.updateMarketDataProviderSettings).not.toHaveBeenCalled();
  });
});
