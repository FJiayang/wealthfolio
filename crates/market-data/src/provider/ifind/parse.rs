use super::{error, DATA_SOURCE_IFIND};
use crate::{errors::MarketDataError, AssetProfile, Quote, SearchResult};
use chrono::{DateTime, NaiveDate, NaiveDateTime, TimeZone, Utc};
use rust_decimal::Decimal;
use serde_json::Value;
use std::{collections::BTreeMap, str::FromStr};
type Row = BTreeMap<String, String>;

pub(super) fn data(value: Value) -> Result<Value, MarketDataError> {
    if value["code"].as_i64() != Some(1) {
        return Err(error(
            "iFinD rejected the request; check permissions and query",
        ));
    }
    let data = value
        .get("data")
        .filter(|v| v.is_object())
        .ok_or_else(|| error("Missing iFinD data"))?;
    if let Some(answer) = data["answer"].as_str() {
        if ["截断", "内容过长", "结果过多", "truncated"]
            .iter()
            .any(|s| answer.contains(s))
        {
            return Err(error("Truncated iFinD answer"));
        }
    }
    Ok(data.clone())
}
fn heading(s: &str) -> String {
    s.trim()
        .split(['（', '('])
        .next()
        .unwrap_or(s)
        .trim()
        .to_string()
}
fn tables(data: &Value) -> Result<Vec<Row>, MarketDataError> {
    if let Some(table) = data["tables"].as_array() {
        let mut lines = Vec::new();
        for row in table {
            lines.push(
                row.as_array()
                    .ok_or_else(|| error("Invalid quote table"))?
                    .iter()
                    .map(|v| {
                        v.as_str()
                            .map(str::to_string)
                            .ok_or_else(|| error("Invalid quote cell"))
                    })
                    .collect::<Result<Vec<_>, _>>()?,
            );
        }
        return rows(&lines);
    }
    let answer = data["answer"]
        .as_str()
        .ok_or_else(|| error("Missing answer table"))?;
    let mut output = Vec::new();
    let mut table = Vec::new();
    for line in answer.lines().chain(std::iter::once("")) {
        let line = line.trim();
        if line.starts_with('|') && line.ends_with('|') {
            let cells: Vec<String> = line[1..line.len() - 1]
                .split('|')
                .map(|s| s.trim().to_string())
                .collect();
            if cells.iter().all(|c| c.bytes().all(|b| b"-: ".contains(&b))) {
                continue;
            }
            table.push(cells);
        } else if !table.is_empty() {
            output.extend(rows(&table)?);
            table.clear();
        }
    }
    if output.is_empty() && data["selectedSecuritiesCount"].as_u64() != Some(0) {
        return Err(error("No structured table in answer"));
    }
    Ok(output)
}
fn rows(table: &[Vec<String>]) -> Result<Vec<Row>, MarketDataError> {
    let Some(headers) = table.first() else {
        return Err(error("Empty quote table"));
    };
    let headers: Vec<_> = headers.iter().map(|s| heading(s)).collect();
    if headers
        .iter()
        .collect::<std::collections::HashSet<_>>()
        .len()
        != headers.len()
    {
        return Err(error("Duplicate table columns"));
    }
    table
        .iter()
        .skip(1)
        .map(|row| {
            if row.len() != headers.len() {
                return Err(error("Incomplete table row"));
            }
            Ok(headers.iter().cloned().zip(row.iter().cloned()).collect())
        })
        .collect()
}
fn field<'a>(row: &'a Row, names: &[&str]) -> Option<&'a str> {
    names
        .iter()
        .find_map(|key| row.get(*key).map(String::as_str))
}
fn code(row: &Row) -> Option<&str> {
    field(row, &["证券代码", "股票代码", "基金代码", "指数代码"])
}
fn same_code(a: &str, b: &str) -> bool {
    if let (Some((a, "HK")), Some((b, "HK"))) = (a.rsplit_once('.'), b.rsplit_once('.')) {
        a.trim_start_matches('0') == b.trim_start_matches('0')
    } else {
        a == b
    }
}
fn selected(data: &Value, symbol: &str) -> Result<Vec<Row>, MarketDataError> {
    let rows: Vec<_> = tables(data)?
        .into_iter()
        .filter(|r| code(r).is_some_and(|c| same_code(c, symbol)))
        .collect();
    if rows.is_empty() {
        return Err(error("Response does not match requested security"));
    }
    Ok(rows)
}
fn currency(value: &str) -> Result<String, MarketDataError> {
    match value {
        "CNY" | "人民币" | "人民币元" => Ok("CNY".into()),
        "HKD" | "港币" | "港元" => Ok("HKD".into()),
        "USD" | "美元" => Ok("USD".into()),
        _ => Err(error("Unsupported or missing quote currency")),
    }
}
fn quote_currency(rows: &[Row]) -> Result<String, MarketDataError> {
    let mut found = None;
    for row in rows {
        if let Some(value) = field(row, &["单位净值币种", "交易币种"]) {
            let value = currency(value)?;
            if found.as_ref().is_some_and(|c| c != &value) {
                return Err(error("Conflicting quote currencies"));
            }
            found = Some(value);
        }
    }
    found.ok_or_else(|| error("Missing quote currency"))
}
fn number(value: &str) -> Result<Decimal, MarketDataError> {
    let (value, multiplier) = if let Some(value) = value.strip_suffix('万') {
        (value, 10000)
    } else if let Some(value) = value.strip_suffix('亿') {
        (value, 100000000)
    } else {
        (value, 1)
    };
    Decimal::from_str(&value.replace(',', ""))
        .ok()
        .and_then(|v| v.checked_mul(Decimal::from(multiplier)))
        .ok_or_else(|| error("Invalid numeric quote field"))
}
fn price(row: &Row, name: &str) -> Result<Decimal, MarketDataError> {
    let value = number(row.get(name).ok_or_else(|| error("Missing quote price"))?)?;
    if value <= Decimal::ZERO {
        return Err(error("Non-positive quote price"));
    }
    Ok(value)
}
fn date(value: &str) -> Result<NaiveDate, MarketDataError> {
    NaiveDate::parse_from_str(value, "%Y%m%d")
        .or_else(|_| NaiveDate::parse_from_str(value, "%Y-%m-%d"))
        .map_err(|_| error("Invalid trade date"))
}
fn midnight(day: NaiveDate) -> DateTime<Utc> {
    day.and_hms_opt(0, 0, 0)
        .expect("midnight is valid")
        .and_utc()
}
fn mic(code: &str) -> Result<&'static str, MarketDataError> {
    match code.rsplit_once('.').map(|(_, s)| s) {
        Some("SH") => Ok("XSHG"),
        Some("SZ") => Ok("XSHE"),
        Some("BJ") => Ok("XBSE"),
        Some("HK") => Ok("XHKG"),
        Some("O") => Ok("XNAS"),
        Some("N") => Ok("XNYS"),
        Some("A") => Ok("XASE"),
        Some("OF") => Ok(""),
        _ => Err(error("Unknown listing suffix")),
    }
}
pub(super) fn canonical(code: &str, kind: &str) -> Result<String, MarketDataError> {
    let (ticker, _) = code
        .rsplit_once('.')
        .ok_or_else(|| error("Missing listing suffix"))?;
    if kind == "nav" {
        return Ok(format!("FUND:{ticker}"));
    }
    if mic(code)? == "XHKG" {
        return Ok(format!("{ticker:0>5}"));
    }
    Ok(ticker.into())
}
pub(super) fn profile(
    data: &Value,
    kind: &str,
    symbol: &str,
) -> Result<AssetProfile, MarketDataError> {
    let rows = selected(data, symbol)?;
    let category_field = match kind {
        "stock" => "股票种类",
        "fund" | "nav" => "基金简称",
        "index" => "指数简称",
        _ => "证券类型",
    };
    if !rows.iter().any(|row| row.contains_key(category_field)) {
        return Err(error("Response does not confirm security category"));
    }
    let name = rows
        .iter()
        .find_map(|r| field(r, &["证券简称", "股票简称", "基金简称", "指数简称"]))
        .ok_or_else(|| error("Missing security name"))?;
    Ok(AssetProfile {
        name: Some(name.into()),
        quote_type: Some(
            match kind {
                "index" => "INDEX",
                "nav" => "MUTUALFUND",
                "fund" => {
                    if symbol.starts_with('5') {
                        "ETF"
                    } else {
                        "MUTUALFUND"
                    }
                }
                _ => "EQUITY",
            }
            .into(),
        ),
        currency: Some(quote_currency(&rows)?),
        exchange: Some(mic(symbol)?.into()),
        source: Some(DATA_SOURCE_IFIND.into()),
        ..Default::default()
    })
}
pub(super) fn search(data: &Value, kind: &str) -> Result<Vec<SearchResult>, MarketDataError> {
    let mut results = Vec::new();
    for row in tables(data)? {
        let Some(code) = code(&row) else {
            continue;
        };
        let Some(name) = field(&row, &["证券简称", "股票简称", "基金简称", "指数简称"])
        else {
            continue;
        };
        let kind = if code.ends_with(".OF") { "nav" } else { kind };
        let symbol = format!("{kind}:{code}");
        super::identity(&symbol)?;
        let exchange = mic(code)?;
        let mut result = SearchResult::new(
            symbol,
            name,
            exchange,
            match kind {
                "index" => "INDEX",
                "nav" => "MUTUALFUND",
                "fund" => {
                    if code.starts_with('5') {
                        "ETF"
                    } else {
                        "MUTUALFUND"
                    }
                }
                _ => "EQUITY",
            },
        )
        .with_data_source(DATA_SOURCE_IFIND);
        result.canonical_symbol = Some(canonical(code, kind)?);
        if !exchange.is_empty() {
            result.exchange_mic = Some(exchange.into());
        }
        results.push(result);
    }
    Ok(results)
}
pub(super) fn latest(data: &Value, symbol: &str, currency: &str) -> Result<Quote, MarketDataError> {
    let rows = selected(data, symbol)?;
    if rows.len() != 1 {
        return Err(error("Expected one current quote"));
    }
    let row = &rows[0];
    let time = NaiveDateTime::parse_from_str(
        field(row, &["time"]).ok_or_else(|| error("Missing quote time"))?,
        "%Y-%m-%d %H:%M:%S",
    )
    .map_err(|_| error("Invalid quote time"))?;
    let timestamp = chrono_tz::Asia::Shanghai
        .from_local_datetime(&time)
        .single()
        .ok_or_else(|| error("Ambiguous quote time"))?
        .with_timezone(&Utc);
    let mut quote = Quote::new(
        timestamp,
        price(row, "最新价")?,
        currency.into(),
        DATA_SOURCE_IFIND.into(),
    );
    quote.open = Some(price(row, "开盘价")?);
    quote.high = Some(price(row, "最高价")?);
    quote.low = Some(price(row, "最低价")?);
    Ok(quote)
}
pub(super) fn history(
    data: &Value,
    kind: &str,
    symbol: &str,
    start: NaiveDate,
    end: NaiveDate,
) -> Result<Vec<Quote>, MarketDataError> {
    let rows = selected(data, symbol)?;
    let currency = quote_currency(&rows)?;
    let close = if kind == "nav" {
        "单位净值"
    } else {
        "收盘价"
    };
    if kind != "nav" && kind != "index" {
        let params = data["indicators_params"]
            .as_object()
            .ok_or_else(|| error("Missing adjustment metadata"))?;
        for name in ["开盘价", "最高价", "最低价", "收盘价"] {
            if !params.iter().any(|(key, value)| {
                heading(key) == name && value["复权方式"].as_str() == Some("不复权")
            }) {
                return Err(error("Unverified unadjusted history"));
            }
        }
    }
    let mut quotes = BTreeMap::new();
    for row in rows.iter().filter(|row| row.contains_key(close)) {
        let day = date(field(row, &["日期"]).ok_or_else(|| error("Missing historical date"))?)?;
        if day < start || day > end {
            return Err(error("History outside requested window"));
        }
        // The service can forward-fill a close on non-trading days, without OHLC or volume.
        if kind != "nav"
            && ["开盘价", "最高价", "最低价", "成交量"]
                .iter()
                .all(|key| row.get(*key).is_none_or(|v| v.trim().is_empty()))
        {
            continue;
        }

        let mut quote = Quote::new(
            midnight(day),
            price(row, close)?,
            currency.clone(),
            DATA_SOURCE_IFIND.into(),
        );
        if kind != "nav" {
            quote.open = Some(price(row, "开盘价")?);
            quote.high = Some(price(row, "最高价")?);
            quote.low = Some(price(row, "最低价")?);
        }
        if let Some(volume) = row.get("成交量") {
            if data["indicators_params"]["成交量"]["单位"].as_str() != Some("股") {
                return Err(error("Unverified volume unit"));
            }
            let volume = number(volume)?;
            if volume < Decimal::ZERO {
                return Err(error("Negative volume"));
            }
            quote.volume = Some(volume);
        }
        if let Some(previous) = quotes.insert(day, quote.clone()) {
            if previous.close != quote.close
                || previous.open != quote.open
                || previous.high != quote.high
                || previous.low != quote.low
                || previous.volume != quote.volume
            {
                return Err(error("Conflicting history rows"));
            }
        }
    }
    if quotes.is_empty() {
        return Err(MarketDataError::NoDataForRange);
    }
    Ok(quotes.into_values().collect())
}

pub(super) fn latest_daily(
    data: &Value,
    kind: &str,
    symbol: &str,
) -> Result<Quote, MarketDataError> {
    let rows = selected(data, symbol)?;
    let currency = quote_currency(&rows)?;
    let date_field = if kind == "nav" {
        "净值日期"
    } else {
        "最近交易日期"
    };
    let close = if kind == "nav" {
        "单位净值"
    } else {
        "收盘价"
    };
    let mut days = rows
        .iter()
        .filter_map(|row| row.get(date_field))
        .map(|value| date(value))
        .collect::<Result<Vec<_>, _>>()?;
    days.sort();
    days.dedup();
    if days.len() != 1 {
        return Err(error("Missing or conflicting publication date"));
    }
    let prices = rows
        .iter()
        .filter(|row| row.contains_key(close))
        .map(|row| price(row, close))
        .collect::<Result<Vec<_>, _>>()?;
    if prices.len() != 1 {
        return Err(error("Missing or ambiguous latest published price"));
    }
    if kind == "global"
        && !data["indicators_params"].as_object().is_some_and(|params| {
            params.iter().any(|(name, value)| {
                heading(name) == close && value["复权方式"].as_str() == Some("不复权")
            })
        })
    {
        return Err(error("Unverified unadjusted latest price"));
    }
    Ok(Quote::new(
        midnight(days[0]),
        prices[0],
        currency,
        DATA_SOURCE_IFIND.into(),
    ))
}
