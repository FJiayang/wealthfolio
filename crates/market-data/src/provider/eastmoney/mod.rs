//! Eastmoney / Tiantian Fund native APIs. No Python or proxy service is involved.
use super::{native::*, DATA_SOURCE_EASTMONEY};
use crate::{
    errors::MarketDataError, AssetProfile, DividendEvent, FinancialReport, FinancialStatement,
    InstrumentId, MarketDataProvider, NewsItem, ProviderCapabilities, ProviderInstrument, Quote,
    QuoteContext, RateLimit, SearchResult, SplitEvent,
};
use async_trait::async_trait;
use chrono::{DateTime, Datelike, NaiveDate, Utc};
use num_traits::ToPrimitive;
use rust_decimal::Decimal;
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub struct EastmoneyProvider {
    client: reqwest::Client,
    #[cfg(test)]
    responses: Option<std::sync::Mutex<std::collections::VecDeque<Value>>>,
}
impl EastmoneyProvider {
    pub fn new() -> Result<Self, MarketDataError> {
        Ok(Self {
            client: client()?,
            #[cfg(test)]
            responses: None,
        })
    }
    async fn request(
        &self,
        url: &str,
        params: &[(&str, String)],
    ) -> Result<Value, MarketDataError> {
        #[cfg(test)]
        if let Some(responses) = &self.responses {
            assert_eq!(url, "https://push2his.eastmoney.com/api/qt/stock/kline/get");
            return Ok(responses
                .lock()
                .expect("test response lock")
                .pop_front()
                .expect("unexpected history request"));
        }
        json(&get(&self.client, self.id(), url, params).await?)
    }
    async fn fund_search(&self, code: &str) -> Result<Vec<SearchResult>, MarketDataError> {
        if code.len() != 6 || !code.bytes().all(|b| b.is_ascii_digit()) {
            return Ok(vec![]);
        }
        let value = self
            .request(
                "https://fundmobapi.eastmoney.com/FundMNewApi/FundMNNBasicInformation",
                &[
                    ("FCODE", code.into()),
                    ("deviceid", "1".into()),
                    ("plat", "Iphone".into()),
                    ("product", "EFund".into()),
                    ("version", "6.3.8".into()),
                ],
            )
            .await?;
        let row = &value["Datas"];
        if row.is_null() {
            return Err(invalid("Fund metadata unavailable"));
        }
        if row["FCODE"].as_str() != Some(code) {
            return Err(invalid("Fund identity mismatch"));
        }
        let name = row["SHORTNAME"]
            .as_str()
            .ok_or_else(|| invalid("Fund name missing"))?;
        let kind = row["FTYPE"]
            .as_str()
            .ok_or_else(|| invalid("Fund category missing"))?;
        if kind.contains("货币") {
            return Err(unsupported(self.id()));
        }
        let currency = fund_currency(name)?;
        let symbol = format!("FUND:{code}");
        let mut result = SearchResult::new(&symbol, name, "FUND", "MUTUALFUND")
            .with_currency(currency)
            .with_data_source(self.id());
        result.canonical_symbol = Some(symbol);
        Ok(vec![result])
    }
    async fn fund_history(
        &self,
        code: &str,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        latest: bool,
    ) -> Result<Vec<Quote>, MarketDataError> {
        // Verify the share class before interpreting DWJZ (money-market funds use income).
        let currency = self
            .fund_search(code)
            .await?
            .into_iter()
            .next()
            .and_then(|r| r.currency)
            .ok_or_else(|| unsupported(self.id()))?;
        let mut quotes = Vec::new();
        let page_size = if latest { 1 } else { 20 };
        let mut page = 1;
        loop {
            let value = self
                .request(
                    "https://api.fund.eastmoney.com/f10/lsjz",
                    &[
                        ("fundCode", code.into()),
                        ("pageIndex", page.to_string()),
                        ("pageSize", page_size.to_string()),
                        ("startDate", start.format("%Y-%m-%d").to_string()),
                        ("endDate", end.format("%Y-%m-%d").to_string()),
                    ],
                )
                .await?;
            let rows = value
                .pointer("/Data/LSJZList")
                .and_then(Value::as_array)
                .ok_or_else(|| invalid("Invalid fund history"))?;
            let total = value["TotalCount"]
                .as_u64()
                .ok_or_else(|| invalid("Fund history count missing"))?;
            for row in rows {
                let day = date(
                    row["FSRQ"]
                        .as_str()
                        .ok_or_else(|| invalid("Fund date missing"))?,
                )?;
                let nav = decimal(&row["DWJZ"])?;
                if nav <= Decimal::ZERO {
                    return Err(invalid("Invalid unit NAV"));
                }
                quotes.push(Quote {
                    timestamp: midnight(day),
                    close: nav,
                    open: None,
                    high: None,
                    low: None,
                    volume: None,
                    currency: currency.clone(),
                    source: self.id().into(),
                });
            }
            if latest || page * page_size >= total {
                break;
            }
            if rows.is_empty() {
                return Err(invalid("Truncated fund history"));
            }
            page += 1;
        }
        finish_history(quotes, start, end)
    }
    async fn corporate_actions(
        &self,
        symbol: &str,
    ) -> Result<Vec<(NaiveDate, Decimal, Decimal)>, MarketDataError> {
        let code = symbol
            .strip_prefix("1.")
            .filter(|c| c.starts_with('6'))
            .or_else(|| {
                symbol
                    .strip_prefix("0.")
                    .filter(|c| c.starts_with('0') || c.starts_with('3'))
            })
            .filter(|c| c.len() == 6 && c.bytes().all(|b| b.is_ascii_digit()))
            .ok_or_else(|| unsupported(self.id()))?;
        let rows = self
            .financial_rows("RPT_SHAREBONUS_DET", &format!("(SECURITY_CODE=\"{code}\")"))
            .await?;
        rows.iter()
            .filter(|r| r["ASSIGN_PROGRESS"].as_str() == Some("实施分配"))
            .map(|r| {
                let day = date(
                    r["EX_DIVIDEND_DATE"]
                        .as_str()
                        .ok_or_else(|| invalid("Ex-dividend date missing"))?,
                )?;
                let cash = optional_decimal(&r["PRETAX_BONUS_RMB"])?;
                let shares = optional_decimal(&r["BONUS_IT_RATIO"])?;
                if cash.is_none() && shares.is_none() {
                    return Err(invalid("Corporate action amounts missing"));
                }
                Ok((
                    day,
                    cash.unwrap_or(Decimal::ZERO) / Decimal::TEN,
                    Decimal::ONE + shares.unwrap_or(Decimal::ZERO) / Decimal::TEN,
                ))
            })
            .collect()
    }
    async fn financial_rows(
        &self,
        report: &str,
        filter: &str,
    ) -> Result<Vec<Value>, MarketDataError> {
        let mut output = Vec::new();
        let mut page = 1;
        loop {
            let value = self
                .request(
                    if report == "RPT_SHAREBONUS_DET" {
                        "https://datacenter-web.eastmoney.com/api/data/v1/get"
                    } else {
                        "https://datacenter.eastmoney.com/securities/api/data/v1/get"
                    },
                    &[
                        ("reportName", report.into()),
                        ("columns", "ALL".into()),
                        ("filter", filter.into()),
                        ("pageNumber", page.to_string()),
                        ("pageSize", "500".into()),
                        ("source", "SECURITIES".into()),
                        ("client", "PC".into()),
                    ],
                )
                .await?;
            let rows = value
                .pointer("/result/data")
                .and_then(Value::as_array)
                .ok_or_else(|| invalid("Invalid financial report response"))?;
            let pages = value
                .pointer("/result/pages")
                .and_then(Value::as_u64)
                .ok_or_else(|| invalid("Report page count missing"))?;
            output.extend(rows.iter().cloned());
            if page >= pages {
                break;
            }
            if rows.is_empty() {
                return Err(invalid("Truncated financial report"));
            }
            page += 1;
        }
        Ok(output)
    }
}

fn fund_currency(name: &str) -> Result<&'static str, MarketDataError> {
    // These are explicit share-class denominations, not the portfolio's geographic exposure.
    if name.contains("人民币") {
        return Ok("CNY");
    }
    for (marker, currency) in [
        ("美元现汇", "USD"),
        ("美元现钞", "USD"),
        ("美元份额", "USD"),
        ("港币份额", "HKD"),
        ("港元份额", "HKD"),
        ("欧元份额", "EUR"),
    ] {
        if name.contains(marker) {
            return Ok(currency);
        }
    }
    if ["美元", "港币", "港元", "欧元"]
        .iter()
        .any(|s| name.contains(s))
    {
        return Err(unsupported(DATA_SOURCE_EASTMONEY));
    }
    Ok("CNY")
}

fn market(market: i64) -> Option<(&'static str, &'static str)> {
    match market {
        0 => Some(("XSHE", "CNY")),
        1 => Some(("XSHG", "CNY")),
        105 => Some(("XNAS", "USD")),
        106 => Some(("XNYS", "USD")),
        107 => Some(("US", "USD")),
        116 | 128 => Some(("XHKG", "HKD")),
        155 => Some(("XLON", "GBX")),
        _ => None,
    }
}
fn parse_search(value: &Value) -> Result<Vec<SearchResult>, MarketDataError> {
    let rows = value["result"]
        .as_array()
        .ok_or_else(|| invalid("Invalid securities directory"))?;
    let mut results = Vec::new();
    for row in rows {
        let Some(market_id) = row["market"].as_i64() else {
            continue;
        };
        let Some(code) = row["code"].as_str() else {
            continue;
        };
        let Some(name) = row["shortName"].as_str() else {
            continue;
        };
        let kind = row["securityTypeName"].as_str().unwrap_or("");
        if market_id == 150 && kind == "基金" {
            if name.contains("货币") {
                continue;
            }
            let Ok(currency) = fund_currency(name) else {
                continue;
            };
            let symbol = format!("FUND:{code}");
            let mut result = SearchResult::new(&symbol, name, "FUND", "MUTUALFUND")
                .with_currency(currency)
                .with_data_source(DATA_SOURCE_EASTMONEY);
            result.canonical_symbol = Some(symbol);
            results.push(result);
            continue;
        }

        if market_id == 100 && kind.contains("指数") {
            let symbol = format!("100.{code}");
            if let Some((canonical, currency)) = index_identity(DATA_SOURCE_EASTMONEY, &symbol) {
                let mut result = SearchResult::new(&symbol, name, "INDEX", "INDEX")
                    .with_currency(currency)
                    .with_data_source(DATA_SOURCE_EASTMONEY);
                result.canonical_symbol = Some(canonical.into());
                results.push(result);
            }
            continue;
        }
        let (mic, mut currency) = if kind.contains("京A") {
            ("XBEI", "CNY")
        } else if let Some(m) = market(market_id) {
            m
        } else {
            continue;
        };
        if kind == "沪B" {
            currency = "USD";
        } else if kind == "深B" {
            currency = "HKD";
        }
        let quote_type = if kind.contains("指数") {
            "INDEX"
        } else if kind.contains("ETF") || kind.contains("LOF") || kind == "基金" {
            "ETF"
        } else if matches!(
            kind,
            "沪A" | "深A" | "京A" | "美股" | "港股" | "英股" | "沪B" | "深B"
        ) {
            "EQUITY"
        } else {
            continue;
        };
        let mut result = SearchResult::new(format!("{market_id}.{code}"), name, mic, quote_type)
            .with_currency(currency)
            .with_data_source(DATA_SOURCE_EASTMONEY);
        result.canonical_symbol = Some(code.into());
        result.exchange_mic = crate::is_known_mic(mic).then(|| mic.into());
        results.push(result);
    }
    Ok(results)
}
fn parse_kline(value: &Value, currency: &str, symbol: &str) -> Result<Vec<Quote>, MarketDataError> {
    let status = value["rc"]
        .as_i64()
        .ok_or_else(|| invalid("History status missing or invalid"))?;
    if status != 0 {
        return Err(MarketDataError::ProviderError {
            provider: DATA_SOURCE_EASTMONEY.into(),
            message: format!("History request failed (rc={status})"),
        });
    }
    let data = value
        .get("data")
        .ok_or_else(|| invalid("History data missing"))?;
    if data.is_null() {
        return Ok(vec![]);
    }
    let code = symbol
        .split_once('.')
        .ok_or_else(|| unsupported(DATA_SOURCE_EASTMONEY))?
        .1;
    if value["data"]["code"].as_str() != Some(code) {
        return Err(invalid("History identity mismatch"));
    }
    let rows = value["data"]["klines"]
        .as_array()
        .ok_or_else(|| invalid("Missing historical bars"))?;
    rows.iter()
        .map(|row| {
            let fields: Vec<Value> = row
                .as_str()
                .ok_or_else(|| invalid("Invalid kline"))?
                .split(',')
                .map(|s| Value::String(s.into()))
                .collect();
            if fields.len() < 6 {
                return Err(invalid("Incomplete kline"));
            }
            let mut q = quote(
                date(fields[0].as_str().expect("string fields"))?,
                &fields[1..],
                currency,
                DATA_SOURCE_EASTMONEY,
            )?;
            if symbol.starts_with("0.") || symbol.starts_with("1.") {
                q.volume = q.volume.map(|v| v * Decimal::from(100));
            }
            Ok(q)
        })
        .collect()
}

fn parse_news(value: &Value) -> Result<Vec<NewsItem>, MarketDataError> {
    let rows = value
        .pointer("/result/cmsArticleWebOld")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid("Invalid news response"))?;
    rows.iter()
        .map(|r| {
            Ok(NewsItem {
                title: r["title"]
                    .as_str()
                    .ok_or_else(|| invalid("News title missing"))?
                    .into(),
                url: r["url"]
                    .as_str()
                    .map(str::to_owned)
                    .or_else(|| {
                        string(&r["code"])
                            .map(|c| format!("https://finance.eastmoney.com/a/{c}.html"))
                    })
                    .ok_or_else(|| invalid("News URL missing"))?,
                published_at: r["date"]
                    .as_str()
                    .map(|s| local_timestamp(s, "%Y-%m-%d %H:%M:%S", "CNY"))
                    .transpose()?,
                publisher: r["mediaName"].as_str().map(str::to_owned),
                summary: r["content"].as_str().map(str::to_owned),
                source: DATA_SOURCE_EASTMONEY.into(),
            })
        })
        .collect()
}

fn parse_reports(
    rows: Vec<Value>,
    statement: FinancialStatement,
) -> Result<Vec<FinancialReport>, MarketDataError> {
    let mut reports: BTreeMap<(NaiveDate, Option<String>), FinancialReport> = BTreeMap::new();
    for row in rows {
        let day = date(
            row["REPORT_DATE"]
                .as_str()
                .ok_or_else(|| invalid("Report date missing"))?,
        )?;
        let period_label = string(&row["REPORT"])
            .or_else(|| string(&row["REPORT_TYPE"]))
            .or_else(|| string(&row["DATE_TYPE_CODE"]));
        let entry = reports
            .entry((day, period_label.clone()))
            .or_insert(FinancialReport {
                period_label,
                statement,
                period_end: day,
                published_at: row["NOTICE_DATE"].as_str().map(date).transpose()?,
                currency: string(&row["CURRENCY"]),
                unit: string(&row["UNIT"]),
                items: BTreeMap::new(),
                source: DATA_SOURCE_EASTMONEY.into(),
            });
        if let Some(key) = string(&row["STD_ITEM_CODE"]).or_else(|| string(&row["ITEM_CODE"])) {
            entry.items.insert(key, row);
        } else {
            entry.items.extend(
                row.as_object()
                    .ok_or_else(|| invalid("Invalid report row"))?
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone())),
            );
        }
    }
    Ok(reports.into_values().collect())
}

#[async_trait]
impl MarketDataProvider for EastmoneyProvider {
    fn id(&self) -> &'static str {
        DATA_SOURCE_EASTMONEY
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
                Some(("FUND", code)) => return self.fund_search(code).await,
                Some((_, code)) => code,
                None => match normalized.as_str() {
                    "^GSPC" => "SPX",
                    "^DJI" => "DJIA",
                    "^IXIC" => "NDX",
                    "^HSI" => "HSI",
                    _ => query,
                },
            };
            let value = self
                .request(
                    "https://search-codetable.eastmoney.com/codetable/search/web",
                    &[
                        ("client", "web".into()),
                        ("clientType", "webSuggest".into()),
                        ("clientVersion", "lastest".into()),
                        ("keyword", keyword.into()),
                        ("pageIndex", "1".into()),
                        ("pageSize", "30".into()),
                    ],
                )
                .await?;
            if value["result"].is_null() {
                return Ok(vec![]);
            }
            Ok(parse_search(&value)?
                .into_iter()
                .filter(|r| matches_query(r, &normalized))
                .collect())
        }
        .await;
        result.map_err(|error| response_error(self.id(), error))
    }
    async fn get_profile(&self, symbol: &str) -> Result<AssetProfile, MarketDataError> {
        let result: Result<_, MarketDataError> = async {
            if let Some(code) = symbol.strip_prefix("FUND:") {
                return self
                    .fund_search(code)
                    .await?
                    .into_iter()
                    .next()
                    .map(profile)
                    .ok_or_else(|| unsupported(self.id()));
            }
            let (m, code) = symbol
                .split_once('.')
                .ok_or_else(|| unsupported(self.id()))?;
            let result = self
                .search(code)
                .await?
                .into_iter()
                .find(|r| r.symbol == symbol || (m == "116" && r.symbol == format!("128.{code}")))
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
            if let Some(code) = symbol.strip_prefix("FUND:") {
                return self
                    .fund_history(
                        code,
                        midnight(NaiveDate::from_ymd_opt(1990, 1, 1).expect("valid date")),
                        Utc::now(),
                        true,
                    )
                    .await?
                    .pop()
                    .ok_or(MarketDataError::NoDataForRange);
            }
            let value = self
                .request(
                    "https://push2.eastmoney.com/api/qt/stock/get",
                    &[
                        ("secid", symbol.clone()),
                        ("fltt", "2".into()),
                        ("invt", "2".into()),
                        ("fields", "f43,f44,f45,f46,f47,f57,f58,f86,f172".into()),
                    ],
                )
                .await?;
            let data = &value["data"];
            if data["f57"].as_str() != symbol.split_once('.').map(|(_, c)| c) {
                return Err(invalid("Quote identity mismatch"));
            }
            let currency = data["f172"]
                .as_str()
                .filter(|s| s.len() == 3)
                .or(context.currency_hint.as_deref())
                .ok_or_else(|| invalid("Quote currency missing"))?;
            let timestamp = data["f86"]
                .as_i64()
                .and_then(|t| DateTime::from_timestamp(t, 0))
                .ok_or_else(|| invalid("Quote time missing"))?;
            let mut q = quote(
                timestamp.date_naive(),
                &[
                    data["f46"].clone(),
                    data["f43"].clone(),
                    data["f44"].clone(),
                    data["f45"].clone(),
                    data["f47"].clone(),
                ],
                currency,
                self.id(),
            )?;
            q.timestamp = timestamp;
            if symbol.starts_with("0.") || symbol.starts_with("1.") {
                q.volume = q.volume.map(|v| v * Decimal::from(100));
            }
            Ok(q)
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
            if let Some(code) = symbol.strip_prefix("FUND:") {
                return self.fund_history(code, start, end, false).await;
            }
            let currency = context
                .currency_hint
                .as_deref()
                .ok_or_else(|| invalid("History currency missing"))?;
            let mut quotes = Vec::new();
            // A one-year window is below the endpoint's 1,000-row cap.
            for year in start.year()..=end.year() {
                let value = self
                    .request(
                        "https://push2his.eastmoney.com/api/qt/stock/kline/get",
                        &[
                            ("secid", symbol.clone()),
                            ("klt", "101".into()),
                            ("fqt", "0".into()),
                            ("lmt", "1000".into()),
                            ("beg", format!("{year}0101")),
                            ("end", format!("{year}1231")),
                            ("fields1", "f1,f2,f3,f4,f5,f6".into()),
                            ("fields2", "f51,f52,f53,f54,f55,f56".into()),
                        ],
                    )
                    .await?;
                let rows = parse_kline(&value, currency, &symbol)?;
                if rows.len() >= 1000 {
                    return Err(invalid("Truncated history"));
                }
                quotes.extend(rows);
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
            let rows = self
                .corporate_actions(&instrument.to_symbol_string())
                .await?;
            rows.into_iter()
                .filter(|(d, _, _)| *d >= start.date_naive() && *d <= end.date_naive())
                .filter(|(_, amount, _)| *amount > Decimal::ZERO)
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
    async fn get_news(&self, symbol: &str) -> Result<Vec<NewsItem>, MarketDataError> {
        let result: Result<_, MarketDataError> = async {

        let keyword = symbol.split_once('.').map_or(symbol, |(_, code)| code);
        let params = json!({"uid":"","keyword":keyword,"type":["cmsArticleWebOld"],"client":"web","clientType":"web","clientVersion":"curr","param":{"cmsArticleWebOld":{"searchScope":"default","sort":"default","pageIndex":1,"pageSize":10,"preTag":"","postTag":""}}});
        let value = self
            .request(
                "https://search-api-web.eastmoney.com/search/jsonp",
                &[("cb", "callback".into()), ("param", params.to_string())],
            )
            .await?;
        parse_news(&value)

        }.await;
        result.map_err(|error| response_error(self.id(), error))
    }
    async fn get_financial_reports(
        &self,
        symbol: &str,
        statement: FinancialStatement,
    ) -> Result<Vec<FinancialReport>, MarketDataError> {
        let result: Result<_, MarketDataError> = async {
            let (market, code) = symbol
                .split_once('.')
                .ok_or_else(|| unsupported(self.id()))?;
            if !code
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b".-_".contains(&b))
            {
                return Err(invalid("Invalid financial symbol"));
            }
            let report = match (market, statement) {
                ("0" | "1", FinancialStatement::BalanceSheet) => "RPT_DMSK_FN_BALANCE",
                ("0" | "1", FinancialStatement::IncomeStatement) => "RPT_DMSK_FN_INCOME",
                ("0" | "1", FinancialStatement::CashFlow) => "RPT_DMSK_FN_CASHFLOW",
                ("116" | "128", FinancialStatement::BalanceSheet) => "RPT_HKF10_FN_BALANCE_PC",
                ("116" | "128", FinancialStatement::IncomeStatement) => "RPT_HKF10_FN_INCOME_PC",
                ("116" | "128", FinancialStatement::CashFlow) => "RPT_HKF10_FN_CASHFLOW_PC",
                ("105" | "106" | "107", FinancialStatement::BalanceSheet) => "RPT_USF10_FN_BALANCE",
                ("105" | "106" | "107", FinancialStatement::IncomeStatement) => {
                    "RPT_USF10_FN_INCOME"
                }
                ("105" | "106" | "107", FinancialStatement::CashFlow) => "RPT_USSK_FN_CASHFLOW",
                _ => return Err(unsupported(self.id())),
            };
            let rows = self
                .financial_rows(report, &format!("(SECURITY_CODE=\"{code}\")"))
                .await?;
            parse_reports(rows, statement)
        }
        .await;
        result.map_err(|error| response_error(self.id(), error))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ordinary_search_retains_otc_fund_namespace() {
        let rows = parse_search(&json!({"result":[{"market":150,"code":"005827","shortName":"易方达蓝筹精选混合","securityTypeName":"基金"}]})).unwrap();
        assert_eq!(rows[0].canonical_symbol.as_deref(), Some("FUND:005827"));
        assert_eq!(rows[0].asset_type, "MUTUALFUND");
    }
    #[test]
    fn currency_describes_the_fund_share_class_not_its_exposure() {
        assert_eq!(fund_currency("美元债人民币A").unwrap(), "CNY");
        assert_eq!(fund_currency("全球债券A(美元现汇)").unwrap(), "USD");
        assert!(fund_currency("美元债A").is_err());
    }
    #[test]
    fn research_keeps_publication_time_units_and_fiscal_periods() {
        let news=parse_news(&json!({"result":{"cmsArticleWebOld":[{"title":"Results","code":"123","date":"2026-10-20 09:00:00","mediaName":"Issuer","content":"Summary"}]}})).unwrap();
        assert_eq!(
            news[0].published_at.unwrap().to_rfc3339(),
            "2026-10-20T01:00:00+00:00"
        );
        assert_eq!(news[0].url, "https://finance.eastmoney.com/a/123.html");
        let rows = vec![
            json!({"REPORT_DATE":"2025-12-31","REPORT":"2025/FY","CURRENCY":"USD","UNIT":"million","STD_ITEM_CODE":"revenue","AMOUNT":100}),
            json!({"REPORT_DATE":"2025-12-31","REPORT":"2025/Q4","CURRENCY":"USD","UNIT":"million","STD_ITEM_CODE":"revenue","AMOUNT":30}),
        ];
        let reports = parse_reports(rows, FinancialStatement::IncomeStatement).unwrap();
        assert_eq!(reports.len(), 2);
        assert_eq!(reports[0].currency.as_deref(), Some("USD"));
        assert_eq!(reports[0].unit.as_deref(), Some("million"));
        assert!(parse_reports(
            vec![json!({"REPORT_DATE":"invalid"})],
            FinancialStatement::BalanceSheet
        )
        .is_err());
    }
    #[test]
    fn global_indices_keep_a_cross_provider_identity() {
        let rows=parse_search(&json!({"result":[{"market":100,"code":"NDX","shortName":"Nasdaq Composite","securityTypeName":"指数"}]})).unwrap();
        assert_eq!(rows[0].canonical_symbol.as_deref(), Some("^IXIC"));
        assert_eq!(rows[0].asset_type, "INDEX");
        assert_eq!(rows[0].exchange_mic, None);
    }
    #[test]
    fn search_preserves_exchange_and_share_class() {
        let rows=parse_search(&json!({"result":[{"market":105,"code":"BRK.B","shortName":"Berkshire","securityTypeName":"美股"},{"market":1,"code":"600519","shortName":"茅台","securityTypeName":"沪A"}]})).unwrap();
        assert_eq!(rows[0].canonical_symbol.as_deref(), Some("BRK.B"));
        assert!(matches_query(&rows[1], "XSHG:600519"));
        assert!(!matches_query(&rows[1], "XSHE:600519"));
    }
    #[tokio::test]
    async fn failed_year_discards_partial_history_and_allows_failover() {
        use crate::errors::RetryClass;
        use chrono::TimeZone;
        let provider = EastmoneyProvider {
            client: client().unwrap(),
            responses: Some(std::sync::Mutex::new(std::collections::VecDeque::from([
                json!({"rc":0,"data":{"code":"600519","klines":["2025-12-31,10,12,13,9,5"]}}),
                json!({"rc":102,"data":null}),
            ]))),
        };
        let context = QuoteContext {
            instrument: InstrumentId::Equity {
                ticker: "600519".into(),
                mic: Some("XSHG".into()),
            },
            identifiers: Default::default(),
            overrides: None,
            currency_hint: Some("CNY".into()),
            preferred_provider: None,
            bond_metadata: None,
            custom_provider_code: None,
        };
        let result = provider
            .get_historical_quotes(
                &context,
                ProviderInstrument::EquitySymbol {
                    symbol: "1.600519".into(),
                },
                Utc.with_ymd_and_hms(2025, 12, 31, 0, 0, 0).unwrap(),
                Utc.with_ymd_and_hms(2026, 1, 5, 0, 0, 0).unwrap(),
            )
            .await;
        let error =
            result.expect_err("a failed year must not return the other year's partial history");
        assert_eq!(error.retry_class(), RetryClass::NextProvider);
        assert!(provider
            .responses
            .as_ref()
            .unwrap()
            .lock()
            .unwrap()
            .is_empty());
    }

    #[test]
    fn history_status_distinguishes_empty_data_from_failure() {
        assert!(parse_kline(&json!({"rc":0,"data":null}), "CNY", "1.600519")
            .unwrap()
            .is_empty());
        for response in [
            json!({"rc":102,"data":null}),
            json!({"data":null}),
            json!({"rc":"bad","data":null}),
            json!({"rc":0}),
        ] {
            assert!(parse_kline(&response, "CNY", "1.600519").is_err());
        }
    }

    #[test]
    fn volume_and_missing_prices_are_not_guessed() {
        let data = json!({"rc":0,"data":{"code":"600519","klines":["2026-01-05,10,12,13,9,5"]}});
        assert_eq!(
            parse_kline(&data, "CNY", "1.600519").unwrap()[0].volume,
            Some(Decimal::from(500))
        );
        assert_eq!(
            parse_kline(&data, "USD", "105.600519").unwrap()[0].volume,
            Some(Decimal::from(5))
        );
        assert!(parse_kline(&data, "CNY", "0.000001").is_err());
    }
}
