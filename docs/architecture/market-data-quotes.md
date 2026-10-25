# Market Data & Quotes System Architecture

## Overview

The market data system is responsible for fetching, storing, and managing
financial quotes from external providers. It follows a layered architecture with
clear separation between:

- **Provider Layer** (`market-data` crate) - External API integrations
- **Client Layer** (`core/quotes`) - Domain logic and orchestration
- **Storage Layer** (`storage-sqlite`) - Persistence

```
┌─────────────────────────────────────────────────────────────────────┐
│                        Application Layer                             │
│                   (Tauri Commands / REST API)                        │
└─────────────────────────────────────────────────────────────────────┘
                                   │
                                   ▼
┌─────────────────────────────────────────────────────────────────────┐
│                         QuoteService                                 │
│            Unified facade for all quote operations                   │
│   ┌─────────────┐  ┌─────────────────┐  ┌──────────────────┐        │
│   │ QuoteStore  │  │ QuoteSyncService │  │ MarketDataClient │        │
│   │   (CRUD)    │  │    (Sync)        │  │   (Providers)    │        │
│   └─────────────┘  └─────────────────┘  └──────────────────┘        │
└─────────────────────────────────────────────────────────────────────┘
                                   │
                                   ▼
┌─────────────────────────────────────────────────────────────────────┐
│                      ProviderRegistry                                │
│              (market-data crate orchestration)                       │
│   ┌────────────┐  ┌───────────────┐  ┌────────────────┐             │
│   │ Providers  │  │ SymbolResolver │  │ CircuitBreaker │             │
│   └────────────┘  └───────────────┘  └────────────────┘             │
└─────────────────────────────────────────────────────────────────────┘
                                   │
                                   ▼
┌─────────────────────────────────────────────────────────────────────┐
│                     External Market Data APIs                        │
│         Yahoo Finance │ Alpha Vantage │ MarketData.app │ etc.        │
└─────────────────────────────────────────────────────────────────────┘
```

---

## Module Organization

### `crates/market-data/` - Provider Integration Layer

Low-level crate for market data provider integrations. No knowledge of the
application domain (assets, portfolios).

```
market-data/
├── provider/           # Provider implementations
│   ├── traits.rs       # MarketDataProvider trait
│   ├── yahoo/          # Yahoo Finance provider
│   ├── alpha_vantage/  # Alpha Vantage provider
│   ├── marketdata_app/ # MarketData.app provider
│   ├── metal_price_api/# Metal Price API provider
│   └── finnhub/        # Finnhub provider
├── registry/           # Provider orchestration
│   ├── registry.rs     # ProviderRegistry
│   ├── circuit_breaker.rs
│   ├── rate_limiter.rs
│   ├── validator.rs
│   └── skip_reason.rs  # Diagnostics
├── resolver/           # Symbol resolution
│   ├── chain.rs        # ResolverChain
│   ├── rules_resolver.rs
│   └── exchange_map.rs
├── models/             # Data transfer objects
│   ├── instrument.rs   # InstrumentId enum
│   ├── quote.rs        # Quote struct
│   └── profile.rs      # AssetProfile
└── errors/             # Error types
    ├── mod.rs          # MarketDataError
    └── retry.rs        # RetryClass
```

### `crates/core/src/quotes/` - Domain Layer

Application-level quote management with business logic.

```
quotes/
├── model.rs            # Quote, SymbolSearchResult, DataSource
├── types.rs            # AssetId, Day, ProviderId, QuoteSource
├── store.rs            # QuoteStore, ProviderSettingsStore traits
├── sync_state.rs       # QuoteSyncState, SyncStateStore
├── sync.rs             # QuoteSyncService
├── service.rs          # QuoteService (unified facade)
├── client.rs           # MarketDataClient (bridge to market-data)
├── import.rs           # CSV import/export
├── errors.rs           # MarketDataError (core-level)
├── constants.rs        # Configuration constants
└── provider_settings.rs # Provider configuration models
```

---

## Key Components

### 1. MarketDataProvider (Trait)

The core abstraction for external data sources.

```
┌─────────────────────────────────────────────────────────────┐
│                    MarketDataProvider                        │
├─────────────────────────────────────────────────────────────┤
│ + id() -> &str                                              │
│ + priority() -> u8                                          │
│ + capabilities() -> ProviderCapabilities                    │
│ + rate_limit() -> RateLimit                                 │
├─────────────────────────────────────────────────────────────┤
│ + get_latest_quote(instrument) -> Quote                     │
│ + get_historical_quotes(instrument, start, end) -> Vec<Quote>│
│ + search(query) -> Vec<SearchResult>                        │
│ + get_profile(symbol) -> AssetProfile                       │
└─────────────────────────────────────────────────────────────┘
              ▲
              │ implements
    ┌─────────┼─────────┬─────────────┬──────────────┐
    │         │         │             │              │
┌───┴───┐ ┌───┴───┐ ┌───┴────┐ ┌─────┴─────┐ ┌─────┴─────┐
│ Yahoo │ │Alpha  │ │Market  │ │MetalPrice │ │  Finnhub  │
│Finance│ │Vantage│ │Data.app│ │   API     │ │           │
└───────┘ └───────┘ └────────┘ └───────────┘ └───────────┘
```

**Provider Capabilities:**

Each provider declares what it supports:

| Provider        | Equities | Crypto | Forex | Metals | Search | Profiles | Historical |
| --------------- | -------- | ------ | ----- | ------ | ------ | -------- | ---------- |
| Yahoo           | Global   | Yes    | Yes   | Yes    | Yes    | Yes      | Yes        |
| Alpha Vantage   | Global   | Yes    | Yes   | No     | No     | Yes      | Yes        |
| MarketData.app  | US only  | No     | No    | No     | No     | No       | Yes        |
| Metal Price API | No       | No     | No    | Yes    | No     | No       | No         |
| Finnhub         | US/EU    | No     | No    | No     | Yes    | Yes      | Yes        |

### 2. ProviderRegistry

Orchestrates provider selection and fault tolerance.

```
┌─────────────────────────────────────────────────────────────┐
│                     ProviderRegistry                         │
├─────────────────────────────────────────────────────────────┤
│ - providers: Vec<MarketDataProvider>                        │
│ - resolver: SymbolResolver                                  │
│ - rate_limiter: RateLimiter                                 │
│ - circuit_breaker: CircuitBreaker                           │
│ - custom_priorities: HashMap<ProviderId, i32>               │
├─────────────────────────────────────────────────────────────┤
│ + fetch_quotes(context, start, end) -> Vec<Quote>           │
│ + fetch_latest_quote(context) -> Quote                      │
│ + search(query) -> Vec<SearchResult>                        │
│ + get_profile(symbol) -> AssetProfile                       │
└─────────────────────────────────────────────────────────────┘
```

**Provider Selection Flow:**

```
                    QuoteContext
                         │
                         ▼
┌─────────────────────────────────────────┐
│     1. Filter by Capabilities           │
│  - Supports instrument type?            │
│  - Supports operation (historical/latest)?│
│  - Supports market (MIC coverage)?      │
└─────────────────────────────────────────┘
                         │
                         ▼
┌─────────────────────────────────────────┐
│     2. Filter by Circuit Breaker        │
│  - Is circuit open? Skip provider       │
└─────────────────────────────────────────┘
                         │
                         ▼
┌─────────────────────────────────────────┐
│     3. Sort by Priority                 │
│  1. Preferred provider (from context)   │
│  2. Custom priorities (user settings)   │
│  3. Default provider priority           │
└─────────────────────────────────────────┘
                         │
                         ▼
┌─────────────────────────────────────────┐
│     4. Try Providers in Order           │
│  - Resolve symbol for provider          │
│  - Check rate limit                     │
│  - Make request                         │
│  - Handle errors (retry/fallback)       │
└─────────────────────────────────────────┘
```

### 3. SymbolResolver

Translates canonical instrument identifiers to provider-specific formats.

```
┌────────────────────────────────────────────────────────────┐
│                       ResolverChain                         │
│                                                             │
│   InstrumentId ──► ProviderInstrument                      │
│                                                             │
│   Example:                                                  │
│   Equity{ticker:"SHOP", mic:"XTSE"}                        │
│     ──► Yahoo:  EquitySymbol{symbol:"SHOP.TO"}             │
│     ──► Alpha:  EquitySymbol{symbol:"SHOP.TRT"}            │
│                                                             │
│   Fx{base:"EUR", quote:"USD"}                              │
│     ──► Yahoo:  FxSymbol{symbol:"EURUSD=X"}                │
│     ──► Alpha:  FxPair{from:"EUR", to:"USD"}               │
└────────────────────────────────────────────────────────────┘
```

**InstrumentId Variants:**

| Variant  | Fields                 | Example                         |
| -------- | ---------------------- | ------------------------------- |
| `Equity` | ticker, mic (optional) | `{ticker: "AAPL", mic: "XNAS"}` |
| `Crypto` | base, quote            | `{base: "BTC", quote: "USD"}`   |
| `Fx`     | base, quote            | `{base: "EUR", quote: "USD"}`   |
| `Metal`  | code, quote            | `{code: "XAU", quote: "USD"}`   |

### 4. Circuit Breaker

Protects against failing providers.

```
                    ┌─────────┐
         success    │         │    failure threshold
        ┌──────────►│ CLOSED  ├───────────────┐
        │           │         │               │
        │           └─────────┘               ▼
        │                               ┌─────────┐
        │                               │         │
        │                               │  OPEN   │
        │                               │         │
        │                               └────┬────┘
        │                                    │
        │           ┌─────────┐              │ timeout
        │  success  │         │◄─────────────┘
        └───────────┤HALF-OPEN│
           (reset)  │         │────────────┐
                    └─────────┘   failure  │
                         ▲                 │
                         └─────────────────┘
                              (reopen)
```

**States:**

- **CLOSED**: Normal operation, requests pass through
- **OPEN**: Provider disabled, requests fail fast
- **HALF-OPEN**: Testing if provider recovered

### 5. RetryClass

Determines how errors are handled.

```
┌─────────────────────────────────────────────────────────────┐
│                       RetryClass                             │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│  Never              ─► Stop immediately, error is terminal   │
│                        (SymbolNotFound, ValidationFailed)    │
│                                                              │
│  FailoverWithPenalty ─► Try next provider + record failure  │
│                        (RateLimited, Timeout)                │
│                        Circuit breaker tracks failures       │
│                                                              │
│  NextProvider       ─► Try next provider, no penalty        │
│                        (NoDataForRange, ResolutionFailed)    │
│                                                              │
│  CircuitOpen        ─► Skip this provider                   │
│                        (Provider previously failed)          │
│                                                              │
└─────────────────────────────────────────────────────────────┘
```

---

## Core Layer Components

### 6. QuoteService

Unified facade combining all quote operations.

```
┌─────────────────────────────────────────────────────────────┐
│                       QuoteService                           │
├─────────────────────────────────────────────────────────────┤
│ Dependencies:                                                │
│   - QuoteStore (quote persistence)                          │
│   - SyncStateStore (sync tracking)                          │
│   - ProviderSettingsStore (provider config)                 │
│   - AssetRepository (asset info)                            │
│   - MarketDataClient (provider bridge)                      │
│   - SecretStore (API keys)                                  │
├─────────────────────────────────────────────────────────────┤
│ Operations:                                                  │
│                                                              │
│ CRUD:                                                        │
│   get_latest_quote(symbol)                                  │
│   get_historical_quotes(symbol)                             │
│   get_quotes_in_range(symbols, start, end)                  │
│   add_quote(quote)                                          │
│   delete_quote(id)                                          │
│                                                              │
│ Sync:                                                        │
│   sync() -> SyncResult                                      │
│   resync(symbols) -> SyncResult                             │
│   refresh_sync_state()                                      │
│                                                              │
│ Provider:                                                    │
│   search_symbol(query)                                      │
│   get_asset_profile(symbol)                                 │
│   get_providers_info()                                      │
│   update_provider_settings(id, priority, enabled)           │
│                                                              │
│ Import:                                                      │
│   import_quotes(csv_data, overwrite)                        │
└─────────────────────────────────────────────────────────────┘
```

### 7. QuoteSyncService

Manages quote synchronization for portfolio assets.

```
┌─────────────────────────────────────────────────────────────┐
│                    QuoteSyncService                          │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│  sync()                                                      │
│    │                                                         │
│    ├─► 1. Build sync plan for each asset                    │
│    │      - Determine date range needed                      │
│    │      - Check existing quotes                            │
│    │      - Calculate gaps to fill                           │
│    │                                                         │
│    ├─► 2. Execute sync for each asset                       │
│    │      - Fetch quotes via MarketDataClient                │
│    │      - Sort by timestamp (guarantees ordering)          │
│    │      - Save to QuoteStore                               │
│    │      - Update sync state                                │
│    │                                                         │
│    └─► 3. Return SyncResult                                 │
│           - Success/failure per asset                        │
│           - Diagnostics (which providers tried)              │
│                                                              │
└─────────────────────────────────────────────────────────────┘
```

**Sync Categories:**

| Category  | Description              | Sync Behavior                               |
| --------- | ------------------------ | ------------------------------------------- |
| `Active`  | Has open position        | Sync from first activity to today           |
| `Closed`  | Position closed recently | Sync during grace period                    |
| `Dormant` | Position closed long ago | No sync needed                              |
| `FxRate`  | Currency pair            | Sync when activities exist in that currency |

### 8. MarketDataClient

Bridge between core domain and market-data crate.

```
┌─────────────────────────────────────────────────────────────┐
│                    MarketDataClient                          │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│  Responsibilities:                                           │
│                                                              │
│  1. Provider Initialization                                  │
│     - Load API keys from SecretStore                        │
│     - Create provider instances                              │
│     - Pass user priorities to registry                       │
│                                                              │
│  2. Asset → QuoteContext Conversion                         │
│     - Asset.to_instrument_id() → InstrumentId               │
│     - Build QuoteContext with overrides                      │
│                                                              │
│  3. Quote Conversion                                         │
│     - market-data Quote → core Quote                        │
│     - Generate deterministic quote IDs                       │
│     - Map provider source to DataSource                      │
│                                                              │
│  4. Error Translation                                        │
│     - market-data errors → core errors                      │
│     - Preserve error semantics for handling                  │
│                                                              │
└─────────────────────────────────────────────────────────────┘
```

---

## Data Models

### Quote

```
┌─────────────────────────────────────────────────────────────┐
│                         Quote                                │
├─────────────────────────────────────────────────────────────┤
│ id: String          # Format: {asset_id}_{YYYY-MM-DD}_{source}│
│ symbol: String      # Asset ID (foreign key)                 │
│ timestamp: DateTime # Quote date/time                        │
│ open: Decimal                                                │
│ high: Decimal                                                │
│ low: Decimal                                                 │
│ close: Decimal      # Most important price                   │
│ adjclose: Decimal   # Adjusted for splits/dividends          │
│ volume: Decimal                                              │
│ currency: String    # Quote currency                         │
│ data_source: DataSource                                      │
│ created_at: DateTime                                         │
└─────────────────────────────────────────────────────────────┘
```

### QuoteSyncState

```
┌─────────────────────────────────────────────────────────────┐
│                     QuoteSyncState                           │
├─────────────────────────────────────────────────────────────┤
│ asset_id: String                                             │
│ category: SyncCategory   # Active/Closed/Dormant/FxRate     │
│ first_activity_date: Date                                    │
│ last_activity_date: Date                                     │
│ latest_quote_date: Option<Date>                             │
│ earliest_quote_date: Option<Date>                           │
│ last_sync_at: Option<DateTime>                              │
│ last_sync_error: Option<String>                             │
└─────────────────────────────────────────────────────────────┘
```

### Strong Types

The system uses newtype patterns for type safety:

| Type          | Underlying  | Purpose                        |
| ------------- | ----------- | ------------------------------ |
| `AssetId`     | `String`    | Internal asset identifier      |
| `Day`         | `NaiveDate` | UTC date bucket for quotes     |
| `ProviderId`  | `String`    | Provider identifier            |
| `QuoteSource` | enum        | Manual or Provider(ProviderId) |
| `Currency`    | `Cow<str>`  | ISO 4217 currency code         |

---

## Data Flow

### Fetching Historical Quotes

```
User/Scheduler
      │
      ▼
┌─────────────────┐
│  QuoteService   │
│    sync()       │
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│QuoteSyncService │
│ build_sync_plan │
└────────┬────────┘
         │
         ▼
┌─────────────────┐     ┌─────────────────┐
│MarketDataClient │────►│ Asset.to_       │
│fetch_historical │     │ instrument_id() │
└────────┬────────┘     └─────────────────┘
         │
         ▼
┌─────────────────┐
│ProviderRegistry │
│  fetch_quotes   │
└────────┬────────┘
         │
    ┌────┴────┐
    ▼         ▼
┌───────┐ ┌───────┐
│Resolve│ │Filter │
│Symbol │ │by caps│
└───┬───┘ └───┬───┘
    │         │
    └────┬────┘
         ▼
┌─────────────────┐
│ Try Provider 1  │──fail──► Try Provider 2 ──► ...
└────────┬────────┘
         │ success
         ▼
┌─────────────────┐
│ Validate Quotes │
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│ Sort by timestamp│
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│  QuoteStore     │
│  upsert_quotes  │
└─────────────────┘
```

### Symbol Search

```
User Query: "AAPL"
      │
      ▼
┌─────────────────┐
│  QuoteService   │
│ search_symbol() │
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│MarketDataClient │
│    search()     │
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│ProviderRegistry │
│    search()     │
└────────┬────────┘
         │
    Parallel requests to
    providers with search capability
         │
    ┌────┼────┐
    ▼    ▼    ▼
┌──────┐┌──────┐┌──────┐
│Yahoo ││Alpha ││Finnhub│
│search││search││search │
└──┬───┘└──┬───┘└──┬───┘
   │       │       │
   └───────┼───────┘
           ▼
┌─────────────────┐
│ Merge & Dedupe  │
│    Results      │
└────────┬────────┘
         │
         ▼
   Vec<SymbolSearchResult>
```

---

## Error Handling

### Error Flow Across Boundaries

```
┌─────────────────────────────────────────────────────────────┐
│                    market-data crate                         │
│                                                              │
│  MarketDataError                                             │
│  ├── SymbolNotFound(String)                                 │
│  ├── RateLimited { provider }                               │
│  ├── Timeout { provider }                                   │
│  ├── ProviderError { provider, message }                    │
│  ├── ValidationFailed { message }                           │
│  ├── NoDataForRange                                         │
│  ├── CircuitOpen { provider }                               │
│  ├── NoProvidersAvailable                                   │
│  └── AllProvidersFailed                                     │
│                                                              │
└──────────────────────────┬──────────────────────────────────┘
                           │
                           │ From<> implementation
                           ▼
┌─────────────────────────────────────────────────────────────┐
│                      core crate                              │
│                                                              │
│  MarketDataError (core)                                      │
│  ├── NotFound(String)          ◄── SymbolNotFound           │
│  ├── RateLimitExceeded(String) ◄── RateLimited              │
│  ├── Timeout(String)           ◄── Timeout                  │
│  ├── ProviderError(String)     ◄── ProviderError            │
│  ├── InvalidData(String)       ◄── ValidationFailed         │
│  ├── NoData                    ◄── NoDataForRange           │
│  ├── CircuitOpen(String)       ◄── CircuitOpen              │
│  ├── NoProvidersAvailable      ◄── NoProvidersAvailable     │
│  └── ProviderExhausted(String) ◄── AllProvidersFailed       │
│                                                              │
│  Helper methods:                                             │
│  ├── is_terminal() → bool                                   │
│  ├── should_try_next_provider() → bool                      │
│  └── is_transient() → bool                                  │
│                                                              │
└─────────────────────────────────────────────────────────────┘
```

---

## Provider Priority System

### Priority Sources (in order of precedence)

```
1. Asset's preferred_provider
   │  Stored in Asset.preferred_provider
   │  Overrides all other priorities for that asset
   │
   ▼
2. User-configured priorities
   │  Stored in provider_settings table
   │  Managed via QuoteService.update_provider_settings()
   │  Passed to ProviderRegistry.with_priorities()
   │
   ▼
3. Provider's default priority
   │  Hardcoded in provider implementation
   │  provider.priority() method
   │
   ▼
   Final sorted order for provider selection
```

### Provider Settings

```
┌─────────────────────────────────────────────────────────────┐
│                MarketDataProviderSetting                     │
├─────────────────────────────────────────────────────────────┤
│ id: String           # "YAHOO", "ALPHA_VANTAGE", etc.       │
│ name: String         # Display name                          │
│ description: String                                          │
│ priority: i32        # User-configured priority              │
│ enabled: bool        # Can be disabled                       │
│ url: Option<String>  # Provider website                      │
│ last_synced_at: Option<DateTime>                            │
│ last_sync_status: Option<String>                            │
│ last_sync_error: Option<String>                             │
└─────────────────────────────────────────────────────────────┘
```

---

## Storage Traits

### QuoteStore

```
┌─────────────────────────────────────────────────────────────┐
│                       QuoteStore                             │
├─────────────────────────────────────────────────────────────┤
│ Mutations:                                                   │
│   save_quote(quote) -> Quote                                │
│   delete_quote(id)                                          │
│   upsert_quotes(quotes) -> usize                            │
│   delete_quotes_for_asset(asset_id) -> usize                │
│                                                              │
│ Queries (Strong Types):                                      │
│   latest(asset_id, source?) -> Option<Quote>                │
│   range(asset_id, start, end, source?) -> Vec<Quote>        │
│   latest_batch(asset_ids, source?) -> HashMap<AssetId,Quote>│
│   latest_with_previous(asset_ids) -> HashMap<AssetId,Pair>  │
│                                                              │
│ Legacy Queries (String-based):                               │
│   get_latest_quote(symbol) -> Quote                         │
│   get_historical_quotes(symbol) -> Vec<Quote>               │
│   get_quotes_in_range(symbol, start, end) -> Vec<Quote>     │
└─────────────────────────────────────────────────────────────┘
```

### SyncStateStore

```
┌─────────────────────────────────────────────────────────────┐
│                     SyncStateStore                           │
├─────────────────────────────────────────────────────────────┤
│ get(asset_id) -> Option<QuoteSyncState>                     │
│ get_all() -> Vec<QuoteSyncState>                            │
│ upsert(state)                                               │
│ delete(asset_id)                                            │
│ update_after_sync(asset_id, latest_date, earliest_date)     │
│ get_symbols_needing_sync(grace_period_days) -> Vec<State>   │
└─────────────────────────────────────────────────────────────┘
```

### ProviderSettingsStore

```
┌─────────────────────────────────────────────────────────────┐
│                  ProviderSettingsStore                       │
├─────────────────────────────────────────────────────────────┤
│ get_all_providers() -> Vec<MarketDataProviderSetting>       │
│ get_provider(id) -> MarketDataProviderSetting               │
│ update_provider(id, changes) -> MarketDataProviderSetting   │
└─────────────────────────────────────────────────────────────┘
```

---

## Quote Import/Export

### Import Flow

```
CSV File
    │
    ▼
┌─────────────────┐
│ Parse to        │
│ Vec<QuoteImport>│
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│ QuoteValidator  │
│ validate_batch()│
└────────┬────────┘
         │
    ┌────┴────┐
    ▼         ▼
 Valid    Invalid
    │         │
    │         └──► Return with error status
    ▼
┌─────────────────┐
│ Check for       │
│ duplicates      │
└────────┬────────┘
         │
    ┌────┴────┐
    ▼         ▼
 New      Existing
    │         │
    │    overwrite?
    │    ┌────┴────┐
    │    ▼         ▼
    │   Yes       No
    │    │         │
    │    │         └──► Skip with warning
    │    │
    └────┴────┐
              ▼
┌─────────────────┐
│ QuoteConverter  │
│ to core Quote   │
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│ QuoteStore      │
│ upsert_quotes   │
└─────────────────┘
```

### Quote ID Generation

```
Format: {asset_id}_{YYYY-MM-DD}_{source}

Examples:
  - AAPL_2024-01-15_YAHOO
  - BTC_2024-01-15_ALPHA_VANTAGE
  - EUR/USD_2024-01-15_YAHOO
  - SHOP_2024-01-15_MANUAL

Deterministic: Same inputs always produce same ID
Allows upsert without duplicates
```

---

## Configuration Constants

```
┌─────────────────────────────────────────────────────────────┐
│                      constants.rs                            │
├─────────────────────────────────────────────────────────────┤
│ Date Range Constants:                                        │
│   DEFAULT_HISTORY_DAYS = 1825      # 5 years (fallback)     │
│   QUOTE_HISTORY_BUFFER_DAYS = 45   # Buffer before activity │
│   BACKFILL_SAFETY_MARGIN_DAYS = 7  # Extra margin for check │
│   MIN_SYNC_LOOKBACK_DAYS = 5       # Min lookback for gaps  │
│   QUOTE_LOOKBACK_DAYS = 14         # Gap-fill lookback      │
│   MIN_HISTORICAL_TRADING_DAYS = 20 # Min days before first  │
│   CLOSED_POSITION_GRACE_PERIOD = 30 # Days after position   │
│                                       closes to keep syncing │
│                                                              │
│ Provider Identifiers:                                        │
│   DATA_SOURCE_YAHOO = "YAHOO"                               │
│   DATA_SOURCE_ALPHA_VANTAGE = "ALPHA_VANTAGE"               │
│   DATA_SOURCE_MARKET_DATA_APP = "MARKETDATA_APP"            │
│   DATA_SOURCE_METAL_PRICE_API = "METAL_PRICE_API"           │
│   DATA_SOURCE_FINNHUB = "FINNHUB"                           │
│   DATA_SOURCE_MANUAL = "MANUAL"                             │
│   DATA_SOURCE_CALCULATED = "CALCULATED"                     │
└─────────────────────────────────────────────────────────────┘
```

### Buffer System Explained

The sync system uses a buffer approach to ensure adequate quote coverage:

```
Timeline:
─────────────────────────────────────────────────────────────►

    │◄─── QUOTE_HISTORY_BUFFER_DAYS (45) ───►│
    │◄─ BACKFILL_SAFETY_MARGIN (7) ─►│       │
    │                                 │       │
    ▼                                 ▼       ▼
────┬─────────────────────────────────┬───────┬────────────────►
    │                                 │       │         Today
Required                          First    Activity
Start                            Activity   Dates
(for backfill check)              Date

Total required coverage: first_activity - 52 days (45 + 7)
```

**Why these values:**

| Constant                       | Value | Rationale                                                                                                    |
| ------------------------------ | ----- | ------------------------------------------------------------------------------------------------------------ |
| `QUOTE_HISTORY_BUFFER_DAYS`    | 45    | Accounts for ~8-9 weekend days + holidays per month. Ensures ~20 trading days of data before first activity. |
| `BACKFILL_SAFETY_MARGIN_DAYS`  | 7     | Extra cushion for backfill detection. Prevents edge cases where quotes barely cover the needed range.        |
| `MIN_SYNC_LOOKBACK_DAYS`       | 5     | When syncing active positions, look back at least 5 days to handle weekends/holidays.                        |
| `QUOTE_LOOKBACK_DAYS`          | 14    | Gap-filling operations look back 14 days to find last known quote.                                           |
| `CLOSED_POSITION_GRACE_PERIOD` | 30    | Continue syncing for 30 days after position closes (for late dividends, etc.).                               |

---

## Sync State System

The sync state system tracks what quotes exist and what's needed for each asset.

### QuoteSyncState Fields

```
┌─────────────────────────────────────────────────────────────┐
│                     QuoteSyncState                           │
├─────────────────────────────────────────────────────────────┤
│ asset_id: String           # Asset identifier               │
│ is_active: bool            # Has open position?             │
│                                                              │
│ Activity Tracking:                                           │
│   first_activity_date      # Earliest activity for asset    │
│   last_activity_date       # Most recent activity           │
│   position_closed_date     # When position was closed       │
│                                                              │
│ Quote Coverage:                                              │
│   earliest_quote_date      # Oldest quote we have           │
│   last_quote_date          # Most recent quote              │
│   last_synced_at           # Last successful sync timestamp │
│                                                              │
│ Error Tracking:                                              │
│   error_count              # Consecutive failures           │
│   last_error               # Most recent error message      │
│                                                              │
│ Config:                                                      │
│   data_source              # Preferred provider             │
│   sync_priority            # Priority for sync ordering     │
└─────────────────────────────────────────────────────────────┘
```

### Request Intent, Scheduling, and Completion

Range intent is separate from permission to bypass local coverage:

- `Incremental`, `RefetchRecent`, and `BackfillHistory` select the required
  history window and reuse fresh local coverage. Automatic base-currency
  updates, health-driven work, and portfolio recalculation do not become
  provider-forcing operations merely because they need a full history window.
- The existing `sync_market_data` command with `refetch_all=true` selects
  `RefreshAllHistory` through the shared `MarketSyncMode::for_quote_refresh`
  mapping in both Tauri and Axum. This explicit action bypasses local coverage
  and keeps ordinary provider upsert/revision behavior with manual-price
  precedence. The legacy explicit `resync` / `synch_quotes` /
  `/market-data/sync/history` / addon `syncHistory` aliases select the same
  refresh-all intent; no automatic caller uses that alias. The existing
  recent-window flag still takes precedence if both command flags are supplied.
- Reset Provider History is the other explicit bypass. Its separate command,
  confirmation, validation, and atomic provider-history replacement remain
  unchanged. A refresh-all operation is not a reset, and a projection's
  `force_full` flag is not a market-data bypass.

Startup, resume, manual requests, and periodic work retain their existing
entrypoints and backend coordinator. The periodic worker starts its own interval
baseline when it launches. After the startup delay it checks the configured
interval, instead of unconditionally performing another startup sync. Settings
are still checked every minute, and subsequent periodic attempts wait from their
own completion. This is not a global last-completion clock: page visits and
other local no-op jobs cannot continually postpone the periodic interval.
Overlapping requests still enter the existing coordinator; local coverage is
rechecked after acquiring the history lock, rather than dropping a request that
might cover another asset, another range, or an explicit refresh.

The sync, update, and recalculate commands acknowledge background-job admission,
not completed writes. Their frontend mutation hooks therefore do not invalidate
queries on success. The shared completion listener refreshes data after the
job's persistence and reconciliation have finished. Both hosts include an
optional `changed` verdict on the portfolio completion event:

- `false`: confirmed no price or projection changes; clear the progress
  indicator and refresh only database-backed health/provider status. Performance
  and benchmark queries are not invalidated, so health recovery cannot create a
  market-fetch feedback cycle.
- `true`: completed price writes or a successful nonempty projection plan;
  refresh the affected portfolio queries.
- `null` or an absent field: an uncertain partial-write outcome or an older
  host; retain the conservative refresh behavior.

Failed jobs still report errors, and a job that wrote prices before a later
projection failure is never reported as a confirmed no-op. The observer's
backward-compatible default callback preserves existing non-UI observers.

**Architecture impact:** provider request planning and periodic first-run timing
are intentionally optimized. The existing provider registry, transport/API
entrypoints, background-job owner, pricing precedence, reset transaction, and
portfolio calculation rules remain in place; there is no new queue or scheduler.
The completion payload is a small additive event change, compatible with callers
that do not consume it.

### Local Query Outcomes and Benchmark History

Ordinary history reads and symbol-price resolution share the existing
`QuoteSyncService` owner. Pure database getters remain pure. Each profile owns a
weak per-asset lock registry: requests for the same asset wait and recheck local
query outcomes, while different assets retain existing concurrent fetching.
Waiting does not discard another requested range or an explicit force action.

SQLite stores small query receipts in `market_history_queries`, separate from
`QuoteSyncState.last_synced_at`. Receipts contain a digested
canonical/configuration identity, asset ID, inclusive query window, outcome and
fetch time. Neither quote min/max bounds nor missing calendar days establish
complete coverage. Existing prices from an older application version have no
receipts: the first ordinary sync may conservatively validate its existing
incremental window once. It does not infer complete history from old rows,
expand that incremental request to all history, or erase the stored prices.
Persisted receipts then allow repeated views and restarts to reuse that work.
Provider authentication, fallback, parsing and disable rules remain in the
existing registry. Providers explicitly attest complete range results; bounded
or otherwise uncertified successful replies are partial observations:

- Complete historical prefixes can be reused durably. The existing five-day
  correction overlap remains mutable when its market session is not settled.
- An open/current session rechecks using the existing configured market sync
  interval (default six hours), not a new polling timer. The existing market
  timezone/close-grace helpers distinguish settled sessions from intraday data.
  A session closing after an intraday fetch makes that mutable tail due again; a
  settled Friday does not repeatedly expire over the weekend.
- Empty and partial observations are reused only for the configured interval.
  Partial observations never become durable historical coverage or settled
  session proof. Newly requested, unqueried ranges are still fetched.
- Errors and authentication failures do not create receipts. Future dates are
  not certified; latest responses with future quote dates are rejected.

Fetched prices and the query receipt commit atomically. The transaction checks
that the asset/provider context has not changed since fetching. Query commits
skip financially identical provider rows and report actual changed-price counts;
ordinary bulk-upsert and atomic reset semantics are unchanged. Direct service
read-fills emit the existing `PriceHistoryChanged` event once for actual
canonical writes, including committed earlier ranges if a later range fails;
existing planners rebuild locally without recursively fetching market data.
Derived-only writes and local hits emit no financial event. Coordinator-owned
sync retains its existing completion flow. Successful provider recovery can
still heal sync-health state without forcing portfolio price invalidation.
Manual prices keep their existing precedence, and existing provider rows are not
deleted merely because preferences change.

A benchmark that is absent from the user's asset catalog uses isolated
`benchmark_history` rows in the same profile database. It never creates a fake
asset or appears in normal quote lists. Prices for existing assets continue to
use the original quotes table. Benchmark and ordinary history both survive a
service restart; changing canonical identity, currency or provider configuration
invalidates reuse assumptions. Destructive quote changes invalidate receipts.
Only digests of non-secret fetch configuration are stored in query identity;
credentials and raw request URLs are not persisted there. Explicit receipt
invalidation also clears the associated derived benchmark rows atomically, never
user quotes. Custom-provider mutations invalidate these derived caches through
SQLite triggers, including device-sync writes, matching the conservative global
custom-configuration revision. Asset deletion removes associated derived rows
before discarding their receipt mapping.

With an available profile context, both hosts notify the quote service after
successful writes/deletions of a known market-provider credential or a
credential referenced by a custom source header. The service rebuilds the
existing client and invalidates query receipts without hashing secret values;
unrelated application secrets do not trigger market work. Credential storage and
client reload are not one transaction: a reload/validation failure is explicitly
reported as occurring after the credential changed. The frontend does not issue
a duplicate settings refresh for the same credential write. Tauri preserves
generic secret writes while the profile database is unavailable; reopening
constructs a new client from current secrets. Immediate receipt invalidation
cannot run without that database. Credential-backed provider responses,
secret-header custom history, and a public fallback after the request's
credential-dependent primary fails therefore remain bounded Partial
observations, not indefinitely reusable authenticated history. Public Yahoo,
Eastmoney and unauthenticated custom history retain durable restart reuse;
unrelated enabled credential providers do not downgrade those public requests.
Normal market settings require an available profile context.

Explicit provider verification remains distinct from the manual-priority display
price: it reuses compatible local provider proof or the actual response of one
necessary latest fetch, never relabeling a manual price as provider-supplied.
When a manual override prevents storing provider validation in the canonical
price row, that validation payload uses the existing isolated derived-cache
namespace and canonical asset lock. It does not replace or duplicate the
canonical manual price, create catalog assets, or add another cache subsystem.
Unlike canonical user/provider price history, these derived response-cache rows
replace history rows only inside a successfully fetched interval. Partial or
empty new responses cannot silently retain older rows absent from that response;
history rows outside the interval remain available. Latest-only derived results
instead replace their prior snapshot, even when the newly returned observation
is dated earlier. Fetch failures leave the derived cache untouched, and Partial
receipts still expire rather than certify completeness. Current weekend
observations can use bounded latest-only Partial receipts. An observation whose
timestamp cannot safely establish an observable range is returned without
caching, including conservatively handled market-local date-only values ahead of
UTC; it cannot create future or durable historical coverage.

### Sync Categories

The system categorizes each asset to determine sync behavior:

```
┌─────────────────────────────────────────────────────────────┐
│                   Sync Category Flow                         │
└─────────────────────────────────────────────────────────────┘

Has activities but no quotes?
         │
         ├─ Yes ──► NEW (priority: 80)
         │          Fetch from: first_activity - buffer
         │          Fetch to: today
         │
         No
         │
         ▼
Needs backfill?
(first_activity - buffer - margin < earliest_quote)
         │
         ├─ Yes ──► NEEDS_BACKFILL (priority: 70)
         │          Fetch from: first_activity - buffer
         │          Fetch to: earliest_quote
         │
         No
         │
         ▼
Has open position? (is_active = true)
         │
         ├─ Yes ──► ACTIVE (priority: 100)
         │          Fetch from: last_quote + 1 day
         │          Fetch to: today
         │
         No
         │
         ▼
Closed within grace period?
         │
         ├─ Yes ──► RECENTLY_CLOSED (priority: 50)
         │          Fetch from: last_quote + 1 day
         │          Fetch to: today
         │
         No
         │
         ▼
         CLOSED (priority: 0)
         No sync needed
```

### Date Range Calculation

Each category calculates its fetch range differently:

| Category         | Start Date                 | End Date         |
| ---------------- | -------------------------- | ---------------- |
| `New`            | `first_activity - 45 days` | today            |
| `NeedsBackfill`  | `first_activity - 45 days` | `earliest_quote` |
| `Active`         | `last_quote + 1 day`       | today            |
| `RecentlyClosed` | `last_quote + 1 day`       | today            |
| `Closed`         | (no fetch)                 | (no fetch)       |

### State Refresh Process

The sync system automatically maintains accurate state:

```
refresh_sync_states()
         │
         ├─► 1. Create sync states for new syncable assets
         │      (assets with pricing_mode != Manual)
         │
         ├─► 2. Refresh activity dates from activities table
         │      UPDATE quote_sync_state SET
         │        first_activity_date = MIN(activities.date),
         │        last_activity_date = MAX(activities.date)
         │
         └─► 3. Refresh earliest_quote_date from quotes table
               UPDATE quote_sync_state SET
                 earliest_quote_date = MIN(quotes.day)

This ensures:
- Activity dates always reflect actual activities
- Quote coverage dates always reflect actual quotes
- New assets are discovered and tracked
```

### Backfill Detection

Backfill is triggered when there aren't enough historical quotes:

```
Required start = first_activity - BUFFER - MARGIN
               = first_activity - 45 - 7
               = first_activity - 52 days

If required_start < earliest_quote_date:
  → Needs backfill (missing historical data)

If earliest_quote_date is NULL:
  → Needs backfill (no quotes at all)
```

**Example:**

```
Asset: TSLA
first_activity_date: 2025-11-07
earliest_quote_date: 2026-01-05  ← Only recent quotes!

Required start: 2025-11-07 - 52 = 2025-09-16

2025-09-16 < 2026-01-05? YES → NeedsBackfill

System will fetch quotes from 2025-09-16 to 2026-01-05
```

---

## Design Decisions

### 1. Two-Crate Architecture

**Decision:** Separate `market-data` crate from `core` crate.

**Rationale:**

- `market-data` has no knowledge of application domain (assets, portfolios)
- Can be reused in other projects
- Clear API boundary enforces separation of concerns
- Easier to test providers in isolation

### 2. Provider Registry Pattern

**Decision:** Centralized registry orchestrates all provider interactions.

**Rationale:**

- Single point for cross-cutting concerns (rate limiting, circuit breaking)
- Consistent provider selection logic
- Easy to add new providers
- Diagnostic tracking for debugging

### 3. Symbol Resolution Chain

**Decision:** Separate symbol resolution from provider implementation.

**Rationale:**

- Providers don't need to know about other providers' formats
- Rules can be updated without changing providers
- Supports provider-specific overrides in assets
- Handles exchange MIC → provider suffix mapping

### 4. Explicit Error Semantics

**Decision:** Preserve error types across crate boundaries.

**Rationale:**

- Higher layers can make informed retry decisions
- Better error messages for users
- Enables smart fallback behavior
- Distinguishes terminal vs transient errors

### 5. Quote Ordering Guarantee

**Decision:** Sort quotes by timestamp after fetching.

**Rationale:**

- Providers don't guarantee order
- Sync logic relies on first()/last() for date ranges
- Small overhead for correctness guarantee
- Prevents subtle bugs in portfolio calculations

### 6. User-Configurable Priority

**Decision:** Allow users to override provider priority.

**Rationale:**

- Different users have different provider preferences
- Some users have paid API keys for specific providers
- Regional differences (US vs international markets)
- Easy to experiment with provider quality

---

## Future Considerations

1. **Real-time Quotes**: WebSocket connections for live prices
2. **Quote Caching**: In-memory cache for frequently accessed quotes
3. **Batch Optimization**: Fetch multiple symbols in single API call
4. **Provider Health Dashboard**: UI for monitoring provider status
5. **Custom Provider Plugin**: Allow users to add their own providers
