//! Tencent's own search, quote and unadjusted daily-bar endpoints.
use super::{native::*, DATA_SOURCE_TENCENT};
use crate::{
    errors::MarketDataError, AssetProfile, InstrumentId, MarketDataProvider, ProviderCapabilities,
    ProviderInstrument, Quote, QuoteContext, RateLimit, SearchResult,
};
use async_trait::async_trait;
use chrono::{DateTime, Datelike, Utc};
use rust_decimal::Decimal;
use serde_json::Value;

pub struct TencentProvider {
    client: reqwest::Client,
}
impl TencentProvider {
    pub fn new() -> Result<Self, MarketDataError> {
        Ok(Self { client: client()? })
    }
    async fn text(&self, url: &str, params: &[(&str, String)]) -> Result<String, MarketDataError> {
        get(&self.client, self.id(), url, params).await
    }
}
fn assignment(text: &str) -> Result<String, MarketDataError> {
    let (_, value) = text
        .split_once('=')
        .ok_or_else(|| invalid("Invalid Tencent response"))?;
    serde_json::from_str(value.trim().trim_end_matches(';'))
        .map_err(|_| invalid("Invalid Tencent string"))
}
fn parse_search(text: &str) -> Result<Vec<SearchResult>, MarketDataError> {
    let text = assignment(text)?;
    let mut results = Vec::new();
    for entry in text.split('^') {
        let f: Vec<_> = entry.split('~').collect();
        if f.len() < 5 {
            continue;
        }
        let (mic, currency, code) = match f[0] {
            "sh" => (
                Some("XSHG"),
                if f[1].starts_with("900") {
                    "USD"
                } else {
                    "CNY"
                },
                f[1],
            ),
            "sz" => (
                Some("XSHE"),
                if f[1].starts_with("200") {
                    "HKD"
                } else {
                    "CNY"
                },
                f[1],
            ),
            "bj" => (Some("XBEI"), "CNY", f[1]),
            "hk" => (Some("XHKG"), "HKD", f[1]),
            "us" => {
                let (code, venue) = f[1].rsplit_once('.').unwrap_or((f[1], ""));
                let mic = match venue.to_ascii_lowercase().as_str() {
                    "oq" | "o" => Some("XNAS"),
                    "n" => Some("XNYS"),
                    "a" | "am" => None,
                    _ => None,
                };
                (mic, "USD", code)
            }
            _ => continue,
        };
        let kind = match f[4] {
            "GP" => "EQUITY",
            "ZS" => "INDEX",
            kind if kind == "JJ" || kind.ends_with("ETF") || kind.ends_with("LOF") => "ETF",
            _ => continue,
        };
        let code = code.to_uppercase();
        let mut result = SearchResult::new(
            format!(
                "{}{}",
                f[0],
                if f[0] == "us" {
                    f[1].to_uppercase()
                } else {
                    code.clone()
                }
            ),
            f[2],
            mic.unwrap_or("US"),
            kind,
        )
        .with_currency(currency)
        .with_data_source(DATA_SOURCE_TENCENT);
        if kind == "INDEX" {
            if let Some((canonical, ccy)) = index_identity(DATA_SOURCE_TENCENT, &result.symbol) {
                result.canonical_symbol = Some(canonical.into());
                result.currency = Some(ccy.into());
                results.push(result);
                continue;
            }
        }
        result.canonical_symbol = Some(code);
        result.exchange_mic = mic.map(str::to_owned);
        results.push(result);
    }
    Ok(results)
}
fn quote_symbol(symbol: &str) -> &str {
    if symbol.starts_with("us") {
        for suffix in [".OQ", ".N", ".AM", ".A"] {
            if let Some(base) = symbol.strip_suffix(suffix) {
                return base;
            }
        }
    }
    symbol
}
fn parse_quote(
    text: &str,
    symbol: &str,
    currency: &str,
) -> Result<(Quote, String), MarketDataError> {
    let (key, _) = text
        .split_once('=')
        .ok_or_else(|| invalid("Invalid Tencent quote"))?;
    if key.trim() != format!("v_{}", quote_symbol(symbol)) {
        return Err(invalid("Quote identity mismatch"));
    }
    let text = assignment(text)?;
    let f: Vec<_> = text.split('~').collect();
    if f.len() < 35 {
        return Err(invalid("Incomplete Tencent quote"));
    }
    if quote_symbol(symbol) != symbol && !f[2].eq_ignore_ascii_case(&symbol[2..]) {
        return Err(invalid("Tencent venue mismatch"));
    }
    let timestamp = local_timestamp(
        f[30],
        if f[30].contains('-') {
            "%Y-%m-%d %H:%M:%S"
        } else if f[30].contains('/') {
            "%Y/%m/%d %H:%M:%S"
        } else {
            "%Y%m%d%H%M%S"
        },
        if symbol.starts_with("us") {
            "USD"
        } else if symbol.starts_with("hk") {
            "HKD"
        } else {
            "CNY"
        },
    )?;
    let values = [f[5], f[3], f[33], f[34], f[6]].map(|s| Value::String(s.into()));
    let mut q = quote(
        timestamp.date_naive(),
        &values,
        currency,
        DATA_SOURCE_TENCENT,
    )?;
    q.timestamp = timestamp;
    if symbol.starts_with("sh") || symbol.starts_with("sz") || symbol.starts_with("bj") {
        q.volume = q.volume.map(|v| v * Decimal::from(100));
    }
    Ok((q, f[1].into()))
}
fn parse_history(
    value: &Value,
    symbol: &str,
    currency: &str,
) -> Result<Vec<Quote>, MarketDataError> {
    let data = &value["data"][symbol];
    // Never substitute qfqday/hfqday when the unadjusted series is unavailable.
    let Some(rows) = data["day"].as_array() else {
        if data.is_null() {
            return Ok(vec![]);
        }
        return Err(invalid("Unadjusted daily bars missing"));
    };
    rows.iter()
        .map(|row| {
            let f = row
                .as_array()
                .ok_or_else(|| invalid("Invalid Tencent daily bar"))?;
            if f.len() < 6 {
                return Err(invalid("Incomplete daily bar"));
            }
            let mut q = quote(
                date(f[0].as_str().ok_or_else(|| invalid("Missing bar date"))?)?,
                &f[1..],
                currency,
                DATA_SOURCE_TENCENT,
            )?;
            let shares = symbol.starts_with("sh688")
                || symbol.starts_with("sh000")
                || symbol.starts_with("sz399");
            if !shares
                && (symbol.starts_with("sh")
                    || symbol.starts_with("sz")
                    || symbol.starts_with("bj"))
            {
                q.volume = q.volume.map(|v| v * Decimal::from(100));
            }
            Ok(q)
        })
        .collect()
}
#[async_trait]
impl MarketDataProvider for TencentProvider {
    fn id(&self) -> &'static str {
        DATA_SOURCE_TENCENT
    }
    fn capabilities(&self) -> ProviderCapabilities {
        capabilities()
    }
    fn rate_limit(&self) -> RateLimit {
        RateLimit {
            max_concurrency: 2,
            ..RateLimit::default()
        }
    }
    fn supports_instrument(&self, instrument: &InstrumentId) -> bool {
        supports(self.id(), instrument)
    }
    async fn search(&self, query: &str) -> Result<Vec<SearchResult>, MarketDataError> {
        let result: Result<_, MarketDataError> = async {
            let normalized = query.trim().to_uppercase();
            let keyword = match crate::resolver::qualified_symbol(&normalized) {
                Some(("FUND", _)) => return Ok(vec![]),
                Some((_, code)) => code,
                None => match normalized.as_str() {
                    "^GSPC" => "INX",
                    "^DJI" => "DJI",
                    "^IXIC" => "IXIC",
                    "^HSI" => "HSI",
                    _ => query,
                },
            };
            let text = self
                .text(
                    "https://smartbox.gtimg.cn/s3/",
                    &[("q", keyword.into()), ("t", "all".into())],
                )
                .await?;
            Ok(parse_search(&text)?
                .into_iter()
                .filter(|r| matches_query(r, &normalized))
                .collect())
        }
        .await;
        result.map_err(|error| response_error(self.id(), error))
    }
    async fn get_profile(&self, symbol: &str) -> Result<AssetProfile, MarketDataError> {
        let result: Result<_, MarketDataError> = async {
            let code = quote_symbol(symbol)
                .get(2..)
                .ok_or_else(|| unsupported(self.id()))?;
            let result = self
                .search(code)
                .await?
                .into_iter()
                .find(|r| r.symbol.eq_ignore_ascii_case(symbol))
                .ok_or_else(|| unsupported(self.id()))?;
            Ok(profile(result))
        }
        .await;
        result.map_err(|error| response_error(self.id(), error))
    }
    async fn get_latest_quote(
        &self,
        context: &QuoteContext,
        instrument: ProviderInstrument,
    ) -> Result<Quote, MarketDataError> {
        let result: Result<_, MarketDataError> = async {
            let symbol = instrument.to_symbol_string();
            let currency = context
                .currency_hint
                .as_deref()
                .ok_or_else(|| invalid("Quote currency missing"))?;
            let text = self
                .text(
                    "https://qt.gtimg.cn/",
                    &[("q", quote_symbol(&symbol).into())],
                )
                .await?;
            parse_quote(&text, &symbol, currency).map(|(q, _)| q)
        }
        .await;
        result.map_err(|error| response_error(self.id(), error))
    }
    async fn get_historical_quotes(
        &self,
        context: &QuoteContext,
        instrument: ProviderInstrument,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Vec<Quote>, MarketDataError> {
        let result: Result<_, MarketDataError> = async {
            let symbol = instrument.to_symbol_string();
            let currency = context
                .currency_hint
                .as_deref()
                .ok_or_else(|| invalid("History currency missing"))?;
            let mut quotes = Vec::new();
            for year in start.year()..=end.year() {
                let text = self
                    .text(
                        "https://proxy.finance.qq.com/ifzqgtimg/appstock/app/newfqkline/get",
                        &[(
                            "param",
                            format!("{symbol},day,{year}-01-01,{year}-12-31,640,"),
                        )],
                    )
                    .await?;
                let rows = parse_history(&json(&text)?, &symbol, currency)?;
                if rows.len() >= 640
                    && rows.first().is_some_and(|q| {
                        q.timestamp.date_naive()
                            > chrono::NaiveDate::from_ymd_opt(year, 1, 1).expect("valid year")
                    })
                {
                    return Err(invalid("Truncated Tencent history"));
                }
                quotes.extend(rows);
            }
            finish_history(quotes, start, end)
        }
        .await;
        result.map_err(|error| response_error(self.id(), error))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn search_decodes_name_without_losing_us_venue() {
        let r = parse_search(r#"v_hint="us~aapl.oq~\u82f9\u679c~pg~GP""#).unwrap();
        assert_eq!(r[0].name, "苹果");
        assert_eq!(r[0].exchange_mic.as_deref(), Some("XNAS"));
        assert_eq!(r[0].symbol, "usAAPL.OQ");
    }
    #[test]
    fn historical_adjustment_cannot_silently_change() {
        let value = serde_json::json!({"data":{"usAAPL":{"qfqday":[["2026-01-05","10","11","12","9","100"]]}}});
        assert!(parse_history(&value, "usAAPL", "USD").is_err());
        let value = serde_json::json!({"data":{"usAAPL":{"day":[["2026-01-05","10","11","12","9","100"]]}}});
        assert_eq!(
            parse_history(&value, "usAAPL", "USD").unwrap()[0].volume,
            Some(Decimal::from(100))
        );
    }
}
