# Standard HTTP API provider

`HTTP_API` connects Wealthfolio to a self-hosted service implementing the
finance-data-api protocol. It supports exact instrument lookup, profiles, latest
daily prices and historical prices. Requests originate from the Rust backend, in
both web and Tauri runtimes.

## Configuration and use

1. Open Settings → Market Data → Standard HTTP API.
2. Save the service base URL, optionally save its API key, then enable it.
3. Enter a qualified identifier in an existing security search and select the
   result to create the asset through the normal application flow.

For containers on the same Docker network use `http://finance-data-api:8000`.
For a desktop app connecting to a local service use `http://127.0.0.1:8000`. The
browser does not need access to this address. Separate Compose projects must
join the same network, for example:

```yaml
services:
  wealthfolio: # merge into the existing Wealthfolio service
    networks: [investment]
  finance-data-api: # merge into the existing data service
    networks: [investment]
networks:
  investment:
    external: true
    name: investment
```

Create the external network once with `docker network create investment`. Keep
the existing service definitions, environment and storage configuration.

The URL is stored as `{"baseUrl":"http://finance-data-api:8000"}` in the
existing provider `config` column. The optional key uses the `HTTP_API`
SecretStore entry and is sent in `X-API-Key`. It is never stored in provider
config. Requests time out after 15 seconds and do not follow redirects. Saving
settings or credentials refreshes the provider without restarting.

## Protocol v1

| Operation                | GET path                                                          |
| ------------------------ | ----------------------------------------------------------------- |
| Exact lookup and profile | `/api/v1/instruments/{symbol}`                                    |
| Latest daily price       | `/api/v1/quotes/{symbol}/latest`                                  |
| Daily history            | `/api/v1/quotes/{symbol}/history?start=YYYY-MM-DD&end=YYYY-MM-DD` |

All responses use the existing finance-data-api envelope. `provider` names that
service's upstream source; Wealthfolio attributes stored prices to `HTTP_API`.
`fetched_at` describes retrieval time, never the valuation date.

Example instrument:

```json
{
  "data": {
    "symbol": "FUND:005827",
    "code": "005827",
    "name": "易方达蓝筹精选混合",
    "asset_type": "fund",
    "market": "FUND",
    "currency": "CNY"
  },
  "provider": "efinance",
  "price_type": "unit_nav",
  "fetched_at": "2024-01-04T10:00:00+08:00"
}
```

Supported asset types are `stock`, `etf`, `lof` and `fund`. Exchange-traded
instruments use an ISO MIC namespace; off-exchange funds use `FUND`. The current
finance-data-api supports `XSHG`, `XSHE` and `FUND`, with six-digit codes.
Leading zeroes are significant. Wealthfolio never infers the market from a bare
code.

Price `data` contains `symbol`, `date` and decimal-string `close`, with nullable
`open`, `high`, `low` and `volume`. Exchange-traded prices use
`price_type=close`; off-exchange funds use `unit_nav`. Following
finance-data-api, traded volume is in lots of 100, converted to units by this
provider. History uses an array in ascending, unique date order, including both
requested boundary dates. Empty history is valid. Prices must be positive;
volume must be nonnegative. Dates are stored as their original calendar day at
UTC midnight.

404 means no instrument/data. A 422 unsupported identifier allows lookup to
continue through the existing search providers. Authentication errors, timeouts,
malformed responses and mismatched identities remain errors. There is no
additional HTTP retry layer.

## Identity and execution boundaries

| External identifier | Canonical symbol | MIC    |
| ------------------- | ---------------- | ------ |
| `XSHG:600519`       | `600519`         | `XSHG` |
| `XSHE:000001`       | `000001`         | `XSHE` |
| `FUND:005827`       | `FUND:005827`    | none   |

The full external identifier is retained in the existing provider override. This
reuses exchange-traded assets created through other providers, while keeping an
LOF's exchange-traded and off-exchange units distinct.

**Architecture impact:** only explicit qualified lookups and assets assigned to
`HTTP_API` call this service. Qualified lookup results are authoritative even
without a MIC. An exact local match avoids external lookup. Name and partial
code search reuse the existing SQLite search and do not call the HTTP service.
Search does not persist assets. Existing creation, events, enrichment and quote
sync handle persistence and background work. Enrichment preserves saved user
names, currencies and bindings. Assigned HTTP assets do not fall back to other
price or profile providers when this service fails.

The provider is disabled on migration. Existing custom-provider assets are not
automatically rebound. There is one configured HTTP service, no new search
index, external fuzzy search or import-specific resolution. Daily prices alone
do not account for dividends, splits or fund share conversions.
