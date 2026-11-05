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

pub fn tools() -> Vec<Arc<dyn AgentTool>> {
    vec![
        Arc::new(GetSpendingSettings),
        Arc::new(UpdateSpendingSettings),
        Arc::new(CreateEventType),
        Arc::new(UpdateEventType),
    ]
}

fn deserialize_optional_color<'de, D>(deserializer: D) -> Result<Option<Option<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Option::<String>::deserialize(deserializer).map(Some)
}
