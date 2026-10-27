//! Sina native market data and financial statements.
mod decode;
use super::{native::*, DATA_SOURCE_SINA};
use crate::{
    errors::MarketDataError, AssetProfile, DividendEvent, FinancialReport, FinancialStatement,
    InstrumentId, MarketDataProvider, ProviderCapabilities, ProviderInstrument, Quote,
    QuoteContext, RateLimit, SearchResult, SplitEvent,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use num_traits::ToPrimitive;
use rust_decimal::Decimal;
use scraper::{Html, Selector};
use serde_json::Value;
use std::collections::BTreeMap;

pub struct SinaProvider {
    request_recorder: std::sync::Arc<dyn crate::RequestRecorder>,
    client: reqwest::Client,
}
impl SinaProvider {
    /// Attach this profile's recorder without changing provider request behavior.
    pub fn with_request_recorder(
        mut self,
        recorder: std::sync::Arc<dyn crate::RequestRecorder>,
    ) -> Self {
        self.request_recorder = recorder;
        self
    }

    pub fn new() -> Result<Self, MarketDataError> {
        Ok(Self {
            client: client()?,
            request_recorder: std::sync::Arc::new(crate::NoopRequestRecorder),
        })
    }
    async fn corporate_actions(
        &self,
        symbol: &str,
    ) -> Result<Vec<(chrono::NaiveDate, Decimal, Decimal)>, MarketDataError> {
        if !(symbol.starts_with("sh6") || symbol.starts_with("sz0") || symbol.starts_with("sz3"))
            || symbol.len() != 8
            || !symbol[2..].bytes().all(|b| b.is_ascii_digit())
        {
            return Err(unsupported(self.id()));
        }
        let text=self.text(&format!("https://vip.stock.finance.sina.com.cn/corp/go.php/vISSUE_ShareBonus/stockid/{}.phtml",&symbol[2..]),&[]).await?;
        parse_actions(&text)
    }
    async fn text(&self, url: &str, params: &[(&str, String)]) -> Result<String, MarketDataError> {
        get(
            &self.client,
            self.id(),
            url,
            params,
            self.request_recorder.as_ref(),
            &diagnostic_context(url, params),
        )
        .await
    }
}
fn parse_actions(
    text: &str,
) -> Result<Vec<(chrono::NaiveDate, Decimal, Decimal)>, MarketDataError> {
    let html = Html::parse_document(text);
    let table = Selector::parse("#sharebonus_1").expect("fixed selector");
    let rows = Selector::parse("tbody tr").expect("fixed selector");
    let cells = Selector::parse("td").expect("fixed selector");
    let table = html
        .select(&table)
        .next()
        .ok_or_else(|| invalid("Dividend table missing"))?;
    let mut actions = Vec::new();
    for row in table.select(&rows) {
        let f: Vec<String> = row
            .select(&cells)
            .map(|c| c.text().collect::<String>().trim().to_owned())
            .collect();
        if f.len() == 1 && f[0].contains("没有数据") {
            continue;
        }
        if f.len() < 8 {
            return Err(invalid("Invalid dividend row"));
        }
        if f[4] != "实施" {
            continue;
        }
        let bonus = decimal(&Value::String(f[1].clone()))? + decimal(&Value::String(f[2].clone()))?;
        let cash = decimal(&Value::String(f[3].clone()))?;
        actions.push((
            date(&f[5])?,
            cash / Decimal::TEN,
            Decimal::ONE + bonus / Decimal::TEN,
        ));
    }
    Ok(actions)
}
fn assignment(text: &str) -> Result<String, MarketDataError> {
    let (_, value) = text
        .split_once('=')
        .ok_or_else(|| invalid("Invalid Sina assignment"))?;
    serde_json::Deserializer::from_str(value.trim())
        .into_iter::<String>()
        .next()
        .ok_or_else(|| invalid("Missing Sina string"))?
        .map_err(|_| invalid("Invalid Sina string"))
}
fn parse_search(text: &str) -> Result<Vec<SearchResult>, MarketDataError> {
    let text = assignment(text)?;
    let mut results = Vec::new();
    for entry in text.split(';') {
        let f: Vec<_> = entry.split(',').collect();
        if f.len() < 5 {
            continue;
        }
        let (native, code, mic, currency, kind) = match f[1] {
            "11" | "12" | "13" | "21" | "22" | "23" | "203" => {
                let native = if f[3].starts_with("of") && matches!(f[1], "22" | "23") {
                    if f[2].starts_with('5') {
                        format!("sh{}", f[2])
                    } else if f[2].starts_with('1') {
                        format!("sz{}", f[2])
                    } else {
                        continue;
                    }
                } else {
                    f[3].to_owned()
                };
                let mic = if native.starts_with("sh") {
                    "XSHG"
                } else if native.starts_with("sz") {
                    "XSHE"
                } else if native.starts_with("bj") {
                    "XBEI"
                } else {
                    continue;
                };
                let kind = if matches!(f[1], "22" | "23" | "203") {
                    "ETF"
                } else if native.starts_with("sh000") || native.starts_with("sz399") {
                    "INDEX"
                } else {
                    "EQUITY"
                };
                let currency = if native.starts_with("sh900") {
                    "USD"
                } else if native.starts_with("sz200") {
                    "HKD"
                } else {
                    "CNY"
                };
                (native, f[2].to_owned(), Some(mic), currency, kind)
            }
            "33" => (
                format!("hk{}", f[2].to_uppercase()),
                f[2].to_uppercase(),
                None,
                "HKD",
                "INDEX",
            ),
            "31" => (
                format!("hk{}", f[2]),
                f[2].into(),
                Some("XHKG"),
                "HKD",
                "EQUITY",
            ),
            "41" => (
                format!(
                    "gb_{}",
                    if f[2].starts_with('.') {
                        f[2].to_lowercase().replacen('.', "$", 1)
                    } else {
                        f[2].to_lowercase()
                    }
                ),
                f[2].to_uppercase(),
                None,
                "USD",
                "EQUITY",
            ),
            _ => continue,
        };
        let mut r = SearchResult::new(native, f[4], mic.unwrap_or("US"), kind)
            .with_currency(currency)
            .with_data_source(DATA_SOURCE_SINA);
        if let Some((canonical, ccy)) = index_identity(DATA_SOURCE_SINA, &r.symbol) {
            r.canonical_symbol = Some(canonical.into());
            r.asset_type = "INDEX".into();
            r.currency = Some(ccy.into());
            results.push(r);
            continue;
        }
        r.canonical_symbol = Some(code);
        r.exchange_mic = mic.map(str::to_owned);
        results.push(r);
    }
    let mut seen = std::collections::HashSet::new();
    results.retain(|r| seen.insert(r.symbol.clone()));
    Ok(results)
}
fn parse_latest(text: &str, symbol: &str, currency: &str) -> Result<Quote, MarketDataError> {
    let key = text
        .split_once('=')
        .ok_or_else(|| invalid("Invalid Sina quote"))?
        .0
        .trim();
    if key != format!("var hq_str_{symbol}") {
        return Err(invalid("Sina quote identity mismatch"));
    }
    let text = assignment(text)?;
    let f: Vec<_> = text.split(',').collect();
    let (indices, stamp, format, zone) = if symbol.starts_with("gb_") {
        if f.len() < 30 {
            return Err(invalid("Incomplete US quote"));
        }
        let local = f[25]
            .rsplit_once(' ')
            .map(|(time, _)| time)
            .ok_or_else(|| invalid("US trade date missing"))?;
        (
            [5, 1, 6, 7, 10],
            format!("{} {local}", f[29]),
            "%Y %b %d %I:%M%p",
            "USD",
        )
    } else if symbol.starts_with("hk") {
        if f.len() < 19 {
            return Err(invalid("Incomplete HK quote"));
        }
        (
            [2, 6, 4, 5, 12],
            format!("{} {}", f[17], f[18]),
            if f[18].len() == 5 {
                "%Y/%m/%d %H:%M"
            } else {
                "%Y/%m/%d %H:%M:%S"
            },
            "HKD",
        )
    } else {
        if f.len() < 32 {
            return Err(invalid("Incomplete China quote"));
        }
        (
            [1, 3, 4, 5, 8],
            format!("{} {}", f[30], f[31]),
            "%Y-%m-%d %H:%M:%S",
            "CNY",
        )
    };
    let timestamp = local_timestamp(&stamp, format, zone)?;
    let values = indices.map(|i| Value::String(f[i].into()));
    let mut q = quote(timestamp.date_naive(), &values, currency, DATA_SOURCE_SINA)?;
    q.timestamp = timestamp;
    Ok(q)
}
#[async_trait]
impl MarketDataProvider for SinaProvider {
    fn id(&self) -> &'static str {
        DATA_SOURCE_SINA
    }
    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            supports_dividends: true,
            ..capabilities()
        }
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
                    "^GSPC" => ".INX",
                    "^DJI" => ".DJI",
                    "^IXIC" => ".IXIC",
                    "^HSI" => "HSI",
                    _ => query,
                },
            };
            let text = self
                .text(
                    "https://suggest3.sinajs.cn/suggest/",
                    &[("type", "".into()), ("key", keyword.into())],
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
            let code = symbol
                .strip_prefix("gb_")
                .or_else(|| symbol.get(2..))
                .ok_or_else(|| unsupported(self.id()))?;
            let result = self
                .search(&code.replace('$', "."))
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
            if !symbol
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'$' | b'.'))
            {
                return Err(unsupported(self.id()));
            }
            // Sina treats percent-encoded dollar signs as literal symbol characters.
            let text = self
                .text(&format!("https://hq.sinajs.cn/list={symbol}"), &[])
                .await?;
            parse_latest(&text, &symbol, currency)
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
            let url = if let Some(code) = symbol.strip_prefix("gb_") {
                format!(
                    "https://finance.sina.com.cn/staticdata/us/{}",
                    code.to_uppercase().replace('$', ".")
                )
            } else if let Some(code) = symbol.strip_prefix("hk") {
                format!("https://finance.sina.com.cn/stock/hkstock/{code}/klc2_kl.js")
            } else {
                format!("https://finance.sina.com.cn/realstock/company/{symbol}/hisdata/klc_kl.js")
            };
            let text = self.text(&url, &[]).await?;
            let bars = decode::decode(&assignment(&text)?)?;
            let mut quotes = Vec::with_capacity(bars.len());
            for b in bars {
                if b.close <= Decimal::ZERO || b.volume < Decimal::ZERO {
                    return Err(invalid("Invalid decoded bar"));
                }
                quotes.push(Quote {
                    timestamp: midnight(b.date),
                    open: Some(b.open),
                    high: Some(b.high),
                    low: Some(b.low),
                    close: b.close,
                    volume: Some(b.volume),
                    currency: currency.into(),
                    source: self.id().into(),
                });
            }
            finish_history(quotes, start, end)
        }
        .await;
        result.map_err(|error| response_error(self.id(), error))
    }
    async fn get_dividends(
        &self,
        _context: &QuoteContext,
        instrument: ProviderInstrument,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Vec<DividendEvent>, MarketDataError> {
        let result: Result<_, MarketDataError> = async {
            self.corporate_actions(&instrument.to_symbol_string())
                .await?
                .into_iter()
                .filter(|(d, amount, _)| {
                    *d >= start.date_naive() && *d <= end.date_naive() && *amount > Decimal::ZERO
                })
                .map(|(day, amount, _)| {
                    Ok(DividendEvent {
                        date: midnight(day).timestamp(),
                        amount: amount
                            .to_f64()
                            .ok_or_else(|| invalid("Dividend amount overflow"))?,
                    })
                })
                .collect()
        }
        .await;
        result.map_err(|error| response_error(self.id(), error))
    }
    async fn get_splits(
        &self,
        _context: &QuoteContext,
        instrument: ProviderInstrument,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Vec<SplitEvent>, MarketDataError> {
        let result: Result<_, MarketDataError> = async {
            Ok(self
                .corporate_actions(&instrument.to_symbol_string())
                .await?
                .into_iter()
                .filter(|(d, _, ratio)| {
                    *d >= start.date_naive() && *d <= end.date_naive() && *ratio != Decimal::ONE
                })
                .map(|(date, _, ratio)| SplitEvent { date, ratio })
                .collect())
        }
        .await;
        result.map_err(|error| response_error(self.id(), error))
    }
    async fn get_financial_reports(
        &self,
        symbol: &str,
        statement: FinancialStatement,
    ) -> Result<Vec<FinancialReport>, MarketDataError> {
        let result: Result<_, MarketDataError> = async {

        if !(symbol.starts_with("sh") || symbol.starts_with("sz") || symbol.starts_with("bj"))
            || !symbol[2..].bytes().all(|b| b.is_ascii_digit())
        {
            return Err(unsupported(self.id()));
        }
        let source = match statement {
            FinancialStatement::BalanceSheet => "fzb",
            FinancialStatement::IncomeStatement => "lrb",
            FinancialStatement::CashFlow => "llb",
        };
        let mut reports = Vec::new();
        let mut page = 1;
        loop {
            let text=self.text("https://quotes.sina.cn/cn/api/openapi.php/CompanyFinanceService.getFinanceReport2022",&[("paperCode",symbol.into()),("source",source.into()),("type","0".into()),("page",page.to_string()),("num","100".into())]).await?;
            let value = json(&text)?;
            let dates = value
                .pointer("/result/data/report_date")
                .and_then(Value::as_array)
                .ok_or_else(|| invalid("Financial dates missing"))?;
            for d in dates {
                let key = d["date_value"]
                    .as_str()
                    .ok_or_else(|| invalid("Report date missing"))?;
                let row = &value["result"]["data"]["report_list"][key];
                let mut items = BTreeMap::new();
                for item in row["data"]
                    .as_array()
                    .ok_or_else(|| invalid("Report items missing"))?
                {
                    let title = item["item_title"]
                        .as_str()
                        .ok_or_else(|| invalid("Report item name missing"))?;
                    items.insert(title.into(), item["item_value"].clone());
                }
                reports.push(FinancialReport {
                    statement,
                    period_end: date(key)?,
                    period_label: string(&row["rType"]),
                    published_at: row["publish_date"]
                        .as_str()
                        .filter(|s| !s.is_empty())
                        .map(date)
                        .transpose()?,
                    currency: string(&row["rCurrency"]),
                    unit: None,
                    items,
                    source: self.id().into(),
                });
            }
            if dates.len() < 100 {
                break;
            }
            page += 1;
        }
        Ok(reports)

        }.await;
        result.map_err(|error| response_error(self.id(), error))
    }
}
fn diagnostic_context(url: &str, params: &[(&str, String)]) -> crate::RequestContext {
    let parsed =
        reqwest::Url::parse(url).expect("provider URL is constructed from a fixed endpoint");
    let parameters: serde_json::Map<String, serde_json::Value> = parsed
        .query_pairs()
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .chain(
            params
                .iter()
                .map(|(key, value)| (key.to_string(), value.clone())),
        )
        .filter(|(key, _)| {
            [
                "list",
                "symbol",
                "paperCode",
                "source",
                "type",
                "page",
                "num",
                "key",
                "d",
                "scale",
                "datalen",
                "ma",
                "rn",
            ]
            .contains(&key.as_str())
        })
        .map(|(key, value)| (key, serde_json::json!(value)))
        .collect();
    let mut symbols: Vec<String> = ["list", "symbol", "paperCode"]
        .iter()
        .filter_map(|key| parameters.get(*key).and_then(|value| value.as_str()))
        .flat_map(|value| value.split(','))
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .collect();
    if let Some(code) = parsed
        .path()
        .split("/stockid/")
        .nth(1)
        .and_then(|value| value.strip_suffix(".phtml"))
    {
        symbols.push(code.into());
    }
    if let Some(code) = parsed.path().strip_prefix("/list=") {
        symbols.push(code.into());
    }
    if let Some(code) = parsed.path().strip_prefix("/staticdata/us/") {
        symbols.push(format!("gb_{}", code.to_lowercase()));
    }
    if let Some(code) = parsed
        .path()
        .strip_prefix("/stock/hkstock/")
        .and_then(|value| value.split('/').next())
    {
        symbols.push(format!("hk{code}"));
    }
    if let Some(code) = parsed
        .path()
        .strip_prefix("/realstock/company/")
        .and_then(|value| value.split('/').next())
    {
        symbols.push(code.into());
    }
    crate::RequestContext::http(parsed.path(), symbols, parameters.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn corporate_actions_use_per_share_cash_and_ignore_proposals() {
        let rows=parse_actions(r#"<table id="sharebonus_1"><tbody>
            <tr><td>2026-01-01</td><td>1</td><td>2</td><td>5</td><td>实施</td><td>2026-01-05</td><td>2026-01-04</td><td>2026-01-06</td></tr>
            <tr><td>2026-01-01</td><td>0</td><td>0</td><td>99</td><td>预案</td><td>--</td><td>--</td><td>--</td></tr>
        </tbody></table>"#).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].1, Decimal::from_str_exact("0.5").unwrap());
        assert_eq!(rows[0].2, Decimal::from_str_exact("1.3").unwrap());
    }
    #[test]
    fn listed_etf_does_not_become_an_otc_fund_or_stock() {
        let rows=parse_search(r#"var suggestvalue="ETF,203,510300,sh510300,ETF,,ETF,99,1,,,;上证指数,11,000001,sh000001,上证指数,,上证指数,99,1,,,;";"#).unwrap();
        assert_eq!(rows[0].asset_type, "ETF");
        assert_eq!(rows[0].canonical_symbol.as_deref(), Some("510300"));
        assert_eq!(rows[1].asset_type, "INDEX");
        assert_eq!(rows[1].exchange_mic.as_deref(), Some("XSHG"));
    }
    #[test]
    fn bare_us_search_does_not_invent_an_exchange() {
        let rows =
            parse_search(r#"var suggestvalue="AAPL,41,aapl,aapl,苹果,,苹果,99,1,,,;";"#).unwrap();
        assert_eq!(rows[0].symbol, "gb_aapl");
        assert_eq!(rows[0].canonical_symbol.as_deref(), Some("AAPL"));
        assert_eq!(rows[0].exchange_mic, None);
    }
    #[test]
    fn quote_identity_mismatch_is_rejected() {
        assert!(parse_latest(r#"var hq_str_sh000001="";"#, "sh600519", "CNY").is_err());
    }
}
#[cfg(test)]
mod diagnostic_tests {
    use super::*;
    #[test]
    fn diagnostics_select_business_parameters_without_credentials() {
        let context = diagnostic_context(
            "https://hq.sinajs.cn/list=sh600519?token=private",
            &[("unexpected_private_field", "private".into())],
        );
        assert_eq!(context.symbols, ["sh600519"]);
        let encoded = serde_json::to_string(&context).unwrap();
        assert!(!encoded.contains("private"));
        assert!(!encoded.contains("unexpected_private_field"));
    }
}
