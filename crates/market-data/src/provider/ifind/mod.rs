//! iFinD's MCP service is a transport detail of this ordinary market-data provider.
mod parse;
#[cfg(test)]
mod tests;

use super::DATA_SOURCE_IFIND;
use crate::{
    errors::MarketDataError, AssetProfile, Coverage, InstrumentKind, MarketDataProvider,
    ProviderCapabilities, ProviderInstrument, Quote, QuoteContext, RateLimit, SearchResult,
};
use async_trait::async_trait;
use chrono::{DateTime, Days, Utc};
use rmcp::{
    model::{
        CallToolRequestParams, ClientCapabilities, ClientInfo, Implementation, ProtocolVersion,
    },
    service::{RoleClient, RunningService},
    transport::{
        streamable_http_client::StreamableHttpClientTransportConfig, StreamableHttpClientTransport,
    },
    ServiceExt,
};
use serde_json::{json, Value};
use std::{collections::HashMap, sync::Arc, time::Duration};
use tokio::{sync::Mutex, time::Instant};

const BASE: &str = "https://api-mcp.51ifind.com:8643/ds-mcp-servers";
#[cfg(not(test))]
const TIMEOUT: Duration = Duration::from_secs(65);
#[cfg(test)]
const TIMEOUT: Duration = Duration::from_secs(2);
type Connection = RunningService<RoleClient, ClientInfo>;

#[derive(Default)]
struct State {
    connections: HashMap<&'static str, Connection>,
    last_call: Option<Instant>,
}

pub struct IfindProvider {
    client: reqwest::Client,
    authorization: reqwest::header::HeaderValue,
    state: Mutex<State>,
    base: String,
}

fn error(message: &str) -> MarketDataError {
    MarketDataError::ProviderError {
        provider: DATA_SOURCE_IFIND.into(),
        message: message.into(),
    }
}
fn unsupported() -> MarketDataError {
    MarketDataError::NotSupported {
        provider: DATA_SOURCE_IFIND.into(),
        operation: "instrument".into(),
    }
}

impl IfindProvider {
    pub fn new(key: String) -> Result<Self, MarketDataError> {
        let key = key.trim();
        if key.is_empty() {
            return Err(error("API key is empty"));
        }
        let mut authorization = reqwest::header::HeaderValue::from_str(key)
            .map_err(|_| error("Invalid API key header"))?;
        authorization.set_sensitive(true);
        let client = wealthfolio_http::client_builder()
            .user_agent("Wealthfolio")
            .timeout(TIMEOUT)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| error("HTTP client initialization failed"))?;
        Ok(Self {
            client,
            authorization,
            state: Mutex::new(State::default()),
            base: BASE.into(),
        })
    }

    async fn call(
        &self,
        service: &'static str,
        tool: &str,
        arguments: Value,
    ) -> Result<Value, MarketDataError> {
        // One gate covers every service: the registry's token bucket cannot limit in-flight calls.
        let mut state = self.state.lock().await;
        if let Some(last) = state.last_call {
            tokio::time::sleep_until(last + Duration::from_millis(500)).await;
        }
        if !state.connections.contains_key(service) {
            let headers =
                HashMap::from([(reqwest::header::AUTHORIZATION, self.authorization.clone())]);
            let config = StreamableHttpClientTransportConfig::with_uri(format!(
                "{}/hexin-ifind-ds-{service}-mcp",
                self.base
            ))
            .custom_headers(headers)
            .reinit_on_expired_session(false);
            let transport = StreamableHttpClientTransport::with_client(self.client.clone(), config);
            let info = ClientInfo::new(
                ClientCapabilities::default(),
                Implementation::new("wealthfolio", env!("CARGO_PKG_VERSION")),
            )
            .with_protocol_version(ProtocolVersion::V_2025_03_26);
            let connection = tokio::time::timeout(TIMEOUT, info.serve(transport))
                .await
                .map_err(|_| MarketDataError::Timeout {
                    provider: self.id().into(),
                })?
                .map_err(|_| {
                    error("MCP initialization failed; check API key, permissions and connection")
                })?;
            state.connections.insert(service, connection);
        }
        state.last_call = Some(Instant::now());
        let params = CallToolRequestParams::new(tool.to_string()).with_arguments(
            arguments
                .as_object()
                .cloned()
                .ok_or_else(|| error("Invalid tool arguments"))?,
        );
        let result =
            tokio::time::timeout(TIMEOUT, state.connections[service].call_tool(params)).await;
        let result = match result {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => {
                state.connections.remove(service);
                return Err(error(
                    "MCP tool request failed; check permissions and connection",
                ));
            }
            Err(_) => {
                state.connections.remove(service);
                return Err(MarketDataError::Timeout {
                    provider: self.id().into(),
                });
            }
        };
        if result.is_error == Some(true) {
            return Err(error("MCP tool returned an error"));
        }
        let value = if let Some(value) = result.structured_content {
            value
        } else {
            if result.content.len() != 1 {
                return Err(error("Unexpected tool content"));
            }
            let text = result.content[0]
                .as_text()
                .ok_or_else(|| error("Expected text tool content"))?;
            serde_json::from_str(&text.text).map_err(|_| error("Invalid tool JSON"))?
        };
        parse::data(value)
    }

    async fn query(
        &self,
        service: &'static str,
        tool: &str,
        query: String,
    ) -> Result<Value, MarketDataError> {
        self.call(service, tool, json!({"query": query})).await
    }

    async fn profile(&self, symbol: &str) -> Result<AssetProfile, MarketDataError> {
        let (kind, code) = identity(symbol)?;
        let (service, tool) = match kind {
            "stock" => ("stock", "get_stock_info"),
            "fund" | "nav" => ("fund", "get_fund_profile"),
            "index" => ("index", "index_data"),
            "global" => ("global-stock", "global_stock_profile"),
            _ => return Err(unsupported()),
        };
        let fields = match kind {
            "stock" => "股票简称、上市交易所、股票种类、交易币种",
            "fund" | "nav" => "基金简称、基金类型、基金上市地点、交易币种",
            "index" => "指数代码、指数简称、交易币种",
            _ => "股票简称、交易所中文名称、证券类型、交易币种",
        };
        let data = self
            .query(service, tool, format!("{code}的{fields}"))
            .await?;
        parse::profile(&data, kind, code)
    }

    async fn history(
        &self,
        symbol: &str,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Vec<Quote>, MarketDataError> {
        let (kind, code) = identity(symbol)?;
        let (service, tool) = match kind {
            "stock" => ("stock", "get_stock_performance"),
            "fund" | "nav" => ("fund", "get_fund_market_performance"),
            "index" => ("index", "index_data"),
            "global" => ("global-stock", "global_stock_quotes"),
            _ => return Err(unsupported()),
        };
        let indicators = if kind == "nav" {
            "每日单位净值、净值日期、单位净值币种"
        } else {
            "每日不复权开盘价、最高价、最低价、收盘价、成交量（股）、交易币种，仅交易日，不填充非交易日"
        };
        let mut quotes = Vec::new();
        let mut day = start.date_naive();
        while day <= end.date_naive() {
            // Small fixed windows avoid upstream answer truncation; never accept a truncated answer.
            // shortcut: no upstream row count; add calendar validation before promising exhaustive coverage.
            let last = day
                .checked_add_days(Days::new(29))
                .ok_or_else(|| error("Invalid history date"))?
                .min(end.date_naive());
            let data = self
                .query(
                    service,
                    tool,
                    format!(
                        "{code}在{}-{}{indicators}",
                        day.format("%Y%m%d"),
                        last.format("%Y%m%d")
                    ),
                )
                .await?;
            quotes.extend(parse::history(&data, kind, code, day, last)?);
            day = last
                .checked_add_days(Days::new(1))
                .ok_or_else(|| error("Invalid history date"))?;
        }
        quotes.sort_by_key(|q| q.timestamp);
        for pair in quotes.windows(2) {
            if pair[0].timestamp == pair[1].timestamp && pair[0].close != pair[1].close {
                return Err(error("Conflicting history rows"));
            }
        }
        quotes.dedup_by_key(|q| q.timestamp);
        if quotes.is_empty() {
            return Err(MarketDataError::NoDataForRange);
        }
        Ok(quotes)
    }
}

fn identity(symbol: &str) -> Result<(&str, &str), MarketDataError> {
    let (kind, code) = symbol.split_once(':').ok_or_else(unsupported)?;
    if !matches!(kind, "stock" | "fund" | "nav" | "index" | "global")
        || code.is_empty()
        || !code
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
    {
        return Err(unsupported());
    }
    let suffix = code.rsplit_once('.').map(|(_, suffix)| suffix);
    let valid = match kind {
        "stock" | "fund" | "index" => matches!(suffix, Some("SH" | "SZ" | "BJ")),
        "nav" => suffix == Some("OF"),
        "global" => matches!(suffix, Some("HK" | "O" | "N" | "A")),
        _ => false,
    };
    if !valid {
        return Err(unsupported());
    }
    Ok((kind, code))
}

/// Resolve only unambiguous local identities. Explicit overrides retain fund/index categories.
pub fn resolve(ticker: &str, mic: Option<&str>) -> Option<String> {
    if let Some(code) = ticker.strip_prefix("FUND:") {
        return (code.len() == 6 && code.bytes().all(|b| b.is_ascii_digit()))
            .then(|| format!("nav:{code}.OF"));
    }
    let mic = mic?;
    let (kind, suffix) = match mic {
        "XSHG" if ticker.starts_with('6') => ("stock", "SH"),
        "XSHE" if ticker.starts_with(['0', '3']) => ("stock", "SZ"),
        "XBSE" => ("stock", "BJ"),
        "XSHG" if ticker.starts_with('5') => ("fund", "SH"),
        "XSHE" if ticker.starts_with('1') => ("fund", "SZ"),
        "XHKG" => ("global", "HK"),
        "XNAS" => ("global", "O"),
        "XNYS" => ("global", "N"),
        "XASE" => ("global", "A"),
        _ => return None,
    };
    let code = if mic == "XHKG" {
        format!("{ticker:0>5}")
    } else {
        ticker.into()
    };
    let symbol = format!("{kind}:{code}.{suffix}");
    if identity(&symbol).is_ok() {
        Some(symbol)
    } else {
        None
    }
}

fn symbol(instrument: ProviderInstrument) -> Result<Arc<str>, MarketDataError> {
    match instrument {
        ProviderInstrument::EquitySymbol { symbol } => Ok(symbol),
        _ => Err(unsupported()),
    }
}

#[async_trait]
impl MarketDataProvider for IfindProvider {
    fn id(&self) -> &'static str {
        DATA_SOURCE_IFIND
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
            requests_per_minute: 120,
            max_concurrency: 1,
            min_delay: Duration::from_millis(500),
        }
    }
    async fn get_profile(&self, symbol: &str) -> Result<AssetProfile, MarketDataError> {
        self.profile(symbol).await
    }
    async fn get_latest_quote(
        &self,
        _context: &QuoteContext,
        instrument: ProviderInstrument,
    ) -> Result<Quote, MarketDataError> {
        let symbol = symbol(instrument)?;
        let (kind, code) = identity(&symbol)?;
        if matches!(kind, "nav" | "global") {
            let (service, tool, fields) = if kind == "nav" {
                (
                    "fund",
                    "get_fund_market_performance",
                    "最新已公布单位净值、净值日期、单位净值币种",
                )
            } else {
                (
                    "global-stock",
                    "global_stock_quotes",
                    "最新不复权收盘价、最近交易日期、交易币种",
                )
            };
            let data = self
                .query(service, tool, format!("{code}的{fields}"))
                .await?;
            return parse::latest_daily(&data, kind, code);
        }
        let profile = self.profile(&symbol).await?;
        let (service, tool) = match kind {
            "stock" => ("stock", "stock_highfreq_quotes"),
            "fund" => ("fund", "fund_highfreq_quotes"),
            "index" => ("index", "index_highfreq_quotes"),
            _ => return Err(unsupported()),
        };
        // shortcut: real-time volume units are unspecified; add volume once official units are verified.
        let data = self.call(service, tool, json!({"symbols":code,"indicators":"最新价,开盘价,最高价,最低价","data_mode":"real_time"})).await?;
        parse::latest(
            &data,
            code,
            profile
                .currency
                .as_deref()
                .ok_or_else(|| error("Missing quote currency"))?,
        )
    }
    async fn get_historical_quotes(
        &self,
        _context: &QuoteContext,
        instrument: ProviderInstrument,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Vec<Quote>, MarketDataError> {
        self.history(&symbol(instrument)?, start, end).await
    }
    async fn search(&self, query: &str) -> Result<Vec<SearchResult>, MarketDataError> {
        let normalized = query.trim().to_uppercase();
        let qualified = if identity(query).is_ok() {
            None
        } else {
            crate::resolver::qualified_symbol(&normalized)
        };
        let resolved_query = if let Some((market, code)) = qualified {
            let resolved = if market == "FUND" {
                resolve(&normalized, None)
            } else {
                resolve(code, Some(market))
            };
            let Some(symbol) = resolved else {
                return Ok(Vec::new());
            };
            Some(symbol)
        } else {
            None
        };
        let query = resolved_query.as_deref().unwrap_or(query);
        if let Ok((kind, code)) = identity(query) {
            let profile = self.profile(query).await?;
            let mut result = SearchResult::new(
                query,
                profile.name.ok_or_else(|| error("Missing security name"))?,
                profile.exchange.unwrap_or_default(),
                profile
                    .quote_type
                    .ok_or_else(|| error("Missing security type"))?,
            )
            .with_data_source(self.id());
            result.currency = profile.currency;
            result.exchange_mic = match result.exchange.as_str() {
                "" => None,
                value => Some(value.into()),
            };
            result.canonical_symbol = Some(parse::canonical(code, kind)?);
            return Ok(vec![result]);
        }
        let mut results = Vec::new();
        // Return successful services even when this key lacks access to another market.
        let mut failure = None;
        for (kind, service, tool, market) in [
            ("stock", "stock", "search_stocks", None),
            ("fund", "fund", "search_funds", None),
            ("index", "index", "index_data", None),
            (
                "global",
                "global-stock",
                "search_global_stocks",
                Some("港股"),
            ),
            (
                "global",
                "global-stock",
                "search_global_stocks",
                Some("美股"),
            ),
        ] {
            let mut args = json!({"query":format!("{query}的证券代码、证券简称、上市交易所、交易币种，最多5只")});
            if let Some(market) = market {
                args["market"] = json!(market);
            }
            match self
                .call(service, tool, args)
                .await
                .and_then(|data| parse::search(&data, kind))
            {
                Ok(rows) => results.extend(rows),
                Err(e) => failure = Some(e),
            }
        }
        let normalized = query.to_lowercase();
        let matches = |result: &SearchResult| {
            result.name.to_lowercase().contains(&normalized)
                || result.symbol.to_lowercase().contains(&normalized)
        };
        if results.iter().any(matches) {
            results.retain(matches);
        }
        results.sort_by(|a, b| a.symbol.cmp(&b.symbol));
        results.dedup_by(|a, b| a.symbol == b.symbol);
        let mut verified = Vec::new();
        for mut result in results.into_iter().take(5) {
            match self.profile(&result.symbol).await {
                Ok(profile) => {
                    result.currency = profile.currency;
                    if let Some(name) = profile.name {
                        result.name = name;
                    }
                    if let Some(kind) = profile.quote_type {
                        result.asset_type = kind;
                    }
                    verified.push(result);
                }
                Err(error) => failure = Some(error),
            }
        }
        let mut results = verified;
        results.sort_by(|a, b| a.symbol.cmp(&b.symbol));
        results.dedup_by(|a, b| a.symbol == b.symbol);
        if results.is_empty() {
            if let Some(error) = failure {
                return Err(error);
            }
        }
        Ok(results)
    }
}
