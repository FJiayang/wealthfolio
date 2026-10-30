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
struct CreateAllocationTargetArgs {
    pub input: wealthfolio_core::portfolio::allocation_targets::NewAllocationTarget,
}

business_tool!(CreateAllocationTarget, CreateAllocationTargetArgs, "create_allocation_target",
    "Save allocation configuration through existing validation. Weight lists replace the complete existing list, in basis points (10000 = 100%). Does not execute trades.",
    json!({"type":"object","properties":{"input":{"type":"object","properties":{"name":{"type":"string"},"scopeType":{"type":"string","enum":["all","account","portfolio"]},"scopeId":{"type":"string"},"taxonomyId":{"type":"string"},"triggerType":{"type":"string"},"driftBandBps":{"type":"integer"},"bandType":{"type":"string","enum":["absolute","hybrid"]},"relativeFactorBps":{"type":"integer"},"rebalanceGoal":{"type":"string","enum":["nearest_band","exact_target"]},"minTradeAmount":{"type":"string"},"wholeSharesOnly":{"type":"boolean"},"allowSells":{"type":"boolean"},"maxTurnoverBps":{"type":"integer"}},"required":["name","scopeType","taxonomyId","triggerType","driftBandBps"]}},"required":["input"]}), [AgentScope::FinancialPlanningWrite], Write, |env, args| {
        output(env.allocation_target_service().create_target(args.input).await?)
    });

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct UpdateAllocationTargetArgs {
    pub id: String,
    pub input: wealthfolio_core::portfolio::allocation_targets::NewAllocationTarget,
}

business_tool!(UpdateAllocationTarget, UpdateAllocationTargetArgs, "update_allocation_target",
    "Save allocation configuration through existing validation. Weight lists replace the complete existing list, in basis points (10000 = 100%). Does not execute trades.",
    json!({"type":"object","properties":{"id":{"type":"string"},"input":{"type":"object","properties":{"name":{"type":"string"},"scopeType":{"type":"string","enum":["all","account","portfolio"]},"scopeId":{"type":"string"},"taxonomyId":{"type":"string"},"triggerType":{"type":"string"},"driftBandBps":{"type":"integer"},"bandType":{"type":"string","enum":["absolute","hybrid"]},"relativeFactorBps":{"type":"integer"},"rebalanceGoal":{"type":"string","enum":["nearest_band","exact_target"]},"minTradeAmount":{"type":"string"},"wholeSharesOnly":{"type":"boolean"},"allowSells":{"type":"boolean"},"maxTurnoverBps":{"type":"integer"}},"required":["name","scopeType","taxonomyId","triggerType","driftBandBps"]}},"required":["id","input"]}), [AgentScope::FinancialPlanningWrite], Write, |env, args| {
        output(env.allocation_target_service().update_target(&args.id, args.input).await?)
    });

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ArchiveAllocationTargetArgs {
    pub id: String,
}

business_tool!(ArchiveAllocationTarget, ArchiveAllocationTargetArgs, "archive_allocation_target",
    "Save allocation configuration through existing validation. Weight lists replace the complete existing list, in basis points (10000 = 100%). Does not execute trades.",
    json!({"type":"object","properties":{"id":{"type":"string"}},"required":["id"]}), [AgentScope::FinancialPlanningWrite], Write, |env, args| {
        output(env.allocation_target_service().archive_target(&args.id).await?)
    });

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SaveAllocationTargetWeightsArgs {
    pub target_id: String,
    pub weights: Vec<wealthfolio_core::portfolio::allocation_targets::NewAllocationTargetWeight>,
}

business_tool!(SaveAllocationTargetWeights, SaveAllocationTargetWeightsArgs, "save_allocation_target_weights",
    "Save allocation configuration through existing validation. Weight lists replace the complete existing list, in basis points (10000 = 100%). Does not execute trades.",
    json!({"type":"object","properties":{"targetId":{"type":"string"},"weights":{"type":"array","items":{"type":"object","properties":{"categoryId":{"type":"string"},"targetBps":{"type":"integer"},"isLocked":{"type":"boolean"},"isRequired":{"type":"boolean"}},"required":["categoryId","targetBps","isLocked","isRequired"]},"maxItems":100}},"required":["targetId","weights"]}), [AgentScope::FinancialPlanningWrite], Write, |env, args| {
        if args.weights.len() > 100 { return Err(AgentToolError::InvalidInput("At most 100 items per call".into())); }
        output(env.allocation_target_service().save_weights(&args.target_id, args.weights).await?)
    });

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SaveAllocationTargetWithWeightsArgs {
    #[serde(default)]
    pub id: Option<String>,
    pub input: wealthfolio_core::portfolio::allocation_targets::NewAllocationTarget,
    pub weights: Vec<wealthfolio_core::portfolio::allocation_targets::NewAllocationTargetWeight>,
}

business_tool!(SaveAllocationTargetWithWeights, SaveAllocationTargetWithWeightsArgs, "save_allocation_target_with_weights",
    "Save allocation configuration through existing validation. Weight lists replace the complete existing list, in basis points (10000 = 100%). Does not execute trades.",
    json!({"type":"object","properties":{"id":{"type":"string"},"input":{"type":"object","properties":{"name":{"type":"string"},"scopeType":{"type":"string","enum":["all","account","portfolio"]},"scopeId":{"type":"string"},"taxonomyId":{"type":"string"},"triggerType":{"type":"string"},"driftBandBps":{"type":"integer"},"bandType":{"type":"string","enum":["absolute","hybrid"]},"relativeFactorBps":{"type":"integer"},"rebalanceGoal":{"type":"string","enum":["nearest_band","exact_target"]},"minTradeAmount":{"type":"string"},"wholeSharesOnly":{"type":"boolean"},"allowSells":{"type":"boolean"},"maxTurnoverBps":{"type":"integer"}},"required":["name","scopeType","taxonomyId","triggerType","driftBandBps"]},"weights":{"type":"array","items":{"type":"object","properties":{"categoryId":{"type":"string"},"targetBps":{"type":"integer"},"isLocked":{"type":"boolean"},"isRequired":{"type":"boolean"}},"required":["categoryId","targetBps","isLocked","isRequired"]},"maxItems":100}},"required":["input","weights"]}), [AgentScope::FinancialPlanningWrite], Write, |env, args| {
        if args.weights.len() > 100 { return Err(AgentToolError::InvalidInput("At most 100 items per call".into())); }
        output(env.allocation_target_service().save_target_with_weights(args.id, args.input, args.weights).await?)
    });

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ListAllocationTargetsArgs {}

business_tool!(
    ListAllocationTargets,
    ListAllocationTargetsArgs,
    "list_allocation_targets",
    "Read saved allocation target configuration.",
    json!({"type":"object","properties":{},"required":[]}),
    [AgentScope::FinancialPlanningRead],
    Read,
    |env, args| {
        let _ = args;
        output(env.allocation_target_service().list_targets()?)
    }
);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GetAllocationTargetArgs {
    pub id: String,
}

business_tool!(
    GetAllocationTarget,
    GetAllocationTargetArgs,
    "get_allocation_target",
    "Read saved allocation target configuration.",
    json!({"type":"object","properties":{"id":{"type":"string"}},"required":["id"]}),
    [AgentScope::FinancialPlanningRead],
    Read,
    |env, args| { output(env.allocation_target_service().get_target(&args.id)?) }
);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ListAllocationTargetWeightsArgs {
    pub target_id: String,
}

business_tool!(
    ListAllocationTargetWeights,
    ListAllocationTargetWeightsArgs,
    "list_allocation_target_weights",
    "Read saved allocation target configuration.",
    json!({"type":"object","properties":{"targetId":{"type":"string"}},"required":["targetId"]}),
    [AgentScope::FinancialPlanningRead],
    Read,
    |env, args| {
        output(
            env.allocation_target_service()
                .list_weights_for_target(&args.target_id)?,
        )
    }
);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ListTargetConstraintsArgs {
    pub target_id: String,
}

business_tool!(
    ListTargetConstraints,
    ListTargetConstraintsArgs,
    "list_target_constraints",
    "Read saved allocation target configuration.",
    json!({"type":"object","properties":{"targetId":{"type":"string"}},"required":["targetId"]}),
    [AgentScope::FinancialPlanningRead],
    Read,
    |env, args| {
        output(
            env.allocation_target_service()
                .list_target_constraints(&args.target_id)?,
        )
    }
);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SaveTargetConstraintsArgs {
    pub target_id: String,
    pub constraints:
        Vec<wealthfolio_core::portfolio::allocation_targets::AllocationTargetConstraint>,
}

business_tool!(SaveTargetConstraints, SaveTargetConstraintsArgs, "save_target_constraints",
    "Replace the complete target sell-constraint list. Existing domain rules validate each constraint. Does not execute trades.",
    json!({"type":"object","properties":{"targetId":{"type":"string"},"constraints":{"type":"array","items":{"type":"object","properties":{"id":{"type":"string"},"targetId":{"type":"string"},"subjectType":{"type":"string","enum":["asset","account"]},"subjectId":{"type":"string"},"effect":{"type":"string"},"action":{"type":"string"},"createdAt":{"type":"string"},"updatedAt":{"type":"string"}},"required":["id","targetId","subjectType","subjectId","effect","action","createdAt","updatedAt"]},"maxItems":100}},"required":["targetId","constraints"]}), [AgentScope::FinancialPlanningWrite], Write, |env, args| {
        if args.constraints.len() > 100 { return Err(AgentToolError::InvalidInput("At most 100 items per call".into())); }
        output(env.allocation_target_service().save_target_constraints(&args.target_id, args.constraints).await?)
    });

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GetAllocationTargetDriftArgs {
    pub target_id: String,
    #[serde(default)]
    pub include_holdings: Option<bool>,
}

business_tool!(GetAllocationTargetDrift, GetAllocationTargetDriftArgs, "get_allocation_target_drift",
    "Calculate drift using the target saved scope (all, account or portfolio). Optionally include holding-level exposure. Does not change positions.",
    json!({"type":"object","properties":{"targetId":{"type":"string"},"includeHoldings":{"type":"boolean"}},"required":["targetId"]}), [AgentScope::FinancialPlanningRead, AgentScope::HoldingsRead], Read, |env, args| {
        let resolved = target_scope(&env, &args.target_id)?;
        let service = env.drift_service();
        let report = if args.include_holdings.unwrap_or(false) {
            service.get_drift_report_with_holdings_for_target(&args.target_id, &resolved.account_ids, &env.base_currency(), &resolved.scope_id).await?
        } else {
            service.get_drift_report_for_target(&args.target_id, &resolved.account_ids, &env.base_currency(), &resolved.scope_id).await?
        };
        output(report)
    });

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GenerateCalculatedAdjustmentsArgs {
    target_id: String,
    filter: wealthfolio_core::portfolios::AccountScope,
    selected_account_ids: Vec<String>,
    mode: wealthfolio_core::portfolio::allocation_targets::WorksheetMode,
    rule: wealthfolio_core::portfolio::allocation_targets::AllocationRule,
    cash: wealthfolio_core::portfolio::allocation_targets::WorksheetCashInput,
    #[serde(default)]
    eligible_asset_ids: Option<Vec<String>>,
    #[serde(default)]
    sellable_asset_ids: Option<Vec<String>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CalculateAllocationWorksheetArgs {
    target_id: String,
    filter: wealthfolio_core::portfolios::AccountScope,
    selected_account_ids: Vec<String>,
    cash: wealthfolio_core::portfolio::allocation_targets::WorksheetCashInput,
    lines: Vec<wealthfolio_core::portfolio::allocation_targets::AllocationWorksheetLineInput>,
}

fn worksheet_schema(generate: bool) -> Value {
    let decimal = json!({"oneOf":[{"type":"string"},{"type":"number"}]});
    let ids = json!({"type":"array","items":{"type":"string"},"maxItems":100});
    let mut schema = json!({
        "type":"object", "additionalProperties":false,
        "properties":{
            "targetId":{"type":"string"},
            "filter":{"type":"object","properties":{"type":{"type":"string","enum":["all","account","portfolio","accounts"]},"accountId":{"type":"string"},"portfolioId":{"type":"string"},"accountIds":ids},"required":["type"]},
            "selectedAccountIds":ids,
            "cash":{"type":"object","properties":{
                "trackedCashToUse":decimal,
                "externalContribution":{"type":"object","additionalProperties":decimal},
                "trackedCashByAccount":{"type":["object","null"],"additionalProperties":decimal}
            },"required":["trackedCashToUse"]}
        },
        "required":["targetId","filter","selectedAccountIds","cash"]
    });
    if generate {
        schema["properties"]["mode"] = json!({"type":"string","enum":["invest_cash","rebalance"]});
        schema["properties"]["rule"] =
            json!({"type":"string","enum":["current_holding_proportions"]});
        schema["properties"]["eligibleAssetIds"] =
            json!({"type":["array","null"],"items":{"type":"string"},"maxItems":100});
        schema["properties"]["sellableAssetIds"] = schema["properties"]["eligibleAssetIds"].clone();
        schema["required"] = json!([
            "targetId",
            "filter",
            "selectedAccountIds",
            "cash",
            "mode",
            "rule"
        ]);
    } else {
        schema["properties"]["lines"] = json!({"type":"array","maxItems":100,"items":{
            "type":"object","properties":{
                "lineId":{"type":"string"},"direction":{"type":"string","enum":["increase","reduce"]},
                "assetId":{"type":"string"},"accountId":{"type":["string","null"]},
                "inputMode":{"type":"string","enum":["amount","quantity"]},"value":decimal
            },"required":["lineId","direction","assetId","inputMode","value"]
        }});
        schema["required"] = json!(["targetId", "filter", "selectedAccountIds", "cash", "lines"]);
    }
    schema
}

fn validate_worksheet_scope(
    env: &Arc<dyn AgentEnvironment>,
    target_id: &str,
    filter: &wealthfolio_core::portfolios::AccountScope,
    selected_account_ids: &[String],
) -> Result<wealthfolio_core::portfolios::ResolvedAccountScope, AgentToolError> {
    if selected_account_ids.len() > 100 {
        return Err(AgentToolError::InvalidInput(
            "At most 100 accounts per call".into(),
        ));
    }
    if let wealthfolio_core::portfolios::AccountScope::Accounts { account_ids } = filter {
        if account_ids.len() > 100 {
            return Err(AgentToolError::InvalidInput(
                "At most 100 accounts per call".into(),
            ));
        }
    }
    let requested = resolve_scope(env, filter)?;
    let resolved = target_scope(env, target_id)?;
    let mut requested_ids = requested.account_ids;
    let mut target_ids = resolved.account_ids.clone();
    requested_ids.sort();
    target_ids.sort();
    if requested_ids != target_ids {
        return Err(AgentToolError::InvalidInput(
            "Worksheet page scope does not match the selected target scope".into(),
        ));
    }
    // Selected-account membership and cash/line validity remain owned by the
    // upstream worksheet service, exactly as in both runtime adapters.
    Ok(resolved)
}

business_tool!(GenerateCalculatedAdjustments, GenerateCalculatedAdjustmentsArgs, "generate_calculated_adjustments",
    "Generate proposed allocation worksheet adjustments using the upstream worksheet model. Cash contributions are assigned explicitly by account; never records trades. The filter must match the saved target scope. Pass the returned adjustments as editable lines to calculate_allocation_worksheet for a projection.",
    worksheet_schema(true), [AgentScope::FinancialPlanningRead, AgentScope::HoldingsRead], Read, |env, args| {
        if args.eligible_asset_ids.as_ref().is_some_and(|ids| ids.len() > 100)
            || args.sellable_asset_ids.as_ref().is_some_and(|ids| ids.len() > 100) {
            return Err(AgentToolError::InvalidInput("At most 100 assets per call".into()));
        }
        let resolved = validate_worksheet_scope(&env, &args.target_id, &args.filter, &args.selected_account_ids)?;
        let input = wealthfolio_core::portfolio::allocation_targets::GenerateCalculatedAdjustmentsInput {
            target_id: args.target_id, account_ids: resolved.account_ids,
            base_currency: resolved.base_currency, aggregated_account_id: resolved.scope_id,
            selected_account_ids: args.selected_account_ids, mode: args.mode, rule: args.rule,
            cash: args.cash, eligible_asset_ids: args.eligible_asset_ids, sellable_asset_ids: args.sellable_asset_ids,
        };
        output(env.allocation_worksheet_service().generate_adjustments(input).await?)
    });

business_tool!(CalculateAllocationWorksheet, CalculateAllocationWorksheetArgs, "calculate_allocation_worksheet",
    "Validate and project caller-supplied allocation worksheet lines using the upstream worksheet model. Does not regenerate adjustments, record trades, or change holdings. The filter must match the saved target scope; cash contributions are assigned explicitly by account.",
    worksheet_schema(false), [AgentScope::FinancialPlanningRead, AgentScope::HoldingsRead], Read, |env, args| {
        if args.lines.len() > 100 { return Err(AgentToolError::InvalidInput("At most 100 lines per call".into())); }
        let resolved = validate_worksheet_scope(&env, &args.target_id, &args.filter, &args.selected_account_ids)?;
        let input = wealthfolio_core::portfolio::allocation_targets::CalculateAllocationWorksheetInput {
            target_id: args.target_id, account_ids: resolved.account_ids,
            base_currency: resolved.base_currency, aggregated_account_id: resolved.scope_id,
            selected_account_ids: args.selected_account_ids, cash: args.cash, lines: args.lines,
        };
        output(env.allocation_worksheet_service().calculate_worksheet(input).await?)
    });

pub fn tools() -> Vec<Arc<dyn AgentTool>> {
    vec![
        Arc::new(CreateAllocationTarget),
        Arc::new(UpdateAllocationTarget),
        Arc::new(ArchiveAllocationTarget),
        Arc::new(SaveAllocationTargetWeights),
        Arc::new(SaveAllocationTargetWithWeights),
        Arc::new(ListAllocationTargets),
        Arc::new(GetAllocationTarget),
        Arc::new(ListAllocationTargetWeights),
        Arc::new(ListTargetConstraints),
        Arc::new(SaveTargetConstraints),
        Arc::new(GetAllocationTargetDrift),
        Arc::new(GenerateCalculatedAdjustments),
        Arc::new(CalculateAllocationWorksheet),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn generation() -> Value {
        json!({"targetId":"target-1", "filter":{"type":"account","accountId":"account-1"},
            "selectedAccountIds":["account-1"], "mode":"invest_cash",
            "rule":"current_holding_proportions", "cash":{"trackedCashToUse":"0",
                "externalContribution":{"account-1":"100"}}})
    }

    #[test]
    fn worksheet_schema_matches_upstream_request_models() {
        let args: GenerateCalculatedAdjustmentsArgs = serde_json::from_value(generation()).unwrap();
        assert_eq!(
            args.cash.external_contribution["account-1"],
            rust_decimal::Decimal::from(100)
        );
        assert_eq!(args.selected_account_ids, vec!["account-1"]);
        let schema = GenerateCalculatedAdjustments.input_schema();
        for mode in [
            wealthfolio_core::portfolio::allocation_targets::WorksheetMode::InvestCash,
            wealthfolio_core::portfolio::allocation_targets::WorksheetMode::Rebalance,
        ] {
            assert!(schema["properties"]["mode"]["enum"]
                .as_array()
                .unwrap()
                .contains(&serde_json::to_value(mode).unwrap()));
        }
        assert!(schema["properties"].get("availableCash").is_none());
        assert!(schema["properties"].get("scenarioMode").is_none());
        let preview: CalculateAllocationWorksheetArgs = serde_json::from_value(json!({
            "targetId":"target-1", "filter":{"type":"account","accountId":"account-1"},
            "selectedAccountIds":["account-1"], "cash":{"trackedCashToUse":"0"},
            "lines":[{"lineId":"line-1","direction":"increase","assetId":"asset-1",
                "accountId":null,"inputMode":"amount","value":"10"}]
        }))
        .unwrap();
        assert_eq!(preview.lines[0].value, rust_decimal::Decimal::from(10));
        assert_eq!(preview.lines[0].account_id, None);
        assert_eq!(tools().len(), 13);
    }

    #[test]
    fn worksheet_rejects_legacy_single_step_parameters() {
        let mut legacy = generation();
        legacy["availableCash"] = json!("100");
        legacy["scenarioMode"] = json!("cash_flow_only");
        assert!(serde_json::from_value::<GenerateCalculatedAdjustmentsArgs>(legacy).is_err());
        let mut invalid = generation();
        invalid["mode"] = json!("hybrid");
        assert!(serde_json::from_value::<GenerateCalculatedAdjustmentsArgs>(invalid).is_err());
    }
}
