import { getAssetProfile, registerBenchmark } from "@/adapters";
import type { NewAsset, SymbolSearchResult } from "@/lib/types";
import { mapQuoteTypeToInstrumentType } from "@/pages/activity/import/utils/asset-review-utils";

// Predefined benchmarks with canonical asset IDs
// exchangeMic is undefined for indices (will use "INDEX" as pseudo-MIC)
// exchangeMic is set for ETFs that trade on real exchanges
export const BENCHMARKS: {
  groupKey: string;
  items: { symbol: string; name: string; descriptionKey: string; exchangeMic?: string }[];
}[] = [
  {
    groupKey: "benchmark_group_us_indices",
    items: [
      { symbol: "^GSPC", name: "S&P 500", descriptionKey: "benchmark_desc_large_cap_us" },
      { symbol: "^NDX", name: "Nasdaq 100", descriptionKey: "benchmark_desc_large_cap_tech_us" },
      { symbol: "^RUT", name: "Russell 2000", descriptionKey: "benchmark_desc_small_cap_us" },
      { symbol: "^DJI", name: "Dow Jones", descriptionKey: "benchmark_desc_blue_chip_us" },
    ],
  },
  {
    groupKey: "benchmark_group_european_indices",
    items: [
      { symbol: "^FTSE", name: "FTSE 100", descriptionKey: "benchmark_desc_large_cap_uk" },
      {
        symbol: "^STOXX50E",
        name: "EURO STOXX 50",
        descriptionKey: "benchmark_desc_european_blue_chip",
      },
      { symbol: "^GDAXI", name: "DAX", descriptionKey: "benchmark_desc_german_blue_chip" },
      { symbol: "^FCHI", name: "CAC 40", descriptionKey: "benchmark_desc_french_large_cap" },
      { symbol: "^IBEX", name: "IBEX 35", descriptionKey: "benchmark_desc_spanish_large_cap" },
      { symbol: "^AEX", name: "AEX", descriptionKey: "benchmark_desc_dutch_blue_chip" },
      {
        symbol: "^OMX",
        name: "OMX Stockholm 30",
        descriptionKey: "benchmark_desc_swedish_large_cap",
      },
    ],
  },
  {
    groupKey: "benchmark_group_asian_indices",
    items: [
      { symbol: "^N225", name: "Nikkei 225", descriptionKey: "benchmark_desc_japanese_large_cap" },
      { symbol: "^HSI", name: "Hang Seng", descriptionKey: "benchmark_desc_hong_kong_large_cap" },
      {
        symbol: "000001.SS",
        name: "Shanghai Composite",
        descriptionKey: "benchmark_desc_chinese_a_shares",
      },
      { symbol: "^KS11", name: "KOSPI", descriptionKey: "benchmark_desc_south_korean" },
      { symbol: "^TWII", name: "Taiwan Weighted", descriptionKey: "benchmark_desc_taiwanese" },
      { symbol: "^AXJO", name: "ASX 200", descriptionKey: "benchmark_desc_australian_large_cap" },
      { symbol: "^BSESN", name: "BSE Sensex", descriptionKey: "benchmark_desc_indian_large_cap" },
      { symbol: "^NSEI", name: "NIFTY 50", descriptionKey: "benchmark_desc_indian_blue_chip" },
    ],
  },
  {
    groupKey: "benchmark_group_global_emerging",
    items: [
      {
        symbol: "EEM",
        name: "MSCI Emerging Markets",
        descriptionKey: "benchmark_desc_emerging_market",
        exchangeMic: "ARCX",
      },
      {
        symbol: "ACWI",
        name: "MSCI All Country World",
        descriptionKey: "benchmark_desc_global_equity",
        exchangeMic: "XNAS",
      },
      {
        symbol: "IEFA",
        name: "Core MSCI EAFE",
        descriptionKey: "benchmark_desc_eafe",
        exchangeMic: "ARCX",
      },
    ],
  },
  {
    groupKey: "benchmark_group_etfs",
    items: [
      {
        symbol: "VOO",
        name: "Vanguard S&P 500",
        descriptionKey: "benchmark_desc_sp500_fund",
        exchangeMic: "ARCX",
      },
      {
        symbol: "VTI",
        name: "Vanguard Total Stock",
        descriptionKey: "benchmark_desc_total_us_market",
        exchangeMic: "ARCX",
      },
      {
        symbol: "VEA",
        name: "Vanguard FTSE Developed",
        descriptionKey: "benchmark_desc_developed_ex_us",
        exchangeMic: "ARCX",
      },
      {
        symbol: "VWO",
        name: "Vanguard FTSE Emerging",
        descriptionKey: "benchmark_desc_emerging_markets",
        exchangeMic: "ARCX",
      },
    ],
  },
];

const INDEX_CURRENCIES: Record<string, string> = {
  "^GSPC": "USD",
  "^NDX": "USD",
  "^RUT": "USD",
  "^DJI": "USD",
  "^FTSE": "GBP",
  "^STOXX50E": "EUR",
  "^GDAXI": "EUR",
  "^FCHI": "EUR",
  "^IBEX": "EUR",
  "^AEX": "EUR",
  "^OMX": "SEK",
  "^N225": "JPY",
  "^HSI": "HKD",
  "000001.SS": "CNY",
  "^KS11": "KRW",
  "^TWII": "TWD",
  "^AXJO": "AUD",
  "^BSESN": "INR",
  "^NSEI": "INR",
};
export interface BenchmarkSelection {
  id: string;
  name: string;
  payload?: NewAsset;
}
export function presetBenchmarkPayload(symbol: string): NewAsset | undefined {
  const preset = BENCHMARKS.flatMap((group) => group.items).find((item) => item.symbol === symbol);
  if (!preset) return undefined;
  const mic = preset.exchangeMic;
  return {
    kind: "INVESTMENT",
    name: preset.name,
    isActive: true,
    quoteMode: "MARKET",
    quoteCcy: mic ? "USD" : INDEX_CURRENCIES[symbol],
    instrumentType: "EQUITY",
    instrumentSymbol: symbol,
    instrumentExchangeMic: mic ?? "INDEX",
    providerId: "YAHOO",
    providerSymbol: symbol,
  };
}
export function searchBenchmarkSelection(ticker: SymbolSearchResult): BenchmarkSelection {
  const instrumentType = mapQuoteTypeToInstrumentType(ticker.quoteType);
  return {
    id: ticker.existingAssetId || ticker.symbol,
    name: ticker.longName || ticker.symbol,
    payload:
      ticker.existingAssetId || !ticker.currency || !instrumentType
        ? undefined
        : {
            kind: "INVESTMENT",
            name: ticker.longName || ticker.symbol,
            isActive: true,
            quoteMode: "MARKET",
            quoteCcy: ticker.currency,
            instrumentType,
            instrumentSymbol: ticker.canonicalSymbol || ticker.symbol,
            instrumentExchangeMic: ticker.canonicalExchangeMic || ticker.exchangeMic,
            providerId: ticker.providerId || ticker.dataSource,
            providerSymbol: ticker.providerSymbol || ticker.symbol,
          },
  };
}
export async function registerBenchmarkSelection(selection: BenchmarkSelection) {
  const preset = selection.payload ?? presetBenchmarkPayload(selection.id);
  if (preset) return registerBenchmark(preset);
  const asset = await getAssetProfile(selection.id);
  return registerBenchmark({
    id: asset.id,
    kind: asset.kind,
    isActive: asset.isActive ?? true,
    quoteMode: asset.quoteMode,
    quoteCcy: asset.quoteCcy,
  });
}
