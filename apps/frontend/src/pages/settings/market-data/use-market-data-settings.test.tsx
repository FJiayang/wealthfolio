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

  it("preserves the existing credential flow for other providers", async () => {
    const { result } = renderHook(() => useSetApiKey(), { wrapper });
    await act(() => result.current.mutateAsync({ providerId: "ALPHA_VANTAGE", apiKey: "key" }));
    expect(adapters.setSecret).toHaveBeenCalledWith("ALPHA_VANTAGE", "key");
    expect(adapters.updateMarketDataProviderSettings).not.toHaveBeenCalled();
  });
});
