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
struct CreateAssetArgs {
    pub input: wealthfolio_core::assets::NewAsset,
}

business_tool!(CreateAsset, CreateAssetArgs, "create_asset",
    "Register an investment security or a manually priced custom investment. Existing asset events handle enrichment. Arbitrary provider configuration and alternative assets are not accepted.",
    json!({"type":"object","properties":{"input":{"type":"object","properties":{"kind":{"type":"string","enum":["INVESTMENT"]},"name":{"type":"string"},"displayCode":{"type":"string"},"isActive":{"type":"boolean"},"quoteMode":{"type":"string","enum":["MARKET","MANUAL"]},"quoteCcy":{"type":"string"},"instrumentType":{"type":"string"},"instrumentSymbol":{"type":"string"},"instrumentExchangeMic":{"type":"string"},"providerId":{"type":"string"},"providerSymbol":{"type":"string"},"notes":{"type":"string"},"metadata":{"type":"object"}},"required":["kind","quoteMode","quoteCcy"]}},"required":["input"]}), [AgentScope::AssetsWrite], Write, |env, args| {
        if args.input.provider_config.is_some() || args.input.kind != wealthfolio_core::assets::AssetKind::Investment {
            return Err(AgentToolError::InvalidInput("Only investment assets without providerConfig can be registered here".into()));
        }
        output(env.asset_service().create_asset(args.input).await?)
    });

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct UpdateAssetProfileArgs {
    pub asset_id: String,
    pub input: wealthfolio_core::assets::UpdateAssetProfile,
}

business_tool!(UpdateAssetProfile, UpdateAssetProfileArgs, "update_asset_profile",
    "Update an investment asset profile. Use update_quote_mode for quote mode changes. Provider configuration and asset kind changes are excluded.",
    json!({"type":"object","properties":{"assetId":{"type":"string"},"input":{"type":"object","properties":{"name":{"type":"string"},"displayCode":{"type":"string"},"quoteCcy":{"type":"string"},"instrumentType":{"type":"string"},"instrumentSymbol":{"type":"string"},"instrumentExchangeMic":{"type":"string"},"notes":{"type":"string"},"metadata":{"type":"object"}},"required":["notes"]}},"required":["assetId","input"]}), [AgentScope::AssetsWrite], Write, |env, args| {
        if args.input.provider_config.is_some() || args.input.kind.is_some() || args.input.quote_mode.is_some() {
            return Err(AgentToolError::InvalidInput("providerConfig, kind and quoteMode are not accepted in profile updates".into()));
        }
        if env.asset_service().get_asset_profile(&args.asset_id)?.asset.kind != wealthfolio_core::assets::AssetKind::Investment {
            return Err(AgentToolError::InvalidInput("Alternative asset writes are excluded".into()));
        }
        output(env.asset_service().update_asset_profile(&args.asset_id, args.input).await?)
    });

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GetAssetsArgs {}

business_tool!(
    GetAssets,
    GetAssetsArgs,
    "get_assets",
    "List complete local asset records. Does not refresh providers.",
    json!({"type":"object","properties":{},"required":[]}),
    [AgentScope::HoldingsRead],
    Read,
    |env, args| {
        let _ = args;
        output(env.asset_service().get_assets()?)
    }
);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GetAssetProfileArgs {
    pub asset_id: String,
}

business_tool!(
    GetAssetProfile,
    GetAssetProfileArgs,
    "get_asset_profile",
    "Get the complete locally stored asset profile.",
    json!({"type":"object","properties":{"assetId":{"type":"string"}},"required":["assetId"]}),
    [AgentScope::HoldingsRead],
    Read,
    |env, args| { output(env.asset_service().get_asset_profile(&args.asset_id)?) }
);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct UpdateQuoteModeArgs {
    pub asset_id: String,
    pub quote_mode: String,
}

business_tool!(
    UpdateQuoteMode,
    UpdateQuoteModeArgs,
    "update_quote_mode",
    "Change an asset quote mode to MARKET or MANUAL through the existing asset service and events.",
    json!({"type":"object","properties":{"assetId":{"type":"string"},"quoteMode":{"type":"string","enum":["MARKET","MANUAL"]}},"required":["assetId","quoteMode"]}),
    [AgentScope::MarketDataWrite],
    Write,
    |env, args| {
        if env
            .asset_service()
            .get_asset_profile(&args.asset_id)?
            .asset
            .kind
            != wealthfolio_core::assets::AssetKind::Investment
        {
            return Err(AgentToolError::InvalidInput(
                "Alternative asset writes are excluded".into(),
            ));
        }
        output(
            env.asset_service()
                .update_quote_mode(&args.asset_id, &args.quote_mode)
                .await?,
        )
    }
);

pub fn tools() -> Vec<Arc<dyn AgentTool>> {
    vec![
        Arc::new(CreateAsset),
        Arc::new(UpdateAssetProfile),
        Arc::new(GetAssets),
        Arc::new(GetAssetProfile),
        Arc::new(UpdateQuoteMode),
    ]
}
