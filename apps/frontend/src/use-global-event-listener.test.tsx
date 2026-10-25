import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import useGlobalEventListener from "./use-global-event-listener";
import { QueryKeys } from "@/lib/query-keys";

const mocks = vi.hoisted(() => ({
  handlers: {} as Record<string, (event: { payload: unknown }) => void>,
  updatePortfolio: vi.fn().mockResolvedValue(undefined),
  setIdle: vi.fn(),
  setPortfolioCalculating: vi.fn(),
  setMarketSyncing: vi.fn(),
  mobile: true,
  dismiss: vi.fn(),
}));

vi.mock("@/adapters", () => {
  const listen = (name: string) =>
    vi.fn((handler: (event: { payload: unknown }) => void) => {
      mocks.handlers[name] = handler;
      return Promise.resolve(vi.fn());
    });
  return {
    isTauri: false,
    updatePortfolio: mocks.updatePortfolio,
    logger: { error: vi.fn() },
    listenPortfolioUpdateStart: listen("start"),
    listenPortfolioUpdateComplete: listen("complete"),
    listenPortfolioUpdateError: listen("error"),
    listenMarketSyncStart: listen("market-start"),
    listenMarketSyncComplete: listen("market-complete"),
    listenMarketSyncError: listen("market-error"),
    listenAssetClassificationsChanged: listen("classifications"),
    listenDatabaseRestored: listen("restore"),
    listenBrokerSyncComplete: listen("broker-complete"),
    listenBrokerSyncError: listen("broker-error"),
  };
});
vi.mock("@/context/portfolio-sync-context", () => ({
  usePortfolioSyncOptional: () => mocks,
}));
vi.mock("@/hooks/use-platform", () => ({
  useIsMobileViewport: () => mocks.mobile,
}));
vi.mock("@/lib/asset-logo-registry", () => ({ assetLogoRegistry: { reset: vi.fn() } }));
vi.mock("react-router-dom", () => ({ useNavigate: () => vi.fn() }));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
vi.mock("sonner", () => ({
  toast: {
    dismiss: mocks.dismiss,
    loading: vi.fn(),
    error: vi.fn(),
    warning: vi.fn(),
    success: vi.fn(),
    info: vi.fn(),
  },
}));

async function setup() {
  const client = new QueryClient();
  const invalidate = vi.spyOn(client, "invalidateQueries");
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={client}>{children}</QueryClientProvider>
  );
  const hook = renderHook(useGlobalEventListener, { wrapper });
  await waitFor(() => expect(hook.result.current).toBe(true));
  return { ...hook, invalidate, client };
}

describe("portfolio job completion", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.handlers = {};
    mocks.mobile = true;
  });

  it("clears mobile progress and refreshes only health after unchanged prices", async () => {
    const { invalidate, client } = await setup();
    act(() => mocks.handlers.complete({ payload: { changed: false } }));
    expect(mocks.setIdle).toHaveBeenCalledOnce();
    expect(invalidate).toHaveBeenCalledOnce();
    const predicate = invalidate.mock.calls[0][0]?.predicate;
    expect(predicate).toBeDefined();
    for (const key of [QueryKeys.HEALTH_STATUS, QueryKeys.MARKET_DATA_PROVIDER_SETTINGS]) {
      expect(
        predicate?.(
          client.getQueryCache().build(client, { queryKey: [key] as readonly unknown[] }),
        ),
      ).toBe(true);
    }
    expect(
      predicate?.(
        client.getQueryCache().build(client, {
          queryKey: [QueryKeys.PERFORMANCE_HISTORY] as readonly unknown[],
        }),
      ),
    ).toBe(false);
  });

  it("dismisses desktop progress without invalidating performance", async () => {
    mocks.mobile = false;
    const { invalidate, client } = await setup();
    act(() => mocks.handlers.complete({ payload: { changed: false } }));
    expect(mocks.dismiss).toHaveBeenCalledWith("portfolio-update-start");
    expect(invalidate).toHaveBeenCalledOnce();
    const predicate = invalidate.mock.calls[0][0]?.predicate;
    expect(predicate).toBeDefined();
    for (const key of [QueryKeys.HEALTH_STATUS, QueryKeys.MARKET_DATA_PROVIDER_SETTINGS]) {
      expect(
        predicate?.(
          client.getQueryCache().build(client, { queryKey: [key] as readonly unknown[] }),
        ),
      ).toBe(true);
    }
    expect(
      predicate?.(
        client.getQueryCache().build(client, {
          queryKey: [QueryKeys.PERFORMANCE_HISTORY] as readonly unknown[],
        }),
      ),
    ).toBe(false);
  });

  it.each([{ changed: true }, { changed: null }, {}, null])(
    "refreshes once after completed writes or an unknown legacy outcome: %j",
    async (payload) => {
      const { invalidate } = await setup();
      expect(invalidate).not.toHaveBeenCalled();
      act(() => mocks.handlers.complete({ payload }));
      expect(invalidate).toHaveBeenCalledOnce();
      expect(mocks.setIdle).toHaveBeenCalledOnce();
    },
  );
});
