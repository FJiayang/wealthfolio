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
struct ListCashActivitiesArgs {
    #[serde(default)]
    pub filter: Option<wealthfolio_spending::cash_activities::CashActivityFilter>,
}

business_tool!(
    ListCashActivities,
    ListCashActivitiesArgs,
    "list_cash_activities",
    "List cash activities in the configured spending scope using existing filters.",
    json!({"type":"object","properties":{"filter":{"type":"object","properties":{"accountIds":{"type":"array","items":{"type":"string"},"maxItems":100},"startDate":{"type":"string"},"endDate":{"type":"string"},"activityTypes":{"type":"array","items":{"type":"string"},"maxItems":100}},"required":[]}},"required":[]}),
    [AgentScope::SpendingRead],
    Read,
    |env, args| {
        output(
            env.cash_activity_service()
                .list(args.filter.unwrap_or_default())
                .await?,
        )
    }
);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SearchCashActivitiesArgs {
    pub request: wealthfolio_spending::cash_activities::CashActivitySearchRequest,
}

business_tool!(
    SearchCashActivities,
    SearchCashActivitiesArgs,
    "search_cash_activities",
    "Search spending activity rows with pagination, category, amount and status filters.",
    json!({"type":"object","properties":{"request":{"type":"object","properties":{"accountIds":{"type":"array","items":{"type":"string"},"maxItems":100},"startDate":{"type":"string"},"endDate":{"type":"string"},"activityTypes":{"type":"array","items":{"type":"string"},"maxItems":100},"search":{"type":"string"},"categoryIds":{"type":"array","items":{"type":"string"},"maxItems":100},"subcategoryIds":{"type":"array","items":{"type":"string"},"maxItems":100},"eventIds":{"type":"array","items":{"type":"string"},"maxItems":100},"status":{"type":"string","enum":["all","needs_review","uncategorized","categorized"]},"minAmount":{"type":"number"},"maxAmount":{"type":"number"},"sortBy":{"type":"string","enum":["date","amount"]},"sortDir":{"type":"string","enum":["asc","desc"]},"offset":{"type":"integer"},"limit":{"type":"integer"},"includeCashBalances":{"type":"boolean"}},"required":[]}},"required":["request"]}),
    [AgentScope::SpendingRead],
    Read,
    |env, args| { output(env.cash_activity_service().search(args.request).await?) }
);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GetActivityAssignmentsArgs {
    pub activity_id: String,
}

business_tool!(
    GetActivityAssignments,
    GetActivityAssignmentsArgs,
    "get_activity_assignments",
    "Read category assignments or splits for an activity within the spending scope.",
    json!({"type":"object","properties":{"activityId":{"type":"string"}},"required":["activityId"]}),
    [AgentScope::SpendingRead, AgentScope::ClassificationRead],
    Read,
    |env, args| {
        output(
            env.cash_activity_service()
                .list_assignments(&args.activity_id)
                .await?,
        )
    }
);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GetActivitySplitsArgs {
    pub activity_id: String,
}

business_tool!(
    GetActivitySplits,
    GetActivitySplitsArgs,
    "get_activity_splits",
    "Read category assignments or splits for an activity within the spending scope.",
    json!({"type":"object","properties":{"activityId":{"type":"string"}},"required":["activityId"]}),
    [AgentScope::SpendingRead, AgentScope::ClassificationRead],
    Read,
    |env, args| {
        output(
            env.cash_activity_service()
                .list_splits(&args.activity_id)
                .await?,
        )
    }
);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AssignActivityCategoryArgs {
    pub activity_id: String,
    pub taxonomy_id: String,
    pub category_id: String,
}

business_tool!(AssignActivityCategory, AssignActivityCategoryArgs, "assign_activity_category",
    "Assign a category through the existing spending validation; replaces conflicting splits as the frontend does.",
    json!({"type":"object","properties":{"activityId":{"type":"string"},"taxonomyId":{"type":"string"},"categoryId":{"type":"string"}},"required":["activityId","taxonomyId","categoryId"]}), [AgentScope::ClassificationSuggest, AgentScope::ClassificationWrite], Write, |env, args| {
        output(env.cash_activity_service().assign_category(&args.activity_id, &args.taxonomy_id, &args.category_id).await?)
    });

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReplaceActivitySplitsArgs {
    pub activity_id: String,
    pub splits: Vec<wealthfolio_spending::activity_splits::NewActivitySplit>,
}

business_tool!(ReplaceActivitySplits, ReplaceActivitySplitsArgs, "replace_activity_splits",
    "Replace the complete non-empty split list for an activity. The service checks categories and amounts and clears conflicting assignments.",
    json!({"type":"object","properties":{"activityId":{"type":"string"},"splits":{"type":"array","items":{"type":"object","properties":{"taxonomyId":{"type":"string"},"categoryId":{"type":"string"},"amount":{"oneOf":[{"type":"string"},{"type":"number"}]},"note":{"type":"string"},"sortOrder":{"type":"integer"}},"required":["taxonomyId","categoryId","amount"]},"maxItems":100}},"required":["activityId","splits"]}), [AgentScope::SpendingWrite, AgentScope::ClassificationSuggest, AgentScope::ClassificationWrite], Write, |env, args| {
        if args.splits.len() > 100 { return Err(AgentToolError::InvalidInput("At most 100 items per call".into())); }
        output(env.cash_activity_service().replace_splits(&args.activity_id, args.splits).await?)
    });

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SetActivityEventArgs {
    pub activity_id: String,
    #[serde(default)]
    pub event_id: Option<String>,
}

business_tool!(
    SetActivityEvent,
    SetActivityEventArgs,
    "set_activity_event",
    "Set or remove an activity event association. The activity itself is not deleted.",
    json!({"type":"object","properties":{"activityId":{"type":"string"},"eventId":{"type":"string"}},"required":["activityId"]}),
    [AgentScope::SpendingWrite],
    Write,
    |env, args| {
        output(
            env.cash_activity_service()
                .set_event(&args.activity_id, args.event_id)
                .await?,
        )
    }
);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GetSpendingSettingsArgs {}

business_tool!(
    GetSpendingSettings,
    GetSpendingSettingsArgs,
    "get_spending_settings",
    "Read the spending business settings and account scope, not application settings.",
    json!({"type":"object","properties":{},"required":[]}),
    [AgentScope::SpendingRead],
    Read,
    |env, args| {
        let _ = args;
        output(env.spending_settings_service().get().await?)
    }
);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct UpdateSpendingSettingsArgs {
    pub input: wealthfolio_spending::settings::SpendingSettingsUpdate,
}

business_tool!(UpdateSpendingSettings, UpdateSpendingSettingsArgs, "update_spending_settings",
    "Update spending opt-in accounts and excluded categories. Broadening scope triggers the existing background uncategorized-only rule pass; manual edits are preserved.",
    json!({"type":"object","properties":{"input":{"type":"object","properties":{"enabled":{"type":"boolean"},"accountIds":{"type":"array","items":{"type":"string"},"maxItems":100},"excludedCategoryIds":{"type":"array","items":{"type":"string"},"maxItems":100}},"required":[]}},"required":["input"]}), [AgentScope::SpendingWrite, AgentScope::ClassificationSuggest, AgentScope::ClassificationWrite], Write, |env, args| {
        let (before, after) = env.spending_settings_service().update_with_previous(args.input).await?;
        let accounts = if !before.enabled && after.enabled { after.account_ids.clone() }
        else if after.enabled { after.account_ids.iter().filter(|id| !before.account_ids.contains(id)).cloned().collect() }
        else { Vec::new() };
        auto_categorize(env.clone(), accounts);
        output(after)
    });

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ListEventTypesArgs {}

business_tool!(
    ListEventTypes,
    ListEventTypesArgs,
    "list_event_types",
    "List spending events or event types.",
    json!({"type":"object","properties":{},"required":[]}),
    [AgentScope::SpendingRead],
    Read,
    |env, args| {
        let _ = args;
        output(env.events_service().list_types().await?)
    }
);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ListEventsArgs {}

business_tool!(
    ListEvents,
    ListEventsArgs,
    "list_events",
    "List spending events or event types.",
    json!({"type":"object","properties":{},"required":[]}),
    [AgentScope::SpendingRead],
    Read,
    |env, args| {
        let _ = args;
        output(env.events_service().list_events().await?)
    }
);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CreateEventTypeArgs {
    pub input: wealthfolio_spending::events::NewEventType,
}

business_tool!(
    CreateEventType,
    CreateEventTypeArgs,
    "create_event_type",
    "Save spending event configuration using existing validation.",
    json!({"type":"object","properties":{"input":{"type":"object","properties":{"name":{"type":"string"},"color":{"type":"string"},"id":{"type":"string"}},"required":["name"]}},"required":["input"]}),
    [AgentScope::SpendingWrite],
    Write,
    |env, args| { output(env.events_service().create_type(args.input).await?) }
);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct UpdateEventTypeArgs {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_color")]
    pub color: Option<Option<String>>,
}

business_tool!(
    UpdateEventType,
    UpdateEventTypeArgs,
    "update_event_type",
    "Save spending event configuration using existing validation.",
    json!({"type":"object","properties":{"id":{"type":"string"},"name":{"type":"string"},"color":{"type":["string","null"]}},"required":["id"]}),
    [AgentScope::SpendingWrite],
    Write,
    |env, args| {
        output(
            env.events_service()
                .update_type(&args.id, args.name, args.color)
                .await?,
        )
    }
);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CreateEventArgs {
    pub input: wealthfolio_spending::events::NewEvent,
}

business_tool!(
    CreateEvent,
    CreateEventArgs,
    "create_event",
    "Save spending event configuration using existing validation.",
    json!({"type":"object","properties":{"input":{"type":"object","properties":{"name":{"type":"string"},"description":{"type":"string"},"eventTypeId":{"type":"string"},"startDate":{"type":"string"},"endDate":{"type":"string"},"id":{"type":"string"}},"required":["name","eventTypeId","startDate","endDate"]}},"required":["input"]}),
    [AgentScope::SpendingWrite],
    Write,
    |env, args| { output(env.events_service().create_event(args.input).await?) }
);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct UpdateEventArgs {
    pub id: String,
    pub patch: wealthfolio_spending::events::UpdateEvent,
}

business_tool!(
    UpdateEvent,
    UpdateEventArgs,
    "update_event",
    "Save spending event configuration using existing validation.",
    json!({"type":"object","properties":{"id":{"type":"string"},"patch":{"type":"object","properties":{"name":{"type":"string"},"description":{"type":["string","null"]},"eventTypeId":{"type":"string"},"startDate":{"type":"string"},"endDate":{"type":"string"}},"required":[]}},"required":["id","patch"]}),
    [AgentScope::SpendingWrite],
    Write,
    |env, args| {
        output(
            env.events_service()
                .update_event(&args.id, args.patch)
                .await?,
        )
    }
);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct UpsertBudgetTargetArgs {
    pub input: wealthfolio_spending::budget::NewBudgetTarget,
    #[serde(default)]
    pub period_key: Option<String>,
}

business_tool!(UpsertBudgetTarget, UpsertBudgetTargetArgs, "upsert_budget_target",
    "Save budget configuration through the existing budget service. Cross-period copies overwrite existing targets only when overwrite is true.",
    json!({"type":"object","properties":{"input":{"type":"object","properties":{"id":{"type":"string"},"periodKey":{"type":"string"},"targetType":{"type":"string","enum":["category","group_buffer"]},"taxonomyId":{"type":"string"},"categoryId":{"type":"string"},"groupId":{"type":"string"},"amount":{"type":"string"}},"required":["periodKey","targetType","amount"]},"periodKey":{"type":"string"}},"required":["input"]}), [AgentScope::SpendingRead, AgentScope::BudgetWrite], Write, |env, args| {
        output(env.budget_management_service().upsert_target(args.input, args.period_key, &env.base_currency(), &timezone(&env)?).await?)
    });

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct UpsertBudgetRolloverSettingArgs {
    pub input: wealthfolio_spending::budget::NewBudgetRolloverSetting,
    #[serde(default)]
    pub period_key: Option<String>,
}

business_tool!(UpsertBudgetRolloverSetting, UpsertBudgetRolloverSettingArgs, "upsert_budget_rollover_setting",
    "Save budget configuration through the existing budget service. Cross-period copies overwrite existing targets only when overwrite is true.",
    json!({"type":"object","properties":{"input":{"type":"object","properties":{"id":{"type":"string"},"targetType":{"type":"string","enum":["category","group"]},"taxonomyId":{"type":"string"},"categoryId":{"type":"string"},"groupId":{"type":"string"},"enabled":{"type":"boolean"},"startMonth":{"type":"string"},"startingBalance":{"type":"string"}},"required":["targetType","startMonth","startingBalance"]},"periodKey":{"type":"string"}},"required":["input"]}), [AgentScope::SpendingRead, AgentScope::BudgetWrite], Write, |env, args| {
        output(env.budget_management_service().upsert_rollover_setting(args.input, args.period_key, &env.base_currency(), &timezone(&env)?).await?)
    });

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CreateBudgetGroupArgs {
    pub input: wealthfolio_spending::budget::NewBudgetGroup,
    #[serde(default)]
    pub period_key: Option<String>,
}

business_tool!(CreateBudgetGroup, CreateBudgetGroupArgs, "create_budget_group",
    "Save budget configuration through the existing budget service. Cross-period copies overwrite existing targets only when overwrite is true.",
    json!({"type":"object","properties":{"input":{"type":"object","properties":{"name":{"type":"string"},"color":{"type":"string"},"icon":{"type":"string"},"sortOrder":{"type":"integer"},"id":{"type":"string"},"key":{"type":"string"}},"required":["name"]},"periodKey":{"type":"string"}},"required":["input"]}), [AgentScope::SpendingRead, AgentScope::BudgetWrite], Write, |env, args| {
        output(env.budget_management_service().create_group(args.input, args.period_key, &env.base_currency(), &timezone(&env)?).await?)
    });

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct UpdateBudgetGroupArgs {
    pub id: String,
    pub patch: wealthfolio_spending::budget::UpdateBudgetGroup,
    #[serde(default)]
    pub period_key: Option<String>,
}

business_tool!(UpdateBudgetGroup, UpdateBudgetGroupArgs, "update_budget_group",
    "Save budget configuration through the existing budget service. Cross-period copies overwrite existing targets only when overwrite is true.",
    json!({"type":"object","properties":{"id":{"type":"string"},"patch":{"type":"object","properties":{"name":{"type":"string"},"color":{"type":["string","null"]},"icon":{"type":["string","null"]},"sortOrder":{"type":"integer"}},"required":[]},"periodKey":{"type":"string"}},"required":["id","patch"]}), [AgentScope::SpendingRead, AgentScope::BudgetWrite], Write, |env, args| {
        output(env.budget_management_service().update_group(&args.id, args.patch, args.period_key, &env.base_currency(), &timezone(&env)?).await?)
    });

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AssignCategoryToGroupArgs {
    pub category_id: String,
    pub group_id: String,
    #[serde(default)]
    pub period_key: Option<String>,
}

business_tool!(AssignCategoryToGroup, AssignCategoryToGroupArgs, "assign_category_to_group",
    "Save budget configuration through the existing budget service. Cross-period copies overwrite existing targets only when overwrite is true.",
    json!({"type":"object","properties":{"categoryId":{"type":"string"},"groupId":{"type":"string"},"periodKey":{"type":"string"}},"required":["categoryId","groupId"]}), [AgentScope::SpendingRead, AgentScope::BudgetWrite], Write, |env, args| {
        output(env.budget_management_service().assign_category_to_group(args.category_id, args.group_id, args.period_key, &env.base_currency(), &timezone(&env)?).await?)
    });

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CopyBudgetTargetsArgs {
    pub source_period_key: String,
    pub target_period_key: String,
    pub overwrite: bool,
}

business_tool!(CopyBudgetTargets, CopyBudgetTargetsArgs, "copy_budget_targets",
    "Save budget configuration through the existing budget service. Cross-period copies overwrite existing targets only when overwrite is true.",
    json!({"type":"object","properties":{"sourcePeriodKey":{"type":"string"},"targetPeriodKey":{"type":"string"},"overwrite":{"type":"boolean"}},"required":["sourcePeriodKey","targetPeriodKey","overwrite"]}), [AgentScope::SpendingRead, AgentScope::BudgetWrite], Write, |env, args| {
        output(env.budget_management_service().copy_period_targets(&args.source_period_key, &args.target_period_key, args.overwrite, &env.base_currency(), &timezone(&env)?).await?)
    });

pub fn tools() -> Vec<Arc<dyn AgentTool>> {
    vec![
        Arc::new(ListCashActivities),
        Arc::new(SearchCashActivities),
        Arc::new(GetActivityAssignments),
        Arc::new(GetActivitySplits),
        Arc::new(AssignActivityCategory),
        Arc::new(ReplaceActivitySplits),
        Arc::new(SetActivityEvent),
        Arc::new(GetSpendingSettings),
        Arc::new(UpdateSpendingSettings),
        Arc::new(ListEventTypes),
        Arc::new(ListEvents),
        Arc::new(CreateEventType),
        Arc::new(UpdateEventType),
        Arc::new(CreateEvent),
        Arc::new(UpdateEvent),
        Arc::new(UpsertBudgetTarget),
        Arc::new(UpsertBudgetRolloverSetting),
        Arc::new(CreateBudgetGroup),
        Arc::new(UpdateBudgetGroup),
        Arc::new(AssignCategoryToGroup),
        Arc::new(CopyBudgetTargets),
    ]
}

fn deserialize_optional_color<'de, D>(deserializer: D) -> Result<Option<Option<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Option::<String>::deserialize(deserializer).map(Some)
}
