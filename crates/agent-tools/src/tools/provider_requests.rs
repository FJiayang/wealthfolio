//! Local diagnostics only: these tools never dispatch provider requests.
use super::business_support::*;
use crate::{
    AgentEnvironment, AgentScope, AgentTool, AgentToolAccess, AgentToolError, AgentToolResult,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;
use wealthfolio_core::quotes::request_counts::RequestQuery;

business_tool!(
    ListProviderRequests, RequestQuery, "list_provider_requests",
    "Read local provider diagnostics retained for 30 days. HTTP records measure send-to-headers; SDK records measure the provider SDK call, including first-call initialization but excluding rate-limit waiting. Parameters are provider-selected business values only. Missing context/result means historical detail unavailable or an unfinished call. Never refreshes providers.",
    json!({"type":"object","additionalProperties":false,"properties":{
        "providerId":{"type":"string"},"symbol":{"type":"string"},
        "start":{"type":"string","format":"date-time"},"end":{"type":"string","format":"date-time"},
        "recordKind":{"type":"string","enum":["http","sdk"]},
        "outcome":{"type":"string","enum":["success","failure","unknown"]},
        "cursor":{"type":"string"},"limit":{"type":"integer","minimum":1,"maximum":200,"default":50}
    }}), [AgentScope::MarketDataRead], Read, |env, args| {
        output(env.provider_request_service().list_requests(args).await?)
    }
);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GetProviderRequestCountsArgs {
    days: Option<u32>,
    provider_id: Option<String>,
}

business_tool!(
    GetProviderRequestCounts, GetProviderRequestCountsArgs, "get_provider_request_counts",
    "Read local daily provider counts by HTTP/SDK record kind, using the configured provider statistics timezone. Defaults to 7 days; accepts 1-30. Success/failure/unknown describe the recorded layer: HTTP success does not guarantee business success. Records older than 30 days are deleted, and availableSince marks partial history. Does not refresh providers.",
    json!({"type":"object","additionalProperties":false,"properties":{
        "days":{"type":"integer","minimum":1,"maximum":30,"default":7},"providerId":{"type":"string"}
    }}), [AgentScope::MarketDataRead], Read, |env, args| {
        let days = args.days.unwrap_or(7);
        if !(1..=30).contains(&days) { return Err(AgentToolError::InvalidInput("days must be 1-30".into())); }
        let timezone = env.settings_service().get_settings()?.provider_request_timezone;
        let mut counts = env.provider_request_service().get_counts(days, &timezone).await?;
        if let Some(provider) = args.provider_id { counts.rows.retain(|row| row.provider_id == provider); }
        output(counts)
    }
);

pub fn tools() -> Vec<Arc<dyn AgentTool>> {
    vec![
        Arc::new(ListProviderRequests),
        Arc::new(GetProviderRequestCounts),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AgentScopeSet, AgentToolCatalog};

    #[test]
    fn diagnostics_are_read_only_mcp_tools_with_market_data_scope() {
        let mcp = AgentToolCatalog::mcp_catalog();
        let assistant = AgentToolCatalog::assistant_catalog();
        for name in ["list_provider_requests", "get_provider_request_counts"] {
            let tool = mcp.get(name).unwrap();
            assert_eq!(tool.access_level(), AgentToolAccess::Read);
            assert_eq!(tool.required_scopes(), &[AgentScope::MarketDataRead]);
            assert!(!AgentScopeSet::default().grants_all(tool.required_scopes()));
            assert!(assistant.get(name).is_none());
            assert_eq!(
                tool.sanitize_args_for_audit(&json!({"symbol":"private"})),
                Value::Null
            );
        }
    }
}
