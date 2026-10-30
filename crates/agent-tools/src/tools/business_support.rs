//! Small helpers shared by MCP business tools; no transport or persistence ownership.
use crate::{AgentEnvironment, AgentToolError, AgentToolResult};
use rust_decimal::prelude::ToPrimitive;
use serde::Serialize;
use std::{collections::HashMap, sync::Arc};
use wealthfolio_core::{
    accounts::AccountPurpose,
    goals::SaveGoalPlan,
    portfolio::{allocation_targets::ScopeType, valuation::CurrentAccountValuationService},
    portfolios::{AccountScope, ResolvedAccountScope},
    utils::time_utils::{parse_user_timezone_or_default, user_today},
};

pub fn output(value: impl Serialize) -> Result<AgentToolResult, AgentToolError> {
    Ok(AgentToolResult {
        content: serde_json::to_value(value)?,
    })
}

pub fn timezone(env: &Arc<dyn AgentEnvironment>) -> Result<String, AgentToolError> {
    Ok(env.settings_service().get_settings()?.timezone)
}

pub fn resolve_scope(
    env: &Arc<dyn AgentEnvironment>,
    scope: &AccountScope,
) -> Result<ResolvedAccountScope, AgentToolError> {
    Ok(env.portfolio_service().resolve_account_scope_for_purpose(
        scope,
        &env.base_currency(),
        AccountPurpose::Holdings,
    )?)
}

pub fn target_scope(
    env: &Arc<dyn AgentEnvironment>,
    target_id: &str,
) -> Result<ResolvedAccountScope, AgentToolError> {
    let target = env
        .allocation_target_service()
        .get_target(target_id)?
        .ok_or_else(|| AgentToolError::NotFound(target_id.into()))?;
    let scope = match target.scope_type {
        ScopeType::All => AccountScope::All,
        ScopeType::Account => AccountScope::Account {
            account_id: target
                .scope_id
                .ok_or_else(|| AgentToolError::InvalidInput("Target scopeId is missing".into()))?,
        },
        ScopeType::Portfolio => AccountScope::Portfolio {
            portfolio_id: target
                .scope_id
                .ok_or_else(|| AgentToolError::InvalidInput("Target scopeId is missing".into()))?,
        },
    };
    resolve_scope(env, &scope)
}

pub async fn valuation_map(
    env: &Arc<dyn AgentEnvironment>,
) -> Result<HashMap<String, f64>, AgentToolError> {
    let accounts = env.account_service();
    let ids: Vec<_> = accounts
        .get_active_non_archived_accounts()?
        .into_iter()
        .map(|a| a.id)
        .collect();
    let snapshots = env.snapshot_service();
    let assets = env.asset_service();
    let quotes = env.quote_service();
    let fx = env.fx_service();
    let service = CurrentAccountValuationService::new(
        accounts.as_ref(),
        snapshots.as_ref(),
        assets.as_ref(),
        quotes.as_ref(),
        fx.as_ref(),
    );
    let cutoff = user_today(parse_user_timezone_or_default(&timezone(env)?));
    let result = service
        .get_current_valuation_for_scope("all", &ids, &env.base_currency(), cutoff, true)
        .await?;
    result
        .accounts
        .into_iter()
        .map(|account| {
            account
                .total_value_base
                .to_f64()
                .map(|v| (account.account_id, v))
                .ok_or_else(|| AgentToolError::ExecutionFailed("Invalid account valuation".into()))
        })
        .collect()
}

pub async fn refresh_summary_after_save(env: &Arc<dyn AgentEnvironment>, goal_id: &str) {
    // Match the UI: summary failure must not undo a successfully saved plan/funding rule.
    let result = async {
        env.goal_service().refresh_goal_summary(goal_id).await?;
        Ok::<_, AgentToolError>(())
    }
    .await;
    if result.is_err() {
        log::warn!("Goal summary refresh failed after saving goal configuration");
    }
}

pub fn normalize_plan_currency(plan: &mut SaveGoalPlan, currency: &str) {
    if plan.plan_kind != "retirement" {
        return;
    }
    if let Ok(mut settings) = serde_json::from_str::<serde_json::Value>(&plan.settings_json) {
        if let Some(object) = settings.as_object_mut() {
            object.insert(
                "currency".into(),
                serde_json::Value::String(currency.into()),
            );
        }
        if let Ok(json) = serde_json::to_string(&settings) {
            plan.settings_json = json;
        }
    }
}

pub fn validate_account_connection(
    provider: &Option<String>,
    provider_account_id: &Option<String>,
) -> Result<(), AgentToolError> {
    if provider.is_some() || provider_account_id.is_some() {
        return Err(AgentToolError::InvalidInput(
            "Broker connection fields cannot be changed through MCP".into(),
        ));
    }
    Ok(())
}

pub fn auto_categorize(env: Arc<dyn AgentEnvironment>, accounts: Vec<String>) {
    if accounts.is_empty() {
        return;
    }
    tokio::spawn(async move {
        if env
            .categorization_rules_service()
            .rerun_all(&accounts, true)
            .await
            .is_err()
        {
            log::warn!("Background spending categorization failed");
        }
    });
}

pub async fn auto_categorize_opted_in(env: Arc<dyn AgentEnvironment>) {
    match env.spending_settings_service().get().await {
        Ok(settings) if settings.enabled => auto_categorize(env, settings.account_ids),
        Ok(_) => {}
        Err(_) => log::warn!("Could not load spending scope after rule change"),
    }
}

// The MCP-only service tools share transport metadata, deserialization and audit redaction.
macro_rules! business_tool {
    ($tool:ident, $args_ty:ident, $name:literal, $description:literal,
     $schema:expr, [$($scope:expr),*], $access:ident, |$env:ident, $args:ident| { $($body:tt)* }) => {
        pub struct $tool;
        #[async_trait::async_trait]
        impl AgentTool for $tool {
            fn name(&self) -> &'static str { $name }
            fn description(&self) -> &'static str { $description }
            fn input_schema(&self) -> Value { $schema }
            fn required_scopes(&self) -> &'static [AgentScope] { &[$($scope),*] }
            fn access_level(&self) -> AgentToolAccess { AgentToolAccess::$access }
            fn sanitize_args_for_audit(&self, _args: &Value) -> Value { Value::Null }
            fn sanitize_error_for_audit(&self, error: &AgentToolError) -> String {
                match error {
                    AgentToolError::ScopeDenied { .. } => error.to_string(),
                    _ => "Business tool failed; details omitted from audit".into(),
                }
            }
            async fn call(&self, $env: Arc<dyn AgentEnvironment>, args: Value) -> Result<AgentToolResult, AgentToolError> {
                let $args: $args_ty = serde_json::from_value(args)?;
                $($body)*
            }
        }
    };
}
