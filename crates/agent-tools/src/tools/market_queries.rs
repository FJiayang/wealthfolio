//! MCP-only business tools. Domain services own validation and persistence.
use super::business_support::*;
use crate::{
    AgentEnvironment, AgentScope, AgentTool, AgentToolAccess, AgentToolError, AgentToolResult,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ListAssetLogosArgs {}

business_tool!(
    ListAssetLogos,
    ListAssetLogosArgs,
    "list_asset_logos",
    "Read locally stored asset logo data.",
    json!({"type":"object","properties":{},"required":[]}),
    [AgentScope::HoldingsRead],
    Read,
    |env, args| {
        let _ = args;
        output(env.asset_logo_service().list_asset_logos()?)
    }
);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GetAssetLogoArgs {
    pub asset_id: String,
}

business_tool!(
    GetAssetLogo,
    GetAssetLogoArgs,
    "get_asset_logo",
    "Read locally stored asset logo data.",
    json!({"type":"object","properties":{"assetId":{"type":"string"}},"required":["assetId"]}),
    [AgentScope::HoldingsRead],
    Read,
    |env, args| { output(env.asset_logo_service().get_asset_logo(&args.asset_id)?) }
);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct UpsertAssetLogoArgs {
    pub asset_id: String,
    pub input: wealthfolio_core::assets::UpsertAssetLogo,
}

business_tool!(
    UpsertAssetLogo,
    UpsertAssetLogoArgs,
    "upsert_asset_logo",
    "Save base64 PNG logo content using existing size and image validation; no file paths or URLs.",
    json!({"type":"object","properties":{"assetId":{"type":"string"},"input":{"type":"object","properties":{"dataBase64":{"type":"string"}},"required":["dataBase64"]}},"required":["assetId","input"]}),
    [AgentScope::AssetsWrite],
    Write,
    |env, args| {
        output(
            env.asset_logo_service()
                .upsert_asset_logo(&args.asset_id, args.input)
                .await?,
        )
    }
);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SearchSymbolArgs {
    pub query: String,
}

business_tool!(SearchSymbol, SearchSymbolArgs, "search_symbol",
    "Search configured enabled market data providers for a symbol. This may make provider requests but does not refresh holdings history.",
    json!({"type":"object","properties":{"query":{"type":"string"}},"required":["query"]}), [AgentScope::HoldingsRead], Read, |env, args| {
        output(env.quote_service().search_symbol(&args.query).await?)
    });

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ResolveSymbolQuoteArgs {
    pub symbol: String,
    #[serde(default)]
    pub exchange_mic: Option<String>,
    #[serde(default)]
    pub instrument_type: Option<wealthfolio_core::assets::InstrumentType>,
    #[serde(default)]
    pub quote_ccy: Option<String>,
    #[serde(default)]
    pub provider_id: Option<String>,
    #[serde(default)]
    pub provider_symbol: Option<String>,
}

business_tool!(ResolveSymbolQuote, ResolveSymbolQuoteArgs, "resolve_symbol_quote",
    "Resolve a selected symbol through configured enabled providers. Does not refresh stored history or change provider settings.",
    json!({"type":"object","properties":{"symbol":{"type":"string"},"exchangeMic":{"type":"string"},"instrumentType":{"type":"string"},"quoteCcy":{"type":"string"},"providerId":{"type":"string"},"providerSymbol":{"type":"string"}},"required":["symbol"]}), [AgentScope::HoldingsRead], Read, |env, args| {
        output(env.quote_service().resolve_symbol_quote(&args.symbol, args.exchange_mic.as_deref(), args.instrument_type.as_ref(), args.quote_ccy.as_deref(), args.provider_id.as_deref(), args.provider_symbol.as_deref()).await?)
    });

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GetQuoteHistoryArgs {
    pub asset_id: String,
}

business_tool!(
    GetQuoteHistory,
    GetQuoteHistoryArgs,
    "get_quote_history",
    "Read locally stored quote history for an asset ID; no provider refresh.",
    json!({"type":"object","properties":{"assetId":{"type":"string"}},"required":["assetId"]}),
    [AgentScope::MarketDataRead],
    Read,
    |env, args| { output(env.quote_service().get_historical_quotes(&args.asset_id)?) }
);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GetLatestQuotesArgs {
    pub asset_ids: Vec<String>,
}

business_tool!(
    GetLatestQuotes,
    GetLatestQuotesArgs,
    "get_latest_quotes",
    "Read locally stored latest quote snapshots for asset IDs; no provider refresh.",
    json!({"type":"object","properties":{"assetIds":{"type":"array","items":{"type":"string"},"maxItems":100}},"required":["assetIds"]}),
    [AgentScope::MarketDataRead],
    Read,
    |env, args| {
        if args.asset_ids.len() > 100 {
            return Err(AgentToolError::InvalidInput(
                "At most 100 items per call".into(),
            ));
        }
        output(
            env.quote_service()
                .get_latest_quotes_snapshot(&args.asset_ids)?,
        )
    }
);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GetExchangesArgs {}

business_tool!(
    GetExchanges,
    GetExchangesArgs,
    "get_exchanges",
    "List the built-in exchange directory without network requests.",
    json!({"type":"object","properties":{},"required":[]}),
    [AgentScope::MarketDataRead],
    Read,
    |env, args| {
        let _ = (args, env);
        output(wealthfolio_market_data::get_exchange_list())
    }
);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct FetchDividendsArgs {
    pub symbol: String,
    #[serde(default)]
    pub exchange_mic: Option<String>,
    #[serde(default)]
    pub instrument_type: Option<wealthfolio_core::assets::InstrumentType>,
    #[serde(default)]
    pub quote_ccy: Option<String>,
    #[serde(default)]
    pub provider_id: Option<String>,
    #[serde(default)]
    pub start_date: Option<chrono::NaiveDate>,
    #[serde(default)]
    pub end_date: Option<chrono::NaiveDate>,
}

business_tool!(FetchDividends, FetchDividendsArgs, "fetch_dividends",
    "Query dividend events through configured enabled providers. Does not record activities or refresh price history.",
    json!({"type":"object","properties":{"symbol":{"type":"string"},"exchangeMic":{"type":"string"},"instrumentType":{"type":"string"},"quoteCcy":{"type":"string"},"providerId":{"type":"string"},"startDate":{"type":"string"},"endDate":{"type":"string"}},"required":["symbol"]}), [AgentScope::MarketDataRead], Read, |env, args| {
        output(env.quote_service().fetch_dividends(wealthfolio_core::quotes::FetchDividendsParams { symbol: args.symbol, exchange_mic: args.exchange_mic, instrument_type: args.instrument_type, quote_ccy: args.quote_ccy, preferred_provider: args.provider_id, start: args.start_date, end: args.end_date }).await?)
    });

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GetExchangeRatesArgs {}

business_tool!(
    GetExchangeRates,
    GetExchangeRatesArgs,
    "get_exchange_rates",
    "Read locally stored latest exchange rates.",
    json!({"type":"object","properties":{},"required":[]}),
    [AgentScope::MarketDataRead],
    Read,
    |env, args| {
        let _ = args;
        output(env.fx_service().get_latest_exchange_rates()?)
    }
);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GetExchangeRatesForDatesArgs {
    pub pairs: Vec<wealthfolio_core::fx::ExchangeRateDateQuery>,
}

business_tool!(
    GetExchangeRatesForDates,
    GetExchangeRatesForDatesArgs,
    "get_exchange_rates_for_dates",
    "Resolve historical rates from local storage; each pair returns a rate or error.",
    json!({"type":"object","properties":{"pairs":{"type":"array","items":{"type":"object","properties":{"fromCurrency":{"type":"string"},"toCurrency":{"type":"string"},"date":{"type":"string"}},"required":["fromCurrency","toCurrency","date"]},"maxItems":100}},"required":["pairs"]}),
    [AgentScope::MarketDataRead],
    Read,
    |env, args| {
        if args.pairs.len() > 100 {
            return Err(AgentToolError::InvalidInput(
                "At most 100 items per call".into(),
            ));
        }
        output(env.fx_service().get_exchange_rates_for_dates(args.pairs))
    }
);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AddExchangeRateArgs {
    pub input: wealthfolio_core::fx::NewExchangeRate,
}

business_tool!(
    AddExchangeRate,
    AddExchangeRateArgs,
    "add_exchange_rate",
    "Add a manual exchange rate through the existing FX service.",
    json!({"type":"object","properties":{"input":{"type":"object","properties":{"fromCurrency":{"type":"string"},"toCurrency":{"type":"string"},"rate":{"oneOf":[{"type":"string"},{"type":"number"}]},"source":{"type":"string","enum":["MANUAL"]}},"required":["fromCurrency","toCurrency","rate","source"]}},"required":["input"]}),
    [AgentScope::MarketDataWrite],
    Write,
    |env, args| {
        if args.input.source != wealthfolio_core::quotes::constants::DATA_SOURCE_MANUAL {
            return Err(AgentToolError::InvalidInput(
                "Only manual exchange rates can be added".into(),
            ));
        }
        output(env.fx_service().add_exchange_rate(args.input).await?)
    }
);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct UpdateExchangeRateArgs {
    pub from_currency: String,
    pub to_currency: String,
    pub rate: rust_decimal::Decimal,
}

business_tool!(
    UpdateExchangeRate,
    UpdateExchangeRateArgs,
    "update_exchange_rate",
    "Update a manual currency pair rate through the existing FX service.",
    json!({"type":"object","properties":{"fromCurrency":{"type":"string"},"toCurrency":{"type":"string"},"rate":{"oneOf":[{"type":"string"},{"type":"number"}]}},"required":["fromCurrency","toCurrency","rate"]}),
    [AgentScope::MarketDataWrite],
    Write,
    |env, args| {
        output(
            env.fx_service()
                .update_exchange_rate(&args.from_currency, &args.to_currency, args.rate)
                .await?,
        )
    }
);

pub fn tools() -> Vec<Arc<dyn AgentTool>> {
    vec![
        Arc::new(ListAssetLogos),
        Arc::new(GetAssetLogo),
        Arc::new(UpsertAssetLogo),
        Arc::new(SearchSymbol),
        Arc::new(ResolveSymbolQuote),
        Arc::new(GetQuoteHistory),
        Arc::new(GetLatestQuotes),
        Arc::new(GetExchanges),
        Arc::new(FetchDividends),
        Arc::new(GetExchangeRates),
        Arc::new(GetExchangeRatesForDates),
        Arc::new(AddExchangeRate),
        Arc::new(UpdateExchangeRate),
    ]
}
