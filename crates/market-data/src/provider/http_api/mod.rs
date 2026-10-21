//! Standard HTTP market-data protocol, compatible with finance-data-api.

use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, NaiveDate, TimeZone, Utc};
use reqwest::{header, StatusCode, Url};
use rust_decimal::Decimal;
use serde::{de::DeserializeOwned, Deserialize};

use crate::{
    errors::MarketDataError, is_known_mic, AssetProfile, Coverage, InstrumentKind,
    MarketDataProvider, ProviderCapabilities, ProviderInstrument, Quote, QuoteContext, RateLimit,
    SearchResult,
};

pub const DATA_SOURCE_HTTP_API: &str = "HTTP_API";
const MAX_RESPONSE_BYTES: usize = 8 * 1024 * 1024;

/// Qualified identifiers are explicit; bare codes must never imply a market.
pub fn qualified_symbol(symbol: &str) -> Option<(&str, &str)> {
    let (market, code) = symbol.split_once(':')?;
    if (market != "FUND" && !is_known_mic(market))
        || market.is_empty()
        || code.is_empty()
        || !market
            .bytes()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
        || !code
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
    {
        return None;
    }
    Some((market, code))
}

fn invalid(message: &str) -> MarketDataError {
    MarketDataError::ValidationFailed {
        message: message.into(),
    }
}

pub fn validate_base_url(value: &str) -> Result<Url, MarketDataError> {
    let url = Url::parse(value).map_err(|_| invalid("Invalid HTTP API base URL"))?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(invalid(
            "Base URL must be HTTP(S), without credentials, query or fragment",
        ));
    }
    Ok(url)
}

pub struct HttpApiProvider {
    base_url: Url,
    client: reqwest::Client,
}

#[derive(Deserialize)]
struct Envelope<T> {
    data: T,
    price_type: String,
}

#[derive(Deserialize)]
struct Instrument {
    symbol: String,
    code: String,
    name: String,
    asset_type: String,
    market: String,
    currency: String,
}

impl Instrument {
    fn validate(&self, requested: &str) -> Result<(), MarketDataError> {
        let Some((market, code)) = qualified_symbol(requested) else {
            return Err(invalid("Expected a qualified instrument identifier"));
        };
        if self.symbol != requested
            || self.code != code
            || self.market != market
            || (self.asset_type == "fund") != (self.market == "FUND")
            || (self.market != "FUND" && !is_known_mic(&self.market))
            || self.name.trim().is_empty()
            || self.currency.len() != 3
            || !self.currency.bytes().all(|c| c.is_ascii_uppercase())
        {
            return Err(invalid("Invalid or mismatched instrument identity"));
        }
        self.quote_type()?;
        Ok(())
    }

    fn quote_type(&self) -> Result<&'static str, MarketDataError> {
        match self.asset_type.as_str() {
            "stock" => Ok("EQUITY"),
            "etf" | "lof" => Ok("ETF"),
            "fund" => Ok("MUTUALFUND"),
            _ => Err(invalid("Unsupported HTTP API asset type")),
        }
    }
}

#[derive(Deserialize)]
struct Price {
    symbol: String,
    date: NaiveDate,
    close: Decimal,
    open: Option<Decimal>,
    high: Option<Decimal>,
    low: Option<Decimal>,
    volume: Option<Decimal>,
}

impl Price {
    fn into_quote(self, symbol: &str, currency: &str) -> Result<Quote, MarketDataError> {
        if self.symbol != symbol
            || self.date > Utc::now().date_naive()
            || self.close <= Decimal::ZERO
            || [self.open, self.high, self.low]
                .into_iter()
                .flatten()
                .any(|v| v <= Decimal::ZERO)
            || self.volume.is_some_and(|v| v < Decimal::ZERO)
        {
            return Err(invalid("Invalid HTTP API price or date"));
        }
        let timestamp =
            Utc.from_utc_datetime(&self.date.and_hms_opt(0, 0, 0).expect("midnight is valid"));
        Ok(Quote {
            timestamp,
            open: self.open,
            high: self.high,
            low: self.low,
            close: self.close,
            volume: self
                .volume
                .map(|volume| {
                    volume
                        .checked_mul(Decimal::from(100))
                        .ok_or_else(|| invalid("HTTP API volume exceeds supported precision"))
                })
                .transpose()?,
            currency: currency.into(),
            source: DATA_SOURCE_HTTP_API.into(),
        })
    }
}

impl HttpApiProvider {
    pub fn new(base_url: &str, api_key: Option<String>) -> Result<Self, MarketDataError> {
        let mut headers = header::HeaderMap::new();
        if let Some(key) = api_key.filter(|key| !key.is_empty()) {
            let mut value = header::HeaderValue::from_str(&key)
                .map_err(|_| invalid("Invalid API key header"))?;
            value.set_sensitive(true);
            headers.insert("X-API-Key", value);
        }
        let client = wealthfolio_http::client_builder()
            .timeout(Duration::from_secs(15))
            .redirect(reqwest::redirect::Policy::none())
            .default_headers(headers)
            .build()
            .map_err(MarketDataError::Network)?;
        Ok(Self {
            base_url: validate_base_url(base_url)?,
            client,
        })
    }

    async fn get<T: DeserializeOwned>(
        &self,
        parts: &[&str],
        query: &[(&str, String)],
    ) -> Result<Envelope<T>, MarketDataError> {
        let mut url = self.base_url.clone();
        url.path_segments_mut()
            .map_err(|_| invalid("Invalid base URL"))?
            .pop_if_empty()
            .extend(["api", "v1"])
            .extend(parts.iter().copied());
        let mut response = self
            .client
            .get(url)
            .query(query)
            .send()
            .await
            .map_err(|error| {
                if error.is_timeout() {
                    MarketDataError::Timeout {
                        provider: DATA_SOURCE_HTTP_API.into(),
                    }
                } else {
                    MarketDataError::ProviderError {
                        provider: DATA_SOURCE_HTTP_API.into(),
                        message: "HTTP request failed".into(),
                    }
                }
            })?;
        match response.status() {
            StatusCode::UNPROCESSABLE_ENTITY => {
                return Err(MarketDataError::NotSupported {
                    operation: "HTTP API identifier".into(),
                    provider: DATA_SOURCE_HTTP_API.into(),
                })
            }
            StatusCode::NOT_FOUND => {
                return Err(MarketDataError::SymbolNotFound(
                    "HTTP API instrument".into(),
                ))
            }
            StatusCode::TOO_MANY_REQUESTS => {
                return Err(MarketDataError::RateLimited {
                    provider: DATA_SOURCE_HTTP_API.into(),
                })
            }
            StatusCode::GATEWAY_TIMEOUT => {
                return Err(MarketDataError::Timeout {
                    provider: DATA_SOURCE_HTTP_API.into(),
                })
            }
            status if !status.is_success() => {
                return Err(MarketDataError::ProviderError {
                    provider: DATA_SOURCE_HTTP_API.into(),
                    message: format!("HTTP {status}"),
                })
            }
            _ => {}
        }
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(MarketDataError::Network)? {
            if body.len() + chunk.len() > MAX_RESPONSE_BYTES {
                return Err(invalid("HTTP API response too large"));
            }
            body.extend_from_slice(&chunk);
        }
        serde_json::from_slice(&body).map_err(|_| invalid("Invalid HTTP API response"))
    }

    async fn instrument(&self, symbol: &str) -> Result<Instrument, MarketDataError> {
        let response: Envelope<Instrument> = self.get(&["instruments", symbol], &[]).await?;
        response.data.validate(symbol)?;
        Self::validate_price_type(symbol, &response.price_type)?;
        Ok(response.data)
    }

    fn currency(context: &QuoteContext) -> Result<&str, MarketDataError> {
        context
            .currency_hint
            .as_deref()
            .filter(|value| !value.is_empty())
            .ok_or_else(|| invalid("HTTP API quote currency is required"))
    }

    fn validate_price_type(symbol: &str, price_type: &str) -> Result<(), MarketDataError> {
        if (symbol.starts_with("FUND:") && price_type == "unit_nav")
            || (!symbol.starts_with("FUND:") && price_type == "close")
        {
            Ok(())
        } else {
            Err(invalid("Unsupported HTTP API price type"))
        }
    }
}

#[async_trait]
impl MarketDataProvider for HttpApiProvider {
    fn id(&self) -> &'static str {
        DATA_SOURCE_HTTP_API
    }
    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            instrument_kinds: &[InstrumentKind::Equity],
            coverage: Coverage::global_best_effort(),
            supports_latest: true,
            supports_historical: true,
            supports_search: true,
            supports_profile: true,
            supports_dividends: false,
        }
    }
    fn rate_limit(&self) -> RateLimit {
        RateLimit {
            max_concurrency: 2,
            ..RateLimit::default()
        }
    }

    async fn search(&self, query: &str) -> Result<Vec<SearchResult>, MarketDataError> {
        let symbol = query.trim().to_uppercase();
        if qualified_symbol(&symbol).is_none() {
            return Ok(vec![]);
        }
        let asset = match self.instrument(&symbol).await {
            Ok(asset) => asset,
            Err(MarketDataError::SymbolNotFound(_) | MarketDataError::NotSupported { .. }) => {
                return Ok(vec![])
            }
            Err(error) => return Err(error),
        };
        let quote_type = asset.quote_type()?;
        let mut result = SearchResult::new(&asset.symbol, &asset.name, &asset.market, quote_type)
            .with_currency(asset.currency)
            .with_data_source(DATA_SOURCE_HTTP_API);
        if is_known_mic(&asset.market) {
            result.exchange_mic = Some(asset.market);
            result.canonical_symbol = Some(asset.code);
        } else {
            result.canonical_symbol = Some(asset.symbol);
        }
        Ok(vec![result])
    }

    async fn get_profile(&self, symbol: &str) -> Result<AssetProfile, MarketDataError> {
        let asset = self.instrument(symbol).await?;
        let quote_type = asset.quote_type()?.to_string();
        Ok(AssetProfile {
            name: Some(asset.name),
            quote_type: Some(quote_type),
            currency: Some(asset.currency),
            exchange: Some(asset.market),
            source: Some(DATA_SOURCE_HTTP_API.into()),
            ..AssetProfile::default()
        })
    }

    async fn get_latest_quote(
        &self,
        context: &QuoteContext,
        instrument: ProviderInstrument,
    ) -> Result<Quote, MarketDataError> {
        let symbol = instrument.to_symbol_string();
        let response: Envelope<Price> = self.get(&["quotes", &symbol, "latest"], &[]).await?;
        Self::validate_price_type(&symbol, &response.price_type)?;
        response.data.into_quote(&symbol, Self::currency(context)?)
    }

    async fn get_historical_quotes(
        &self,
        context: &QuoteContext,
        instrument: ProviderInstrument,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Vec<Quote>, MarketDataError> {
        let symbol = instrument.to_symbol_string();
        let response: Envelope<Vec<Price>> = self
            .get(
                &["quotes", &symbol, "history"],
                &[
                    ("start", start.format("%Y-%m-%d").to_string()),
                    ("end", end.format("%Y-%m-%d").to_string()),
                ],
            )
            .await?;
        Self::validate_price_type(&symbol, &response.price_type)?;
        let currency = Self::currency(context)?;
        let mut previous = None;
        response
            .data
            .into_iter()
            .map(|price| {
                if price.date < start.date_naive()
                    || price.date > end.date_naive()
                    || previous.is_some_and(|date| date >= price.date)
                {
                    return Err(invalid(
                        "HTTP API history must be in range and strictly date ordered",
                    ));
                }
                previous = Some(price.date);
                price.into_quote(&symbol, currency)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{InstrumentId, ProviderOverrides, ProviderRegistry, ResolverChain};
    use rust_decimal_macros::dec;
    use serde_json::json;
    use std::{
        borrow::Cow,
        io::{Read, Write},
        net::TcpListener,
        sync::Arc,
        thread,
    };

    fn server(status: &str, body: serde_json::Value) -> (String, thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = format!("http://{}", listener.local_addr().unwrap());
        let status = status.to_string();
        let handle = thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut request = Vec::new();
            let mut buffer = [0; 2048];
            while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                let len = socket.read(&mut buffer).unwrap();
                assert!(len > 0);
                request.extend_from_slice(&buffer[..len]);
            }
            let body = body.to_string();
            write!(socket, "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            String::from_utf8(request).unwrap()
        });
        (address, handle)
    }

    fn instrument(symbol: &str, kind: &str) -> serde_json::Value {
        let (market, code) = symbol.split_once(':').unwrap();
        json!({"data": {"symbol": symbol, "code": code, "name": "测试资产", "asset_type": kind, "market": market, "currency": "CNY"}, "price_type": if market == "FUND" { "unit_nav" } else { "close" }})
    }

    fn context() -> QuoteContext {
        let mut overrides = ProviderOverrides::new();
        overrides.insert(
            DATA_SOURCE_HTTP_API.into(),
            ProviderInstrument::EquitySymbol {
                symbol: "FUND:005827".into(),
            },
        );
        QuoteContext {
            instrument: InstrumentId::Equity {
                ticker: "FUND:005827".into(),
                mic: None,
            },
            identifiers: Default::default(),
            overrides: Some(overrides),
            currency_hint: Some(Cow::Borrowed("CNY")),
            preferred_provider: Some(Cow::Borrowed(DATA_SOURCE_HTTP_API)),
            bond_metadata: None,
            custom_provider_code: None,
        }
    }

    #[tokio::test]
    async fn exact_lookup_carries_canonical_identity_and_optional_key() {
        for (symbol, kind, canonical, mic) in [
            ("XSHG:600519", "stock", "600519", Some("XSHG")),
            ("XSHG:510300", "etf", "510300", Some("XSHG")),
            ("XSHE:161725", "lof", "161725", Some("XSHE")),
            ("FUND:005827", "fund", "FUND:005827", None),
        ] {
            let (url, handle) = server("200 OK", instrument(symbol, kind));
            let provider = HttpApiProvider::new(&url, Some("test-key".into())).unwrap();
            let registry =
                ProviderRegistry::new(vec![Arc::new(provider)], Arc::new(ResolverChain::new()));
            let results = registry.search(symbol).await.unwrap();
            assert_eq!(results.len(), 1);
            assert_eq!(results[0].canonical_symbol.as_deref(), Some(canonical));
            assert_eq!(results[0].exchange_mic.as_deref(), mic);
            assert_eq!(results[0].currency.as_deref(), Some("CNY"));
            assert_eq!(results[0].name, "测试资产");
            let request = handle.join().unwrap();
            assert!(request.contains(&format!("/api/v1/instruments/{symbol}")));
            assert!(request.to_lowercase().contains("x-api-key: test-key"));
        }
    }

    #[tokio::test]
    async fn fuzzy_queries_and_unassigned_assets_make_no_http_requests() {
        let provider = Arc::new(HttpApiProvider::new("http://127.0.0.1:1", None).unwrap());
        for query in ["005827", "测试", "600", "", "SWB:US744330AA93"] {
            assert!(provider.search(query).await.unwrap().is_empty());
        }
        let registry = ProviderRegistry::new(vec![provider], Arc::new(ResolverChain::new()));
        let mut ctx = context();
        ctx.preferred_provider = None;
        assert!(matches!(
            registry.fetch_latest_quote(&ctx).await,
            Err(MarketDataError::NoProvidersAvailable)
        ));
        assert!(registry.get_profile(&ctx).await.is_err());
    }

    #[tokio::test]
    async fn lookup_errors_never_become_placeholder_assets() {
        for status in ["401 Unauthorized", "504 Gateway Timeout", "502 Bad Gateway"] {
            let (url, handle) = server(status, json!({"detail": "failure"}));
            let provider = HttpApiProvider::new(&url, None).unwrap();
            assert!(provider.search("FUND:005827").await.is_err());
            handle.join().unwrap();
        }
        for status in ["404 Not Found", "422 Unprocessable Entity"] {
            let (url, handle) = server(status, json!({}));
            assert!(HttpApiProvider::new(&url, None)
                .unwrap()
                .search("FUND:005827")
                .await
                .unwrap()
                .is_empty());
            handle.join().unwrap();
        }
        let (url, handle) = server("200 OK", instrument("FUND:000001", "fund"));
        assert!(HttpApiProvider::new(&url, None)
            .unwrap()
            .search("FUND:005827")
            .await
            .is_err());
        handle.join().unwrap();
    }

    #[tokio::test]
    async fn quotes_keep_decimal_precision_and_nav_date() {
        let body = json!({"data": {"symbol": "FUND:005827", "date": "2024-01-03", "close": "1.234567890123", "open": null, "high": null, "low": null, "volume": null}, "price_type": "unit_nav", "fetched_at": "2024-01-04T10:00:00+08:00"});
        let (url, handle) = server("200 OK", body.clone());
        let provider = HttpApiProvider::new(&url, None).unwrap();
        let ctx = context();
        let symbol = ctx
            .overrides
            .as_ref()
            .unwrap()
            .get(DATA_SOURCE_HTTP_API)
            .unwrap()
            .clone();
        let quote = provider
            .get_latest_quote(&ctx, symbol.clone())
            .await
            .unwrap();
        assert_eq!(quote.close, dec!(1.234567890123));
        assert_eq!(quote.timestamp.date_naive().to_string(), "2024-01-03");
        assert_eq!(quote.source, DATA_SOURCE_HTTP_API);
        assert_eq!(quote.currency, "CNY");
        assert_eq!(quote.open, None);
        handle.join().unwrap();
        let (url, handle) = server(
            "200 OK",
            json!({"data": [body["data"]], "price_type": "unit_nav"}),
        );
        let provider = HttpApiProvider::new(&url, None).unwrap();
        let start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        let end = Utc.with_ymd_and_hms(2024, 1, 31, 0, 0, 0).unwrap();
        let quotes = provider
            .get_historical_quotes(&ctx, symbol, start, end)
            .await
            .unwrap();
        assert_eq!(quotes.len(), 1);
        assert_eq!(quotes[0].close, quote.close);
        let request = handle.join().unwrap();
        assert!(request.contains("start=2024-01-01&end=2024-01-31"));
    }

    #[tokio::test]
    async fn authenticated_requests_do_not_follow_redirects() {
        let target = TcpListener::bind("127.0.0.1:0").unwrap();
        target.set_nonblocking(true).unwrap();
        let status = format!(
            "302 Found\r\nLocation: http://{}/stolen",
            target.local_addr().unwrap()
        );
        let (url, handle) = server(&status, json!({}));
        let provider = HttpApiProvider::new(&url, Some("private-key".into())).unwrap();
        assert!(provider.search("FUND:005827").await.is_err());
        handle.join().unwrap();
        assert_eq!(
            target.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
    }

    #[test]
    fn rejects_invalid_prices_and_base_urls() {
        let traded: Price = serde_json::from_value(
            json!({"symbol": "XSHG:600519", "date": "2024-01-03", "close": "100", "volume": "2"}),
        )
        .unwrap();
        assert_eq!(
            traded.into_quote("XSHG:600519", "CNY").unwrap().volume,
            Some(dec!(200))
        );
        for value in ["0", "-1", "NaN"] {
            let price = serde_json::from_value::<Price>(
                json!({"symbol": "FUND:005827", "date": "2024-01-03", "close": value}),
            );
            assert!(price.is_err() || price.unwrap().into_quote("FUND:005827", "CNY").is_err());
        }
        for url in [
            "file:///etc/passwd",
            "http://user:key@localhost",
            "http://localhost?key=secret",
            "not-a-url",
        ] {
            assert!(HttpApiProvider::new(url, None).is_err());
        }
        assert!(HttpApiProvider::new("http://finance-data-api:8000", None).is_ok());
    }
}
