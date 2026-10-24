# Tonghuashun iFinD market data

`IFIND` is an independent Rust market-data provider shared by Tauri and Axum.
Its internal transport uses the existing `rmcp` 2.1 SDK's Streamable HTTP
client; it does not expose an MCP server or require an AI model, Node or Python
runtime.

Enable it in Settings → Market Data and enter an iFinD MCP key from
<https://mcp.51ifind.com/#/profile>. The new migration appends a disabled
settings record without changing existing providers, assets or quotes. Keys stay
in the existing `SecretStore`. Saving, replacing or deleting an IFIND key
refreshes the existing market-data client through the settings API. Environment
variable `THS_KEY` is used only by the explicit acceptance executable below.

## Calls and identity

Authentication sends the original key in `Authorization`, without `Bearer`.
Connections initialize lazily per official service and retain sessions in
memory. All tool calls share a serial gate and at least 500 ms between start
times. This is necessary because the registry token bucket does not bound
in-flight calls. SDK automatic session-expiry replay is disabled. The provider
does not add retries or borrow data from other providers. Errors go back to the
existing registry.

| Category     | Tools                                                                                     | Stored IFIND override example      |
| ------------ | ----------------------------------------------------------------------------------------- | ---------------------------------- |
| A shares     | `search_stocks`, `get_stock_info`, `stock_highfreq_quotes`, `get_stock_performance`       | `stock:600519.SH`                  |
| Listed funds | `search_funds`, `get_fund_profile`, `fund_highfreq_quotes`, `get_fund_market_performance` | `fund:510300.SH`                   |
| OTC funds    | `search_funds`, `get_fund_profile`, `get_fund_market_performance`                         | `nav:005827.OF`                    |
| Indices      | `index_data`, `index_highfreq_quotes`                                                     | `index:000300.SH`                  |
| HK/US shares | `search_global_stocks`, `global_stock_profile`, `global_stock_quotes`                     | `global:00700.HK`, `global:AAPL.O` |

Categories are encoded inside the existing provider-symbol override rather than
adding public fields or tables. Canonical tickers/MICs remain unchanged. HK
codes with different leading-zero lengths compare as the same listing. Ambiguous
index/stock identities require an explicit override. Local resolution makes no
network requests. Explicit `FUND:005827` and exchange-qualified searches such as
`XSHG:600519`, `XSHE:161725`, `XHKG:00700` and `XNAS:AAPL` use the same local
resolver and a single profile lookup. Unsupported or ambiguous qualified codes
never fall back to a fuzzy search. Internal listed-fund symbols (`fund:...`)
remain distinct from the public `FUND:` NAV namespace. Search results are
checked against a category-specific profile before being returned; partial
market failures do not discard other successes.

The service returns real-time tables and Markdown tables wrapped in JSON text.
Parsing uses columns, security identity, currency and adjustment metadata rather
than positional guesses. Missing/changed fields and explicit truncation are
errors. Historical calls use fixed windows of at most 30 calendar days. Index
levels need no corporate-action adjustment; other traded instruments must report
unadjusted price metadata. Historical volume is accepted only with verified
share units. Optional real-time volume is omitted because its units are not
specified. Upstream non-trading-day rows with forward-filled closes and no OHLC
or volume are excluded.

OTC prices are published unit NAV, never cumulative NAV or IOPV. HK/US latest
prices use the latest reported unadjusted close and its actual trading date;
these tools do not promise an intraday quote. Dates of daily bars follow the
existing UTC day-bucket convention. Real-time Chinese timestamps use Shanghai
time. Names and quote currencies edited by the user retain existing enrichment
precedence.

## Architecture impact

Only the outgoing source and internal MCP session lifecycle are new. Existing
background synchronization, quote persistence, events, manual-price precedence,
provider priority and failure isolation remain in their owning layers. No
persistent sessions, background workers or general MCP management layer are
added. News, financial statements, dividends, bonds and derivatives are outside
scope.

## Validation

Normal unit and E2E tests are offline and never read `THS_KEY`. Parser fixtures
contain public instrument samples captured on 2026-10-23; local MCP tests cover
handshake, raw authorization, shared pacing, session reuse and no
expired-request replay. E2E substitutes `FixtureProvider` and exercises creation
and persisted quotes with IFIND alone in LTR and RTL.

Run live acceptance explicitly, with command tracing disabled:

```sh
set +x
source ~/.zshrc
cargo run --locked -p wealthfolio-market-data --example ifind_check
```

The executable reads `THS_KEY` from its environment, prints only scenario
results and sanitized errors, and does not save the key. It is not invoked by
tests or CI. A successful short sample does not prove exhaustive historical
coverage; missing upstream trading rows cannot be inferred from a Markdown table
without a trading calendar. Capability and real-world acceptance results must be
reported separately.

Official references:

- [Installation guide](https://mcp.51ifind.com/gwstatic/static/ds_web/ifind-mcp-web/skills/SKILL_INSTALL_GUIDE.md)
- [Official skill package](https://s.thsi.cn/cd/ifind-java-ds-bff-web-container/ifind-mcp-web/skills/ifind-finance-data-1.5.0.zip)
- [Rust MCP SDK](https://github.com/modelcontextprotocol/rust-sdk/tree/rmcp-v2.1.0)

## Acceptance record (2026-10-23)

Live Rust-provider acceptance passed profiles, latest published prices and five
historical trading rows (2026-10-12 through 2026-10-16) for `600519.SH`,
`000001.SZ`, `920002.BJ`, `510300.SH`, `161725.SZ`, `005827.OF`, `000300.SH`,
`00700.HK` and `AAPL.O`. Name searches for an A share, OTC fund, index, HK share
and US share also passed. These are public samples, not exhaustive market or
historical-calendar coverage.
