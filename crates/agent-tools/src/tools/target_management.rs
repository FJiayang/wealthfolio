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
struct CalculateRebalancePlanArgs {
    pub target_id: String,
    pub available_cash: rust_decimal::Decimal,
    pub filter: wealthfolio_core::portfolios::AccountScope,
    #[serde(default)]
    pub scenario_mode: Option<wealthfolio_core::portfolio::allocation_targets::ScenarioMode>,
    #[serde(default)]
    pub eligible_asset_ids: Option<Vec<String>>,
}

business_tool!(CalculateRebalancePlan, CalculateRebalancePlanArgs, "calculate_rebalance_plan",
    "Calculate proposed trades only; never records activities or changes holdings. Use availableCash in base currency, the existing scenario modes and optional eligibleAssetIds.",
    json!({"type":"object","properties":{"targetId":{"type":"string"},"availableCash":{"oneOf":[{"type":"string"},{"type":"number"}]},"filter":{"type":"object","properties":{"type":{"type":"string","enum":["all","account","portfolio","accounts"]},"accountId":{"type":"string"},"portfolioId":{"type":"string"},"accountIds":{"type":"array","items":{"type":"string"},"maxItems":100}},"required":["type"]},"scenarioMode":{"type":"string","enum":["cash_flow_only","sell_to_rebalance","hybrid"]},"eligibleAssetIds":{"type":"array","items":{"type":"string"},"maxItems":100}},"required":["targetId","availableCash","filter"]}), [AgentScope::FinancialPlanningRead, AgentScope::HoldingsRead], Read, |env, args| {
        let resolved = resolve_scope(&env, &args.filter)?;
        let input = wealthfolio_core::portfolio::allocation_targets::CalculateRebalancePlanInput {
            target_id: args.target_id, available_cash: args.available_cash, account_ids: resolved.account_ids,
            base_currency: env.base_currency(), aggregated_account_id: resolved.scope_id,
            scenario_mode: args.scenario_mode.unwrap_or_default(), eligible_asset_ids: args.eligible_asset_ids,
        };
        output(env.rebalance_service().calculate_plan(input).await?)
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
        Arc::new(CalculateRebalancePlan),
    ]
}
