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
    "Register an investment security or a manually priced custom investment, including its asset-owned providerConfig (preferred provider and symbol overrides). Existing asset events handle enrichment. Alternative assets are not accepted.",
    json!({"type":"object","properties":{"input":{"type":"object","properties":{"kind":{"type":"string","enum":["INVESTMENT"]},"name":{"type":"string"},"displayCode":{"type":"string"},"isActive":{"type":"boolean"},"quoteMode":{"type":"string","enum":["MARKET","MANUAL"]},"quoteCcy":{"type":"string"},"instrumentType":{"type":"string"},"instrumentSymbol":{"type":"string"},"instrumentExchangeMic":{"type":"string"},"providerId":{"type":"string"},"providerSymbol":{"type":"string"},"notes":{"type":"string"},"metadata":{"type":"object"},"providerConfig":{"type":["object","null"],"description":"Asset-owned provider routing configuration, including preferred_provider and overrides. Supply the complete configuration, as returned by get_asset_profile."}},"required":["kind","quoteMode","quoteCcy"]}},"required":["input"]}), [AgentScope::AssetsWrite], Write, |env, args| {
        if args.input.kind != wealthfolio_core::assets::AssetKind::Investment {
            return Err(AgentToolError::InvalidInput("Only investment assets can be registered here".into()));
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
    "Update an investment asset profile, including its asset-owned providerConfig. This is not a partial patch: before every update, call get_asset_profile and pass the existing name and complete providerConfig unchanged unless intentionally editing them. Omitting name or providerConfig (or passing null) clears the stored value and can lose the configured market data source. Also pass the current notes and displayCode to retain them. Quote mode is preserved; use update_quote_mode to change it. Asset kind changes are excluded.",
    json!({"type":"object","properties":{"assetId":{"type":"string"},"input":{"type":"object","properties":{"name":{"type":["string","null"],"description":"On every update, copy the existing name unchanged unless renaming. Omitting it or passing null clears the name."},"displayCode":{"type":"string"},"quoteCcy":{"type":"string"},"instrumentType":{"type":"string"},"instrumentSymbol":{"type":"string"},"instrumentExchangeMic":{"type":"string"},"notes":{"type":"string"},"metadata":{"type":"object"},"providerConfig":{"type":["object","null"],"description":"Asset-owned provider routing configuration, including preferred_provider and overrides. Supply the complete configuration, as returned by get_asset_profile. On every update, copy the existing value unchanged unless changing routing. Omitting it or passing null clears the configured market data source."}},"required":["notes"]}},"required":["assetId","input"]}), [AgentScope::AssetsWrite], Write, |env, args| {
        if args.input.kind.is_some() || args.input.quote_mode.is_some() {
            return Err(AgentToolError::InvalidInput("kind and quoteMode are not accepted in profile updates".into()));
        }
        let existing = env.asset_service().get_asset_profile(&args.asset_id)?.asset;
        if existing.kind != wealthfolio_core::assets::AssetKind::Investment {
            return Err(AgentToolError::InvalidInput("Alternative asset writes are excluded".into()));
        }
        let mut input = args.input;
        input.quote_mode = Some(existing.quote_mode);
        output(env.asset_service().update_asset_profile(&args.asset_id, input).await?)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asset_tools_advertise_provider_configuration_and_replacement_semantics() {
        for tool in [
            Box::new(CreateAsset) as Box<dyn AgentTool>,
            Box::new(UpdateAssetProfile),
        ] {
            let schema = tool.input_schema();
            assert_eq!(
                schema["properties"]["input"]["properties"]["providerConfig"]["type"],
                json!(["object", "null"])
            );
            assert_eq!(tool.required_scopes(), &[AgentScope::AssetsWrite]);
        }
        let schema = UpdateAssetProfile.input_schema();
        for field in ["name", "providerConfig"] {
            assert!(
                schema["properties"]["input"]["properties"][field]["description"]
                    .as_str()
                    .unwrap()
                    .contains("every update")
            );
        }
    }
}
