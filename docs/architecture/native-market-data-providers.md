# Native securities market data

Wealthfolio registers `EASTMONEY`, `TENCENT`, and `SINA` as independent Rust
`MarketDataProvider` implementations. Both Tauri and Axum call the same code.
Neither Python, AKShare/efinance installations, nor a separately deployed
finance-data-api service is required.

## Configuration and identity

Enable the sources in Settings → Market Data and order them with the existing
priority controls. They have no API-key or service-URL setting. New
installations leave them disabled. Search accepts names, ordinary tickers and
exact qualified identifiers such as `XSHG:600519`, `XSHE:161725`, and
`FUND:005827`.

Stocks retain their canonical ticker and MIC. `FUND:` retains its existing
meaning; listed and OTC shares of the same fund are distinct instruments. Native
request identifiers live in asset overrides when they cannot be derived locally.
Search results from these sources do not automatically set a preferred provider.
An explicit user preference still sorts first, but is not an exclusive
assignment. Custom scraper assignments remain exclusive.

Source-specific market groups are not always individual exchanges: Tencent's
`.AM` and Eastmoney's market `107` also contain ETFs. Search does not invent an
exact MIC for those groups. Existing explicit MICs remain unchanged. The
resolver never adds a network dependency to determine an unknown venue.

Known global index aliases (`^GSPC`, `^DJI`, `^IXIC`, `^HSI`) map explicitly to
each source. Domestic exchange indices retain ticker plus MIC. Unknown aliases
are not guessed from display names. Explicit native overrides can identify
instruments whose venue is not yet covered by the local conversion rules.

## Native endpoints and contracts

| Source        | Operation          | Endpoint family                                                                                       | Contract                                                                                                  |
| ------------- | ------------------ | ----------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------- |
| Eastmoney     | Search             | `search-codetable.eastmoney.com/codetable/search/web`                                                 | Provider market ID and security classification; exact qualified queries are filtered by identity          |
| Eastmoney     | Latest             | `push2.eastmoney.com/api/qt/stock/get`                                                                | `fltt=2`, upstream timestamp and native quote currency                                                    |
| Eastmoney     | History            | `push2his.eastmoney.com/api/qt/stock/kline/get`                                                       | Daily, `fqt=0`, yearly windows below the row limit                                                        |
| Tiantian Fund | Identity           | `fundmobapi.eastmoney.com/FundMNewApi/FundMNNBasicInformation`                                        | Exact fund code, category and share-class name; no full-market directory download                         |
| Tiantian Fund | NAV                | `api.fund.eastmoney.com/f10/lsjz`                                                                     | Published `DWJZ`, 20-row pages (the endpoint clamps/rejects larger page sizes)                            |
| Eastmoney     | News               | `search-api-web.eastmoney.com/search/jsonp`                                                           | Ten search results with URL, summary, publisher and publication time; no article scraping                 |
| Eastmoney     | Statements/actions | `datacenter.eastmoney.com/securities/api/data/v1/get`, `datacenter-web.eastmoney.com/api/data/v1/get` | Paginated native reports; implemented domestic cash distributions and share bonuses                       |
| Tencent       | Search             | `smartbox.gtimg.cn/s3/`                                                                               | Market and native exchange suffix; listed ETF/LOF types preserved                                         |
| Tencent       | Latest             | `qt.gtimg.cn`                                                                                         | Quote namespace differs from historical US identifiers; check returned listing suffix                     |
| Tencent       | History            | `proxy.finance.qq.com/ifzqgtimg/appstock/app/newfqkline/get`                                          | Use unadjusted `day` only; US history requires the native exchange suffix; filter yearly windows          |
| Sina          | Search/latest      | `suggest3.sinajs.cn`, `hq.sinajs.cn`                                                                  | GBK/UTF-8, listing-local trade timestamps, not the US feed update timestamp                               |
| Sina          | History            | `finance.sina.com.cn/realstock/company/...`, `/stock/hkstock/...`, `/staticdata/us/...`               | Fixed Rust decoder for daily formats 1479 and 3466; never evaluates JavaScript                            |
| Sina          | Statements         | `quotes.sina.cn/.../CompanyFinanceService.getFinanceReport2022`                                       | Native domestic financial statements, compact publication dates                                           |
| Sina          | Actions            | `vip.stock.finance.sina.com.cn/corp/go.php/vISSUE_ShareBonus/stockid/...`                             | Implemented distributions only; cash per ten shares divided by ten; no rights offerings treated as splits |

The adapters cover resolvable domestic, Hong Kong and US securities, with
Eastmoney also mapping London listings. Coverage is determined by native
identity and operation support, not by a rule that restricts requests to Chinese
assets. A provider without a supported operation returns `NotSupported`; it
never borrows another provider's data. Tencent research/corporate-action methods
retain that explicit unsupported result.

Listed securities use current or delayed native quotes and unadjusted daily
history. Volume is normalized per endpoint/market: mainland Eastmoney bars are
lots, Tencent mainland daily bars have separate STAR/index conventions, and Sina
decoded bars are already shares. Foreign securities are not multiplied by 100.
Money-market fund income and cumulative/estimated NAV are not unit prices.
Explicit foreign-currency share-class denominations are retained; ambiguous fund
currency names are rejected instead of being silently treated as CNY.

News and financial statements are callable only at the provider layer. They have
no persistence, scheduler, command/route or frontend consumer. Statements retain
native fiscal-period labels, currencies/units where supplied, and raw accounting
item keys; annual and quarterly statements ending on the same date are separate.

## Architecture impact and failure behavior

Backend requests now go directly to the selected source. Existing registry
priorities, rate limits, circuit breakers, background jobs and events are
reused. A source's `SymbolNotFound` now means try the next eligible source; it
does not prove the instrument is unavailable everywhere. Malformed native
responses are reported as provider errors, so another eligible source can be
attempted. No adapter implements a hidden cross-provider fallback or additional
retry loop.

Profile enrichment fills missing names and preserves the configured name and
quote currency independently of the selected source. This protection applies
also after an HTTP asset loses its old preferred-provider field.

## One-time migration

Migration `20261023000001` runs offline through the existing migration runner
and pre-migration backup flow. In one transaction it:

- replaces HTTP's position with Eastmoney, Tencent, Sina, in that order;
- inherits HTTP's enabled state and preserves other providers' relative order;
- removes HTTP preferences/overrides, preserving other JSON configuration;
- clears affected sync source/error state without changing lifecycle or
  timestamps;
- removes the HTTP settings/URL record.

Asset IDs, identity fields, manual-update mode, transactions and existing quotes
are preserved. Historical `HTTP_API` provenance is not relabeled. Same-day reads
prefer manual quotes, ordinary providers, legacy HTTP, then broker prices. New
provider quotes do not hide manual overrides. The migration does not contact any
provider or force a startup history reset. Later user settings are not reapplied
on restart. The obsolete key remains unread in the existing secure secret store.

There is intentionally no `down.sql`: deleted source assignments cannot be
reconstructed. Restore the pre-migration backup to roll back. Importing an older
backup applies this same migration to the private import candidate. Do not mix
clients that still write HTTP bindings with the migrated version.

## Validation

Source references: AKShare `fe10d1d`, efinance `c8fd370`. Sina decoder fixtures
were captured from public Shanghai, Hong Kong and US securities on 2026-10-23.
The expected row counts and sample OHLCV values were generated with AKShare's
fixed local reference decoder, not by the Rust implementation. Attribution is in
`crates/market-data/src/provider/sina/THIRD_PARTY_LICENSES.md`.

Offline tests cover parsing, source identity, NAV currency, index aliases,
fiscal-period separation, decoder parity, provider ordering, reserved fund
namespaces, migration rollback/restart, and historical quote precedence.
`e2e/36-native-market-data.spec.ts` covers settings, automatic creation,
persisted quotes and local search in both directions, with provider fixtures.

Opt-in public endpoint checks (no user financial data or credentials):

```sh
cargo test --locked -p wealthfolio-market-data --test native_live -- --ignored --nocapture
```

Live checks report per-operation results separately from offline tests. Network
availability is not guaranteed by a parser fixture or a successful HTTP status.

### Public sample results (2026-10-23)

| Source    | Successful latest/history samples                                                                                                                           | Other verified operations                                                                            | Failed / unverified                                                                                                                                                                                         |
| --------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Eastmoney | Latest: Shanghai stock, ETF, Shenzhen LOF, Beijing stock, Hong Kong/US/London stocks, Shanghai and S&P 500 indices. Published fund NAV: latest and history. | Search; news; A/HK/US balance sheet, income and cash-flow reports; domestic dividends/share bonuses. | Securities daily-history HTTPS connections were closed by the upstream server in the final run, also reproduced with an independent client. Other markets and currencies not listed here remain unverified. |
| Tencent   | Latest and unadjusted history: Shanghai/Beijing stocks, Shenzhen LOF, ETF, Hong Kong/US stocks, Shanghai and S&P 500 indices.                               | Search.                                                                                              | News, financial reports and corporate actions are not declared supported. Other markets remain unverified.                                                                                                  |
| Sina      | Latest and decoded unadjusted history: Shanghai/Beijing stocks, Shenzhen LOF, ETF, Hong Kong/US stocks, Shanghai and S&P 500 indices.                       | Search; A-share three statements; domestic dividends/share bonuses.                                  | OTC fund NAV and news are not declared supported. Other markets remain unverified.                                                                                                                          |

These checks assert nonempty rows, positive prices, currencies and requested
date ranges. They are samples, not a guarantee of future upstream availability
or an exhaustive verification of every exchange/denomination. Eastmoney history
errors remain visible and allow the registry to try other enabled sources.
