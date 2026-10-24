//! Parsing shared by the native wire protocols; routing remains in the registry.
use crate::request_recorder::RecordedRequest;
use chrono::{DateTime, NaiveDate, NaiveDateTime, TimeZone, Utc};
use rust_decimal::Decimal;
use serde_json::Value;
use std::{str::FromStr, time::Duration};

use super::{DATA_SOURCE_EASTMONEY, DATA_SOURCE_SINA, DATA_SOURCE_TENCENT};
use crate::{
    errors::MarketDataError, AssetProfile, Coverage, InstrumentId, InstrumentKind,
    ProviderCapabilities, Quote, SearchResult,
};

pub fn invalid(message: &str) -> MarketDataError {
    MarketDataError::ValidationFailed {
        message: message.into(),
    }
}
pub fn response_error(provider: &str, error: MarketDataError) -> MarketDataError {
    match error {
        MarketDataError::ValidationFailed { message } => MarketDataError::ProviderError {
            provider: provider.into(),
            message,
        },
        error => error,
    }
}
pub fn unsupported(provider: &str) -> MarketDataError {
    MarketDataError::NotSupported {
        provider: provider.into(),
        operation: "instrument".into(),
    }
}
pub fn client() -> Result<reqwest::Client, MarketDataError> {
    wealthfolio_http::client_builder()
        .user_agent("Mozilla/5.0")
        .timeout(Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(Into::into)
}
pub async fn get(
    client: &reqwest::Client,
    provider: &str,
    url: &str,
    params: &[(&str, String)],
    recorder: &dyn crate::RequestRecorder,
) -> Result<String, MarketDataError> {
    let referer = if url.contains("fund.eastmoney.com") {
        "https://fundf10.eastmoney.com/"
    } else {
        match provider {
            DATA_SOURCE_SINA => "https://finance.sina.com.cn/",
            DATA_SOURCE_TENCENT => "https://gu.qq.com/",
            _ => "https://quote.eastmoney.com/",
        }
    };
    let mut response = client
        .get(url)
        .header("Referer", referer)
        .query(params)
        .send_recorded(provider, recorder)
        .await
        .map_err(|error| {
            if error.is_timeout() {
                MarketDataError::Timeout {
                    provider: provider.into(),
                }
            } else {
                MarketDataError::ProviderError {
                    provider: provider.into(),
                    message: "Request failed".into(),
                }
            }
        })?;
    if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
        return Err(MarketDataError::RateLimited {
            provider: provider.into(),
        });
    }
    if !response.status().is_success() {
        return Err(MarketDataError::ProviderError {
            provider: provider.into(),
            message: format!("HTTP {}", response.status()),
        });
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        if bytes.len() + chunk.len() > 16 * 1024 * 1024 {
            return Err(invalid("Response exceeds 16 MiB"));
        }
        bytes.extend_from_slice(&chunk);
    }
    match String::from_utf8(bytes) {
        Ok(text) => Ok(text),
        Err(error) => {
            let bytes = error.into_bytes();
            let (text, _, errors) = encoding_rs::GBK.decode(&bytes);
            if errors {
                return Err(invalid("Invalid response encoding"));
            }
            Ok(text.into_owned())
        }
    }
}
pub fn json(text: &str) -> Result<Value, MarketDataError> {
    let text = text.trim();
    // Strip a JSONP callback or assignment; never evaluate upstream JavaScript.
    let start = text
        .find(['{', '['])
        .ok_or_else(|| invalid("Expected JSON response"))?;
    let end = text
        .rfind(['}', ']'])
        .ok_or_else(|| invalid("Incomplete JSON response"))?;
    let payload = text
        .get(start..=end)
        .ok_or_else(|| invalid("Invalid JSON envelope"))?;
    serde_json::from_str(payload).map_err(|_| invalid("Invalid JSON response"))
}
pub fn string(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}
pub fn decimal(value: &Value) -> Result<Decimal, MarketDataError> {
    string(value)
        .and_then(|s| Decimal::from_str(&s).ok())
        .ok_or_else(|| invalid("Invalid decimal"))
}
pub fn optional_decimal(value: &Value) -> Result<Option<Decimal>, MarketDataError> {
    if value.is_null() || value.as_str().is_some_and(|s| matches!(s, "" | "-" | "--")) {
        return Ok(None);
    }
    decimal(value).map(Some)
}
pub fn date(value: &str) -> Result<NaiveDate, MarketDataError> {
    value
        .get(..10)
        .and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())
        .or_else(|| NaiveDate::parse_from_str(value, "%Y%m%d").ok())
        .ok_or_else(|| invalid("Invalid price date"))
}
pub fn midnight(day: NaiveDate) -> DateTime<Utc> {
    Utc.from_utc_datetime(&day.and_hms_opt(0, 0, 0).expect("valid midnight"))
}
pub fn local_timestamp(
    value: &str,
    format: &str,
    currency: &str,
) -> Result<DateTime<Utc>, MarketDataError> {
    let naive = NaiveDateTime::parse_from_str(value, format)
        .map_err(|_| invalid("Invalid quote timestamp"))?;
    let tz = match currency {
        "USD" => chrono_tz::America::New_York,
        "HKD" => chrono_tz::Asia::Hong_Kong,
        "GBP" | "GBX" => chrono_tz::Europe::London,
        "CHF" => chrono_tz::Europe::Zurich,
        _ => chrono_tz::Asia::Shanghai,
    };
    tz.from_local_datetime(&naive)
        .single()
        .map(|d| d.with_timezone(&Utc))
        .ok_or_else(|| invalid("Ambiguous quote timestamp"))
}
pub fn capabilities() -> ProviderCapabilities {
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
pub fn supports(provider: &str, instrument: &InstrumentId) -> bool {
    match instrument {
        InstrumentId::Equity { ticker, mic } => {
            // An explicit asset override may carry the venue; the resolver still rejects bare guesses.
            (mic.is_none() && !ticker.starts_with("FUND:"))
                || resolve(provider, ticker, mic.as_deref()).is_some()
        }
        _ => false,
    }
}

/// Exact market mappings only. Do not send a guessed bare ticker to another listing.
pub fn resolve(provider: &str, ticker: &str, mic: Option<&str>) -> Option<String> {
    if ticker.starts_with("FUND:") {
        return (provider == DATA_SOURCE_EASTMONEY
            && crate::resolver::qualified_symbol(ticker).is_some())
        .then(|| ticker.into());
    }
    let (ticker, mic) = if let Some((m, c)) = crate::resolver::qualified_symbol(ticker) {
        (c, Some(m))
    } else {
        (ticker, mic)
    };
    let index = match ticker {
        "^GSPC" => Some(("100.SPX", "usINX", "gb_$inx")),
        "^DJI" => Some(("100.DJIA", "usDJI", "gb_$dji")),
        "^IXIC" => Some(("100.NDX", "usIXIC", "gb_$ixic")),
        "^HSI" => Some(("100.HSI", "hkHSI", "hkHSI")),
        _ => None,
    };
    if let Some((em, tx, sina)) = index {
        return Some(
            match provider {
                DATA_SOURCE_EASTMONEY => em,
                DATA_SOURCE_TENCENT => tx,
                DATA_SOURCE_SINA => sina,
                _ => return None,
            }
            .into(),
        );
    }
    let mic = mic?;
    let prefix = match (provider, mic) {
        (DATA_SOURCE_EASTMONEY, "XSHG") => "1.",
        (DATA_SOURCE_EASTMONEY, "XSHE" | "XBEI") => "0.",
        (DATA_SOURCE_EASTMONEY, "XHKG") => "116.",
        (DATA_SOURCE_EASTMONEY, "XNAS") => "105.",
        (DATA_SOURCE_EASTMONEY, "XNYS") => "106.",
        (DATA_SOURCE_EASTMONEY, "XASE" | "ARCX" | "BATS") => "107.",
        (DATA_SOURCE_EASTMONEY, "XLON") => "155.",
        (DATA_SOURCE_SINA | DATA_SOURCE_TENCENT, "XSHG") => "sh",
        (DATA_SOURCE_SINA | DATA_SOURCE_TENCENT, "XSHE") => "sz",
        (DATA_SOURCE_SINA | DATA_SOURCE_TENCENT, "XBEI") => "bj",
        (DATA_SOURCE_SINA | DATA_SOURCE_TENCENT, "XHKG") => "hk",
        (DATA_SOURCE_SINA, "XNAS" | "XNYS" | "XASE" | "ARCX" | "BATS") => "gb_",
        (DATA_SOURCE_TENCENT, "XNAS" | "XNYS" | "XASE" | "ARCX" | "BATS") => "us",
        _ => return None,
    };
    if provider == DATA_SOURCE_TENCENT && prefix == "us" {
        let suffix = match mic {
            "XNAS" => ".OQ",
            "XNYS" => ".N",
            "XASE" | "ARCX" | "BATS" => ".AM",
            _ => return None,
        };
        return Some(format!("us{ticker}{suffix}"));
    }
    Some(format!(
        "{prefix}{}",
        if provider == DATA_SOURCE_SINA && prefix == "gb_" {
            ticker.to_lowercase()
        } else {
            ticker.to_owned()
        }
    ))
}

pub fn index_identity(provider: &str, symbol: &str) -> Option<(&'static str, &'static str)> {
    for (canonical, currency) in [
        ("^GSPC", "USD"),
        ("^DJI", "USD"),
        ("^IXIC", "USD"),
        ("^HSI", "HKD"),
    ] {
        if resolve(provider, canonical, None).is_some_and(|s| s.eq_ignore_ascii_case(symbol)) {
            return Some((canonical, currency));
        }
    }
    None
}

pub fn profile(result: SearchResult) -> AssetProfile {
    AssetProfile {
        name: Some(result.name),
        quote_type: Some(result.asset_type),
        currency: result.currency,
        exchange: result.exchange_mic.or(Some(result.exchange)),
        source: result.data_source,
        ..AssetProfile::default()
    }
}
pub fn matches_query(result: &SearchResult, query: &str) -> bool {
    match crate::resolver::qualified_symbol(query) {
        Some(("FUND", _)) => result.canonical_symbol.as_deref() == Some(query),
        Some((mic, code)) => {
            result.exchange_mic.as_deref() == Some(mic)
                && result
                    .canonical_symbol
                    .as_deref()
                    .is_some_and(|s| s.eq_ignore_ascii_case(code))
        }
        None => true,
    }
}
pub fn finish_history(
    mut quotes: Vec<Quote>,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
) -> Result<Vec<Quote>, MarketDataError> {
    quotes.retain(|q| {
        q.timestamp.date_naive() >= start.date_naive()
            && q.timestamp.date_naive() <= end.date_naive()
    });
    quotes.sort_by_key(|q| q.timestamp);
    for pair in quotes.windows(2) {
        if pair[0].timestamp == pair[1].timestamp && pair[0].close != pair[1].close {
            return Err(invalid("Conflicting history rows"));
        }
    }
    quotes.dedup_by_key(|q| q.timestamp);
    Ok(quotes)
}
pub fn quote(
    day: NaiveDate,
    values: &[Value],
    currency: &str,
    provider: &str,
) -> Result<Quote, MarketDataError> {
    if values.len() < 5 {
        return Err(invalid("Incomplete OHLC row"));
    }
    let close = decimal(&values[1])?;
    if close <= Decimal::ZERO {
        return Err(invalid("Non-positive price"));
    }
    let volume = optional_decimal(&values[4])?;
    if volume.is_some_and(|v| v < Decimal::ZERO) {
        return Err(invalid("Negative volume"));
    }
    Ok(Quote {
        timestamp: midnight(day),
        open: optional_decimal(&values[0])?,
        close,
        high: optional_decimal(&values[2])?,
        low: optional_decimal(&values[3])?,
        volume,
        currency: currency.into(),
        source: provider.into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn each_native_dispatch_is_attributed_to_its_provider() {
        use crate::request_recorder::tests::{MemoryRecorder, MockServer};
        let server = MockServer::start(vec![
            (200, String::new(), "{}".into()),
            (200, String::new(), "{}".into()),
            (200, String::new(), "{}".into()),
        ]);
        let recorder = MemoryRecorder::default();
        let client = client().unwrap();
        for provider in [DATA_SOURCE_EASTMONEY, DATA_SOURCE_TENCENT, DATA_SOURCE_SINA] {
            assert_eq!(
                get(
                    &client,
                    provider,
                    &server.url,
                    &[("page", "1".into())],
                    &recorder
                )
                .await
                .unwrap(),
                "{}"
            );
        }
        let events = recorder.0.lock().unwrap();
        assert_eq!(
            events.iter().map(|(id, _)| id.as_str()).collect::<Vec<_>>(),
            vec![DATA_SOURCE_EASTMONEY, DATA_SOURCE_TENCENT, DATA_SOURCE_SINA]
        );
        assert_eq!(server.requests.lock().unwrap().len(), 3);
    }

    #[test]
    fn identities_are_market_specific() {
        assert_eq!(
            resolve(DATA_SOURCE_EASTMONEY, "600519", Some("XSHG")).as_deref(),
            Some("1.600519")
        );
        assert_eq!(
            resolve(DATA_SOURCE_TENCENT, "00700", Some("XHKG")).as_deref(),
            Some("hk00700")
        );
        assert!(resolve(DATA_SOURCE_SINA, "FUND:005827", None).is_none());
        assert!(resolve(DATA_SOURCE_EASTMONEY, "AAPL", None).is_none());
        assert!(resolve(DATA_SOURCE_TENCENT, "SHOP", Some("XTSE")).is_none());
    }
    #[test]
    fn merged_history_is_sorted_deduplicated_and_range_checked() {
        let day = NaiveDate::from_ymd_opt(2026, 1, 5).unwrap();
        let values = [1, 2, 3, 1, 100].map(Value::from);
        let first = quote(day, &values, "CNY", DATA_SOURCE_EASTMONEY).unwrap();
        let mut second = first.clone();
        second.timestamp += chrono::Duration::days(1);
        let mut outside = second.clone();
        outside.timestamp += chrono::Duration::days(1);
        let rows = finish_history(
            vec![second.clone(), first.clone(), first.clone(), outside],
            first.timestamp,
            second.timestamp,
        )
        .unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].timestamp, first.timestamp);
        let mut conflicting = first.clone();
        conflicting.close += Decimal::ONE;
        assert!(finish_history(
            vec![first.clone(), conflicting],
            first.timestamp,
            second.timestamp
        )
        .is_err());
    }
    #[test]
    fn data_wrappers_are_not_executed() {
        assert_eq!(json("callback({\"x\":1});").unwrap()["x"], 1);
        assert!(json("callback({x: execute()});").is_err());
        assert!(json("} malformed {").is_err());
        assert!(decimal(&Value::String("--".into())).is_err());
    }
}
