import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook } from "@testing-library/react";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  useRecalculatePortfolioMutation,
  useUpdatePortfolioMutation,
} from "./use-calculate-portfolio";
import { useSyncMarketDataMutation } from "./use-sync-market-data";

const mocks = vi.hoisted(() => ({
  syncMarketData: vi.fn(),
  updatePortfolio: vi.fn(),
  recalculatePortfolio: vi.fn(),
  toast: vi.fn(),
  logger: { error: vi.fn() },
}));

vi.mock("@/adapters", () => mocks);
vi.mock("@wealthfolio/ui/components/ui/use-toast", () => ({
  toast: mocks.toast,
  useToast: () => ({ toast: mocks.toast }),
}));

function setup<T>(useHook: () => T) {
  const queryClient = new QueryClient();
  const invalidate = vi.spyOn(queryClient, "invalidateQueries");
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
  );
  return { ...renderHook(useHook, { wrapper }), invalidate };
}

describe("background market job admission", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.syncMarketData.mockResolvedValue(undefined);
    mocks.updatePortfolio.mockResolvedValue(undefined);
    mocks.recalculatePortfolio.mockResolvedValue(undefined);
  });

  it("keeps explicit full-history intent without invalidating before completion", async () => {
    const { result, invalidate } = setup(() => useSyncMarketDataMutation(true));
    await act(async () => {
      await result.current.mutateAsync(["SEC:AAPL:XNAS"]);
    });
    expect(mocks.syncMarketData).toHaveBeenCalledWith(["SEC:AAPL:XNAS"], true, undefined);
    expect(invalidate).not.toHaveBeenCalled();
  });

  it.each([
    ["update", useUpdatePortfolioMutation],
    ["recalculate", useRecalculatePortfolioMutation],
  ] as const)("does not invalidate on %s command admission", async (_name, hook) => {
    const { result, invalidate } = setup(hook);
    await act(async () => {
      await result.current.mutateAsync();
    });
    expect(invalidate).not.toHaveBeenCalled();
  });

  it("still reports a rejected sync command", async () => {
    const error = new Error("Profile is locked");
    mocks.syncMarketData.mockRejectedValue(error);
    const { result, invalidate } = setup(() => useSyncMarketDataMutation());
    await act(async () => {
      await expect(result.current.mutateAsync(["SEC:AAPL:XNAS"])).rejects.toThrow(error);
    });
    expect(mocks.toast).toHaveBeenCalledWith(
      expect.objectContaining({
        description: error.message,
        variant: "destructive",
      }),
    );
    expect(invalidate).not.toHaveBeenCalled();
  });
});
