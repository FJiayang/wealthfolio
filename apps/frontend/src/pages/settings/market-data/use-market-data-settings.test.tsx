import { act, renderHook, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  useDeleteApiKey,
  useSetApiKey,
  useIsSavingProviderSettings,
  useUpdateMarketDataProviderSettings,
} from "./use-market-data-settings";

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

describe("useIsSavingProviderSettings", () => {
  it("stays true until every overlapping save finishes", async () => {
    const finish: (() => void)[] = [];
    adapters.updateMarketDataProviderSettings.mockImplementation(
      () => new Promise<void>((resolve) => finish.push(resolve)),
    );
    const client = new QueryClient();
    const wrapper = ({ children }: { children: ReactNode }) => (
      <QueryClientProvider client={client}>{children}</QueryClientProvider>
    );
    const { result } = renderHook(
      () => ({
        save: useUpdateMarketDataProviderSettings(),
        saving: useIsSavingProviderSettings(),
      }),
      { wrapper },
    );

    act(() => {
      result.current.save.mutate({ providerId: "YAHOO", priority: 1, enabled: true });
      result.current.save.mutate({ providerId: "AMFI", priority: 15, enabled: true });
    });
    await waitFor(() => expect(finish).toHaveLength(2));
    expect(result.current.saving).toBe(true);

    // The later save finishes first. The mutation's own isPending, which follows only
    // the latest call, turns false while the earlier save is still running.
    act(() => finish[1]());
    await waitFor(() => expect(result.current.save.isPending).toBe(false));
    expect(result.current.saving).toBe(true);

    act(() => finish[0]());
    await waitFor(() => expect(result.current.saving).toBe(false));
  });
});
