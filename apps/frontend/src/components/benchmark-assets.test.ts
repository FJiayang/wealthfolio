import { describe, it, expect, vi } from "vitest";
import { getAssetProfile, registerBenchmark } from "@/adapters";
import {
  presetBenchmarkPayload,
  registerBenchmarkSelection,
  searchBenchmarkSelection,
} from "./benchmark-assets";
import type { Asset, SymbolSearchResult } from "@/lib/types";
vi.mock("@/adapters", () => ({ getAssetProfile: vi.fn(), registerBenchmark: vi.fn() }));

describe("local benchmark registration", () => {
  it("preserves preset currencies and index routing", () => {
    expect(presetBenchmarkPayload("^HSI")).toMatchObject({
      quoteCcy: "HKD",
      instrumentExchangeMic: "INDEX",
      providerSymbol: "^HSI",
    });
    expect(presetBenchmarkPayload("^N225")).toMatchObject({ quoteCcy: "JPY" });
    expect(presetBenchmarkPayload("VOO")).toMatchObject({
      quoteCcy: "USD",
      instrumentExchangeMic: "ARCX",
    });
    expect(presetBenchmarkPayload("unknown")).toBeUndefined();
  });
  it("carries provider identity from search without guessing currency", () => {
    const selection = searchBenchmarkSelection({
      symbol: "000300",
      canonicalSymbol: "000300",
      currency: "CNY",
      quoteType: "INDEX",
      exchangeMic: "XSHG",
      providerId: "EASTMONEY",
      providerSymbol: "1.000300",
    } as SymbolSearchResult);
    expect(selection.payload).toMatchObject({
      quoteCcy: "CNY",
      providerSymbol: "1.000300",
      instrumentExchangeMic: "XSHG",
    });
    expect(
      searchBenchmarkSelection({ symbol: "unknown", quoteType: "INDEX" } as SymbolSearchResult)
        .payload,
    ).toBeUndefined();
  });
  it("registers an existing asset without replacing its configuration", async () => {
    vi.mocked(getAssetProfile).mockResolvedValue({
      id: "existing",
      kind: "INVESTMENT",
      quoteMode: "MARKET",
      quoteCcy: "EUR",
      isActive: false,
    } as Asset);
    await registerBenchmarkSelection({ id: "existing", name: "Existing" });
    expect(registerBenchmark).toHaveBeenCalledWith({
      id: "existing",
      kind: "INVESTMENT",
      quoteMode: "MARKET",
      quoteCcy: "EUR",
      isActive: false,
    });
  });
});
