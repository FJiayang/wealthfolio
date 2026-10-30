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
struct CreateGoalArgs {
    pub input: wealthfolio_core::goals::NewGoal,
}

business_tool!(
    CreateGoal,
    CreateGoalArgs,
    "create_goal",
    "Create an investment or saving goal. Currency follows the application base currency.",
    json!({"type":"object","properties":{"input":{"type":"object","properties":{"goalType":{"type":"string"},"title":{"type":"string"},"description":{"type":"string"},"targetAmount":{"type":"number"},"statusLifecycle":{"type":"string"},"statusHealth":{"type":"string"},"priority":{"type":"integer"},"coverImageKey":{"type":"string"},"startDate":{"type":"string"},"targetDate":{"type":"string"}},"required":["goalType","title"]}},"required":["input"]}),
    [AgentScope::FinancialPlanningWrite],
    Write,
    |env, args| {
        let mut input = args.input;
        input.currency = Some(env.base_currency());
        output(env.goal_service().create_goal(input).await?)
    }
);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct UpdateGoalArgs {
    pub goal_id: String,
    pub input: wealthfolio_core::goals::GoalUpdate,
}

business_tool!(
    UpdateGoal,
    UpdateGoalArgs,
    "update_goal",
    "Update a complete goal returned by get_goal. Currency follows the application base currency.",
    json!({"type":"object","properties":{"goalId":{"type":"string"},"input":{"type":"object","properties":{"goalType":{"type":"string"},"title":{"type":"string"},"description":{"type":"string"},"targetAmount":{"type":"number"},"statusLifecycle":{"type":"string"},"statusHealth":{"type":"string"},"priority":{"type":"integer"},"coverImageKey":{"type":"string"},"startDate":{"type":"string"},"targetDate":{"type":["string","null"]},"id":{"type":"string"},"createdAt":{"type":"string"},"updatedAt":{"type":"string"}},"required":["id","goalType","title","statusLifecycle","statusHealth","priority","createdAt","updatedAt"]}},"required":["goalId","input"]}),
    [AgentScope::FinancialPlanningWrite],
    Write,
    |env, args| {
        let mut goal = args.input;
        if goal.goal.id != args.goal_id {
            return Err(AgentToolError::InvalidInput(
                "goalId must match input.id".into(),
            ));
        }
        goal.goal.currency = Some(env.base_currency());
        output(env.goal_service().update_goal(goal).await?)
    }
);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GetGoalArgs {
    pub goal_id: String,
}

business_tool!(
    GetGoal,
    GetGoalArgs,
    "get_goal",
    "Get a complete goal including its saved progress summary.",
    json!({"type":"object","properties":{"goalId":{"type":"string"}},"required":["goalId"]}),
    [AgentScope::FinancialPlanningRead],
    Read,
    |env, args| { output(env.goal_service().get_goal(&args.goal_id)?) }
);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GetGoalFundingArgs {
    pub goal_id: String,
}

business_tool!(
    GetGoalFunding,
    GetGoalFundingArgs,
    "get_goal_funding",
    "Read saved goal funding rules or plan.",
    json!({"type":"object","properties":{"goalId":{"type":"string"}},"required":["goalId"]}),
    [AgentScope::FinancialPlanningRead],
    Read,
    |env, args| { output(env.goal_service().get_goal_funding(&args.goal_id)?) }
);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GetGoalPlanArgs {
    pub goal_id: String,
}

business_tool!(
    GetGoalPlan,
    GetGoalPlanArgs,
    "get_goal_plan",
    "Read saved goal funding rules or plan.",
    json!({"type":"object","properties":{"goalId":{"type":"string"}},"required":["goalId"]}),
    [AgentScope::FinancialPlanningRead],
    Read,
    |env, args| { output(env.goal_service().get_goal_plan(&args.goal_id)?) }
);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SaveGoalFundingArgs {
    pub goal_id: String,
    pub rules: Vec<wealthfolio_core::goals::GoalFundingRuleInput>,
}

business_tool!(SaveGoalFunding, SaveGoalFundingArgs, "save_goal_funding",
    "Replace the complete goal funding rules, then refresh the goal summary. Omitted old rules are removed. A summary refresh failure does not undo the saved rules.",
    json!({"type":"object","properties":{"goalId":{"type":"string"},"rules":{"type":"array","items":{"type":"object","properties":{"accountId":{"type":"string"},"sharePercent":{"type":"number"},"taxBucket":{"type":"string"}},"required":["accountId","sharePercent"]},"maxItems":100}},"required":["goalId","rules"]}), [AgentScope::FinancialPlanningWrite, AgentScope::HoldingsRead], Write, |env, args| {
        if args.rules.len() > 100 { return Err(AgentToolError::InvalidInput("At most 100 items per call".into())); }
        let saved = env.goal_service().save_goal_funding(&args.goal_id, args.rules).await?;
        refresh_summary_after_save(&env, &args.goal_id).await;
        output(saved)
    });

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SaveGoalPlanArgs {
    pub input: wealthfolio_core::goals::SaveGoalPlan,
}

business_tool!(SaveGoalPlan, SaveGoalPlanArgs, "save_goal_plan",
    "Save a complete goal plan and refresh its summary. Retirement plan currency follows base currency. Summary refresh failure does not undo the saved plan.",
    json!({"type":"object","properties":{"input":{"type":"object","properties":{"goalId":{"type":"string"},"planKind":{"type":"string"},"plannerMode":{"type":"string"},"settingsJson":{"type":"string"},"summaryJson":{"type":"string"}},"required":["goalId","planKind","settingsJson"]}},"required":["input"]}), [AgentScope::FinancialPlanningWrite, AgentScope::HoldingsRead], Write, |env, args| {
        let mut input = args.input;
        normalize_plan_currency(&mut input, &env.base_currency());
        let goal_id = input.goal_id.clone();
        let saved = env.goal_service().save_goal_plan(input).await?;
        refresh_summary_after_save(&env, &goal_id).await;
        output(saved)
    });

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RefreshGoalSummaryArgs {
    pub goal_id: String,
}

business_tool!(
    RefreshGoalSummary,
    RefreshGoalSummaryArgs,
    "refresh_goal_summary",
    "Compute goal progress from current account valuations using the existing goal service.",
    json!({"type":"object","properties":{"goalId":{"type":"string"}},"required":["goalId"]}),
    [AgentScope::FinancialPlanningWrite, AgentScope::HoldingsRead],
    Write,
    |env, args| {
        output(
            env.goal_service()
                .refresh_goal_summary(&args.goal_id)
                .await?,
        )
    }
);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GetRetirementOverviewArgs {
    pub goal_id: String,
}

business_tool!(
    GetRetirementOverview,
    GetRetirementOverviewArgs,
    "get_retirement_overview",
    "Compute goal progress from current account valuations using the existing goal service.",
    json!({"type":"object","properties":{"goalId":{"type":"string"}},"required":["goalId"]}),
    [AgentScope::FinancialPlanningRead, AgentScope::HoldingsRead],
    Read,
    |env, args| {
        let valuations = valuation_map(&env).await?;
        output(
            env.goal_service()
                .compute_retirement_overview(&args.goal_id, &valuations)
                .await?,
        )
    }
);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GetSaveUpOverviewArgs {
    pub goal_id: String,
}

business_tool!(
    GetSaveUpOverview,
    GetSaveUpOverviewArgs,
    "get_save_up_overview",
    "Compute goal progress from current account valuations using the existing goal service.",
    json!({"type":"object","properties":{"goalId":{"type":"string"}},"required":["goalId"]}),
    [AgentScope::FinancialPlanningRead, AgentScope::HoldingsRead],
    Read,
    |env, args| {
        let valuations = valuation_map(&env).await?;
        output(
            env.goal_service()
                .compute_save_up_overview(&args.goal_id, &valuations)
                .await?,
        )
    }
);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RefreshAllGoalSummariesArgs {}

business_tool!(RefreshAllGoalSummaries, RefreshAllGoalSummariesArgs, "refresh_all_goal_summaries",
    "Refresh all goal progress summaries using current valuations. Does not rebuild portfolio history.",
    json!({"type":"object","properties":{},"required":[]}), [AgentScope::FinancialPlanningWrite, AgentScope::HoldingsRead], Write, |env, args| {
        let _ = args;
        let mut goals = Vec::new();
        // Preserve this tool's all-goal, fail-on-error contract. The upstream
        // bulk helper selects active goals and isolates per-goal errors instead.
        for goal in env.goal_service().get_goals()? { goals.push(env.goal_service().refresh_goal_summary(&goal.id).await?); }
        output(goals)
    });

pub fn tools() -> Vec<Arc<dyn AgentTool>> {
    vec![
        Arc::new(CreateGoal),
        Arc::new(UpdateGoal),
        Arc::new(GetGoal),
        Arc::new(GetGoalFunding),
        Arc::new(GetGoalPlan),
        Arc::new(SaveGoalFunding),
        Arc::new(SaveGoalPlan),
        Arc::new(RefreshGoalSummary),
        Arc::new(GetRetirementOverview),
        Arc::new(GetSaveUpOverview),
        Arc::new(RefreshAllGoalSummaries),
    ]
}

#[cfg(test)]
mod tests {
    use super::UpdateGoalArgs;
    use serde_json::json;

    #[test]
    fn goal_update_preserves_upstream_deadline_presence() {
        let input = json!({
            "id": "goal-1", "goalType": "save-up", "title": "Goal",
            "statusLifecycle": "active", "statusHealth": "on-track", "priority": 0,
            "createdAt": "2026-10-30", "updatedAt": "2026-10-30"
        });
        let parse = |input| {
            serde_json::from_value::<UpdateGoalArgs>(json!({
                "goalId": "goal-1", "input": input
            }))
            .unwrap()
        };
        assert_eq!(parse(input.clone()).input.target_date, None);
        let mut cleared = input.clone();
        cleared["targetDate"] = serde_json::Value::Null;
        assert_eq!(parse(cleared).input.target_date, Some(None));
        let mut dated = input;
        dated["targetDate"] = json!("2030-01-01");
        assert_eq!(
            parse(dated).input.target_date,
            Some(Some("2030-01-01".into()))
        );
    }
}
