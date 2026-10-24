//! Signed REST quotes. Credentials stay in SecretStore; initialization is local.
mod auth;
pub use auth::FutuCredentials;

use async_trait::async_trait;
use chrono::{DateTime, NaiveDate, Utc};
use reqwest::Method;
use serde_json::Value;
use std::{borrow::Cow, time::Duration};
use uuid::Uuid;

use super::{native, MarketDataProvider, ProviderCapabilities, RateLimit};
use crate::{
    errors::MarketDataError,
    registry::{RateLimitConfig, RateLimiter},
    resolver::futu::market_info,
    Coverage, InstrumentKind, ProviderInstrument, Quote, QuoteContext,
};
use auth::{signing_message, SigningKey};

pub const DATA_SOURCE_FUTU: &str = "FUTU";
const HOST: &str = "https://webapi.futunn.com";

fn error(message: &str) -> MarketDataError {
    MarketDataError::ProviderError {
        provider: DATA_SOURCE_FUTU.into(),
        message: message.into(),
    }
}

pub struct FutuProvider {
    client: reqwest::Client,
    app_key_id: String,
    key: SigningKey,
    limiter: RateLimiter,
    #[cfg(test)]
    endpoint: Option<String>,
}

impl FutuProvider {
    pub fn new(credentials: FutuCredentials) -> Result<Self, MarketDataError> {
        credentials.validate_app_key_id()?;
        let key = SigningKey::parse(&credentials.private_key)?;
        let limiter = RateLimiter::new();
        limiter.configure(
            &Cow::Borrowed(DATA_SOURCE_FUTU),
            RateLimitConfig {
                requests_per_minute: 60,
                burst_capacity: 1.0,
            },
        );
        let client = wealthfolio_http::client_builder()
            .timeout(Duration::from_secs(15))
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        Ok(Self {
            client,
            app_key_id: credentials.app_key_id,
            key,
            limiter,
            #[cfg(test)]
            endpoint: None,
        })
    }

    async fn request(
        &self,
        method: Method,
        path: &str,
        params: &[(&str, String)],
        body: &[u8],
    ) -> Result<Value, MarketDataError> {
        let request = self.signed_request(method, path, params, body).await?;
        let mut response = self.client.execute(request).await.map_err(|err| {
            if err.is_timeout() {
                MarketDataError::Timeout {
                    provider: DATA_SOURCE_FUTU.into(),
                }
            } else {
                error("Request failed")
            }
        })?;
        if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
            return Err(MarketDataError::RateLimited {
                provider: DATA_SOURCE_FUTU.into(),
            });
        }
        if matches!(response.status().as_u16(), 401 | 403) {
            return Err(error("Authentication failed: check AppKeyId, private key, signing algorithm, permissions and system time"));
        }
        if !response.status().is_success() {
            return Err(error(&format!("HTTP {}", response.status())));
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| error("Unable to read response"))?
        {
            if bytes.len() + chunk.len() > 16 * 1024 * 1024 {
                return Err(error("Response exceeds 16 MiB"));
            }
            bytes.extend_from_slice(&chunk);
        }
        let value: Value =
            serde_json::from_slice(&bytes).map_err(|_| error("Invalid JSON response"))?;
        check_response(&value)?;
        Ok(value)
    }

    async fn signed_request(
        &self,
        method: Method,
        path: &str,
        params: &[(&str, String)],
        body: &[u8],
    ) -> Result<reqwest::Request, MarketDataError> {
        self.limiter.acquire(&Cow::Borrowed(DATA_SOURCE_FUTU)).await;
        let host = HOST;
        #[cfg(test)]
        let host = self.endpoint.as_deref().unwrap_or(host);
        let mut builder = self
            .client
            .request(method, format!("{host}{path}"))
            .query(params);
        if !body.is_empty() {
            builder = builder
                .header("Content-Type", "application/json")
                .body(body.to_vec());
        }
        let mut request = builder.build().map_err(|_| error("Invalid request"))?;
        let timestamp = Utc::now().timestamp_millis();
        let signature = self
            .key
            .sign(signing_message(timestamp, &request, body).as_bytes())?;
        for (name, value) in [
            ("X-Api-Key", self.app_key_id.clone()),
            ("X-Timestamp", timestamp.to_string()),
            ("X-Nonce", Uuid::new_v4().to_string()),
            ("Authorization", signature),
        ] {
            let mut value = reqwest::header::HeaderValue::from_str(&value)
                .map_err(|_| error("Invalid authentication header"))?;
            value.set_sensitive(true);
            request.headers_mut().insert(
                reqwest::header::HeaderName::from_bytes(name.as_bytes()).expect("static header"),
                value,
            );
        }
        Ok(request)
    }
}

fn check_response(value: &Value) -> Result<(), MarketDataError> {
    match value["ret_code"].as_i64() {
        Some(0) => Ok(()),
        Some(-12011) => Err(MarketDataError::RateLimited { provider: DATA_SOURCE_FUTU.into() }),
        Some(-12301) => Err(MarketDataError::Timeout { provider: DATA_SOURCE_FUTU.into() }),
        Some(-12006 | -12009 | -12102) => Err(error("Authentication failed: check AppKeyId, private key, signing algorithm, permissions and system time")),
        Some(-7) => Err(MarketDataError::SymbolNotFound("FUTU symbol unavailable".into())),
        Some(-8) => Err(native::unsupported(DATA_SOURCE_FUTU)),
        Some(code) => Err(error(&format!("API error {code}"))),
        None => Err(error("Missing ret_code")),
    }
}

fn symbol(instrument: &ProviderInstrument) -> Result<&str, MarketDataError> {
    match instrument {
        ProviderInstrument::EquitySymbol { symbol } if market_info(symbol).is_some() => Ok(symbol),
        _ => Err(native::unsupported(DATA_SOURCE_FUTU)),
    }
}

fn day(row: &Value, symbol: &str, latest: bool) -> Result<NaiveDate, MarketDataError> {
    let date = &row[if latest { "data_date" } else { "date" }];
    if !date.is_null() {
        return native::string(date)
            .and_then(|date| native::date(&date).ok())
            .ok_or_else(|| error("Invalid trading date"));
    }
    let millis = row[if latest { "data_time" } else { "time_key" }]
        .as_i64()
        .ok_or_else(|| error("Missing quote timestamp"))?;
    let timestamp =
        DateTime::from_timestamp_millis(millis).ok_or_else(|| error("Invalid quote timestamp"))?;
    let (_, timezone) = market_info(symbol).ok_or_else(|| native::unsupported(DATA_SOURCE_FUTU))?;
    Ok(timestamp.with_timezone(&timezone).date_naive())
}

fn parse_quote(
    row: &Value,
    symbol: &str,
    latest: bool,
    volume_precision: u32,
) -> Result<Quote, MarketDataError> {
    let (currency, _) = market_info(symbol).ok_or_else(|| native::unsupported(DATA_SOURCE_FUTU))?;
    let fields = if latest {
        [
            "open_price",
            "last_price",
            "high_price",
            "low_price",
            "volume",
        ]
    } else {
        ["open", "close", "high", "low", "volume"]
    };
    let values = fields.map(|field| row[field].clone());
    let mut quote = native::quote(
        day(row, symbol, latest)?,
        &values,
        currency,
        DATA_SOURCE_FUTU,
    )
    .map_err(|err| native::response_error(DATA_SOURCE_FUTU, err))?;
    if volume_precision > 28 {
        return Err(error("Invalid volume precision"));
    }
    if let Some(volume) = quote.volume.as_mut() {
        *volume *= rust_decimal::Decimal::new(1, volume_precision);
    }
    Ok(quote)
}

fn history_rows(value: &Value, symbol: &str) -> Result<Vec<Quote>, MarketDataError> {
    if value["pagination"]["has_more"].as_bool() == Some(true) {
        return Err(error("Incomplete daily history segment"));
    }
    let rows = value["data"]["kline_list"]
        .as_array()
        .ok_or_else(|| error("Missing kline_list"))?;
    let precision = match value["data"].get("volume_precision") {
        None => 0,
        Some(value) => value
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .ok_or_else(|| error("Invalid volume precision"))?,
    };
    rows.iter()
        .map(|row| parse_quote(row, symbol, false, precision))
        .collect()
}

fn segments(start: NaiveDate, end: NaiveDate) -> Vec<(NaiveDate, NaiveDate)> {
    let mut result = Vec::new();
    let mut from = start;
    while from <= end {
        let to = from
            .checked_add_signed(chrono::Duration::days(299))
            .unwrap_or(end)
            .min(end);
        result.push((from, to));
        if to == end {
            break;
        }
        from = to.succ_opt().expect("to is before end");
    }
    result
}

pub(crate) fn capabilities() -> ProviderCapabilities {
    ProviderCapabilities {
        instrument_kinds: &[InstrumentKind::Equity],
        coverage: Coverage {
            equity_mic_allow: Some(&[
                "XNAS", "XNYS", "XASE", "ARCX", "BATS", "XHKG", "XSHG", "XSHE", "BJSE",
            ]),
            allow_unknown_mic: true,
            ..Coverage::default()
        },
        supports_latest: true,
        supports_historical: true,
        supports_search: false,
        supports_profile: false,
        supports_dividends: false,
    }
}

#[async_trait]
impl MarketDataProvider for FutuProvider {
    fn id(&self) -> &'static str {
        DATA_SOURCE_FUTU
    }
    fn capabilities(&self) -> ProviderCapabilities {
        capabilities()
    }

    fn rate_limit(&self) -> RateLimit {
        RateLimit {
            requests_per_minute: 60,
            max_concurrency: 1,
            min_delay: Duration::from_secs(1),
        }
    }
    async fn get_latest_quote(
        &self,
        _context: &QuoteContext,
        instrument: ProviderInstrument,
    ) -> Result<Quote, MarketDataError> {
        let symbol = symbol(&instrument)?;
        let body = serde_json::to_vec(&serde_json::json!({"code_list": [symbol]}))
            .map_err(|_| error("Invalid quote request"))?;
        let value = self
            .request(Method::POST, "/api/v1.0/quote/stock-quote", &[], &body)
            .await?;
        let row = value["data"]["quote_list"]
            .as_array()
            .ok_or_else(|| error("Missing quote_list"))?
            .iter()
            .find(|row| row["code"].as_str() == Some(symbol))
            .ok_or_else(|| MarketDataError::SymbolNotFound("FUTU symbol unavailable".into()))?;
        parse_quote(row, symbol, true, 0)
    }
    async fn get_historical_quotes(
        &self,
        _context: &QuoteContext,
        instrument: ProviderInstrument,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Vec<Quote>, MarketDataError> {
        let symbol = symbol(&instrument)?;
        let mut quotes = Vec::new();
        for (from, to) in segments(start.date_naive(), end.date_naive()) {
            let value = self
                .request(
                    Method::GET,
                    &format!("/api/v1.0/quote/{symbol}/history-kline"),
                    &[
                        ("start", from.to_string()),
                        ("end", to.to_string()),
                        ("ktype", "2".into()),
                        ("autype", "0".into()),
                        ("num", "370".into()),
                    ],
                    &[],
                )
                .await?;
            quotes.extend(history_rows(&value, symbol)?);
        }
        native::finish_history(quotes, start, end)
            .map_err(|err| native::response_error(DATA_SOURCE_FUTU, err))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::InstrumentId;
    use ed25519_dalek::pkcs8::EncodePrivateKey;
    use serde_json::json;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        sync::mpsc,
    };

    fn provider() -> FutuProvider {
        let key = ed25519_dalek::SigningKey::from_bytes(&[7; 32]);
        FutuProvider::new(FutuCredentials {
            app_key_id: "synthetic-app".into(),
            private_key: key.to_pkcs8_pem(Default::default()).unwrap().to_string(),
        })
        .unwrap()
    }
    fn context() -> QuoteContext {
        QuoteContext {
            instrument: InstrumentId::Equity {
                ticker: "AAPL".into(),
                mic: Some("XNAS".into()),
            },
            identifiers: Default::default(),
            overrides: None,
            currency_hint: None,
            preferred_provider: None,
            bond_metadata: None,
            custom_provider_code: None,
        }
    }
    fn instrument(symbol: &str) -> ProviderInstrument {
        ProviderInstrument::EquitySymbol {
            symbol: symbol.into(),
        }
    }
    fn historical(date: i32, close: &str) -> Value {
        json!({"date": date, "open": "10.1", "close": close, "high": 12, "low": 9, "volume": 100})
    }
    // Local transport exercises the signed request and response paths without credentials or Internet.
    fn server(responses: Vec<Value>) -> (String, mpsc::Receiver<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            for response in responses {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut bytes = Vec::new();
                let mut buf = [0; 4096];
                loop {
                    let count = stream.read(&mut buf).unwrap();
                    if count == 0 {
                        break;
                    }
                    bytes.extend_from_slice(&buf[..count]);
                    if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                        let headers = String::from_utf8_lossy(&bytes[..end]).to_lowercase();
                        let length = headers
                            .lines()
                            .find_map(|line| line.strip_prefix("content-length: "))
                            .and_then(|value| value.parse::<usize>().ok())
                            .unwrap_or(0);
                        if bytes.len() >= end + 4 + length {
                            break;
                        }
                    }
                }
                sender.send(String::from_utf8(bytes).unwrap()).unwrap();
                let body = response.to_string();
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                )
                .unwrap();
            }
        });
        (endpoint, receiver)
    }

    #[test]
    fn futu_dates_decimal_currency_and_response_validation() {
        let value = json!({"ret_code":0, "data":{"kline_list":[historical(20260105, "10.123456789")], "volume_precision":2}});
        let rows = history_rows(&value, "HK.00700").unwrap();
        assert_eq!(rows[0].close.to_string(), "10.123456789");
        assert_eq!(rows[0].volume.unwrap(), rust_decimal::Decimal::ONE);
        assert_eq!(rows[0].currency, "HKD");
        let row = json!({"time_key": 1767650400000_i64, "close": 10});
        assert_eq!(
            day(&row, "US.AAPL", false).unwrap().to_string(),
            "2026-01-05"
        );
        assert!(history_rows(
            &json!({"data":{"kline_list":[]},"pagination":{"has_more":true}}),
            "US.AAPL"
        )
        .is_err());
        assert!(history_rows(&json!({"data":{}}), "US.AAPL").is_err());
        assert!(matches!(
            check_response(&json!({"ret_code":-12011})),
            Err(MarketDataError::RateLimited { .. })
        ));
        assert!(matches!(
            check_response(&json!({"ret_code":-12301})),
            Err(MarketDataError::Timeout { .. })
        ));
        let err = check_response(&json!({"ret_code":-12006,"ret_msg":"sensitive response"}))
            .unwrap_err()
            .to_string();
        assert!(err.contains("system time"));
        assert!(!err.contains("sensitive response"));
        assert!(check_response(&json!({})).is_err());
    }
    #[test]
    fn futu_segments_cover_inclusive_range_without_overlap() {
        let start = NaiveDate::from_ymd_opt(2024, 1, 1).unwrap();
        let end = start + chrono::Duration::days(600);
        let parts = segments(start, end);
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[0], (start, start + chrono::Duration::days(299)));
        assert_eq!(parts[1].0, parts[0].1.succ_opt().unwrap());
        assert_eq!(parts[2], (end, end));
        assert!(segments(end, start).is_empty());
    }
    #[tokio::test]
    async fn futu_each_request_is_throttled_and_independently_signed() {
        let provider = provider();
        let first = provider
            .signed_request(Method::GET, "/one", &[], &[])
            .await
            .unwrap();
        let start = std::time::Instant::now();
        let second = provider
            .signed_request(Method::GET, "/two", &[], &[])
            .await
            .unwrap();
        assert!(start.elapsed() >= Duration::from_millis(900));
        assert_ne!(first.headers()["x-nonce"], second.headers()["x-nonce"]);
        assert_ne!(
            first.headers()["authorization"],
            second.headers()["authorization"]
        );
        assert!(second.headers()["authorization"].is_sensitive());
        use base64::Engine;
        use ed25519_dalek::Verifier;
        for request in [first, second] {
            let timestamp = request.headers()["x-timestamp"]
                .to_str()
                .unwrap()
                .parse()
                .unwrap();
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(request.headers()["authorization"].as_bytes())
                .unwrap();
            ed25519_dalek::SigningKey::from_bytes(&[7; 32])
                .verifying_key()
                .verify(
                    signing_message(timestamp, &request, &[]).as_bytes(),
                    &ed25519_dalek::Signature::from_slice(&bytes).unwrap(),
                )
                .unwrap();
        }
    }
    #[tokio::test]
    async fn futu_history_uses_all_segments_and_discards_partial_failures() {
        let start = native::midnight(NaiveDate::from_ymd_opt(2025, 1, 1).unwrap());
        let end = start + chrono::Duration::days(300);
        for fail in [false, true] {
            let second = if fail {
                json!({"ret_code":-12011})
            } else {
                json!({"ret_code":0,"data":{"kline_list":[historical(20251028, "11")]}})
            };
            let (endpoint, requests) = server(vec![
                json!({"ret_code":0,"data":{"kline_list":[historical(20250101, "10")]}}),
                second,
            ]);
            let mut provider = provider();
            provider.endpoint = Some(endpoint);
            let result = provider
                .get_historical_quotes(&context(), instrument("US.AAPL"), start, end)
                .await;
            if fail {
                assert!(matches!(result, Err(MarketDataError::RateLimited { .. })));
            } else {
                let rows = result.unwrap();
                assert_eq!(rows.len(), 2);
                assert!(rows[0].timestamp < rows[1].timestamp);
            }
            let first = requests.recv_timeout(Duration::from_secs(5)).unwrap();
            let second = requests.recv_timeout(Duration::from_secs(5)).unwrap();
            assert!(first.contains("start=2025-01-01&end=2025-10-27&ktype=2&autype=0&num=370"));
            assert!(second.contains("start=2025-10-28&end=2025-10-28"));
        }
    }
    #[tokio::test]
    async fn futu_latest_matches_code_and_unsupported_instruments_never_connect() {
        let (endpoint, requests) = server(vec![json!({"ret_code":0,"data":{"quote_list":[
            {"code":"US.OTHER","data_date":"2026-01-05","last_price":999},
            {"code":"HK.00700","data_date":"2026-01-05","last_price":"450.12","volume":100}
        ]}})]);
        let mut provider = provider();
        provider.endpoint = Some(endpoint);
        assert!(provider
            .get_latest_quote(&context(), instrument("CA.SHOP"))
            .await
            .is_err());
        assert!(provider
            .get_historical_quotes(&context(), instrument("600519"), Utc::now(), Utc::now())
            .await
            .is_err());
        let quote = provider
            .get_latest_quote(&context(), instrument("HK.00700"))
            .await
            .unwrap();
        assert_eq!(quote.close.to_string(), "450.12");
        assert_eq!(quote.currency, "HKD");
        let request = requests.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(request.starts_with("POST /api/v1.0/quote/stock-quote"));
        assert!(request.ends_with(r#"{"code_list":["HK.00700"]}"#));
    }
    #[tokio::test]
    async fn futu_enabled_preserves_fund_and_exchange_qualified_search() {
        use crate::{FixtureProvider, ProviderRegistry, ResolverChain};
        use std::{collections::HashMap, sync::Arc};
        let mut futu = provider();
        futu.endpoint = Some("http://127.0.0.1:9".into());
        let mut fund = context();
        fund.instrument = InstrumentId::Equity {
            ticker: "FUND:161725".into(),
            mic: None,
        };
        assert!(!futu.supports_instrument(&fund.instrument));
        let fixture = FixtureProvider::new_for_provider(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../e2e/fixtures/quotes"),
            crate::provider::DATA_SOURCE_EASTMONEY,
        );
        let registry = ProviderRegistry::with_priorities(
            vec![Arc::new(futu), Arc::new(fixture)],
            Arc::new(ResolverChain::new()),
            HashMap::from([("FUTU".into(), 0), ("EASTMONEY".into(), 1)]),
        );
        for (query, canonical, mic) in [
            ("FUND:161725", "FUND:161725", None),
            ("fund:005827", "FUND:005827", None),
            ("XSHE:161725", "161725", Some("XSHE")),
            ("xshg:600519", "600519", Some("XSHG")),
        ] {
            let rows = registry.search(query).await.unwrap();
            assert_eq!(rows.len(), 1, "{query}");
            assert_eq!(rows[0].canonical_symbol.as_deref(), Some(canonical));
            assert_eq!(rows[0].exchange_mic.as_deref(), mic);
            assert_eq!(rows[0].data_source.as_deref(), Some("EASTMONEY"));
        }
        assert_eq!(
            registry.fetch_latest_quote(&fund).await.unwrap().source,
            "EASTMONEY"
        );
    }

    #[test]
    fn futu_explicit_override_wins_without_modifying_user_configuration() {
        use crate::{ProviderOverrides, ResolverChain, SymbolResolver};
        let mut context = context();
        let mut overrides = ProviderOverrides::new();
        overrides.insert("FUTU".into(), instrument("HK.00700"));
        context.overrides = Some(overrides);
        let resolved = ResolverChain::new()
            .resolve(&Cow::Borrowed("FUTU"), &context)
            .unwrap();
        assert_eq!(resolved.instrument.to_symbol_string(), "HK.00700");
        assert_eq!(
            context
                .overrides
                .unwrap()
                .get("FUTU")
                .unwrap()
                .to_symbol_string(),
            "HK.00700"
        );
    }
}
