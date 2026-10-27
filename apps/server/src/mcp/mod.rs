//! Server-mode MCP endpoint (`/mcp`), authenticated with Personal Access
//! Tokens. The rmcp service is mounted outside the `/api/v1` subtree (and
//! outside its request timeout, which would kill SSE streams) behind the
//! PAT middleware in [`auth`].

pub mod audit_sink;
pub mod auth;

use std::sync::Arc;
use std::time::Duration;

use axum::{middleware, Router};
use rmcp::transport::streamable_http_server::StreamableHttpServerConfig;
use wealthfolio_mcp::McpServerBuilder;

use crate::{config::Config, main_lib::AppState};

const INSTRUCTIONS: &str = "Scoped access to Wealthfolio investment and spending business data: \
accounts, portfolios, assets, holdings, valuations, activities, goals, allocations, rebalancing, \
planning calculations, market data, classifications and budgets. Available tools depend on \
the access token's permissions. Preview activity, quote and holdings imports before committing \
the reviewed input. Rebalance calculations do not record trades. Provider diagnostics expose local business parameters and HTTP/SDK outcomes for 30 days. System operations, provider \
refresh/configuration, exports and API Key permission management are not exposed.";

/// Builds the `/mcp` router: the Streamable HTTP MCP service behind PAT
/// bearer authentication.
pub fn router(state: Arc<AppState>, config: &Config) -> Router {
    let http_config =
        StreamableHttpServerConfig::default().with_sse_keep_alive(Some(Duration::from_secs(30)));
    // Host header validation: explicit allowlist when configured, disabled
    // otherwise (rmcp's default is loopback-only, which breaks reverse
    // proxy deployments; PAT bearer auth is the security boundary here —
    // see `Config::mcp_allowed_hosts`).
    let http_config = match &config.mcp_allowed_hosts {
        Some(hosts) => http_config.with_allowed_hosts(hosts.clone()),
        None => http_config.disable_allowed_hosts(),
    };

    let mut builder =
        McpServerBuilder::new(state.agent_environment.clone()).instructions(INSTRUCTIONS);
    // Audit logging is opt-out via WF_MCP_AUDIT_ENABLED=false.
    if config.mcp_audit_enabled {
        builder = builder.audit(Arc::new(audit_sink::RepoAuditSink(
            state.mcp_audit_repository.clone(),
        )));
    }
    let handler = builder.build_handler();
    let service = rmcp::transport::streamable_http_server::StreamableHttpService::new(
        move || Ok(handler.clone()),
        state.mcp_sessions.clone(),
        http_config,
    );

    Router::new()
        .nest_service("/mcp", service)
        .layer(middleware::from_fn_with_state(
            auth::PatAuthState::new(state.pat_repository.clone()),
            auth::require_pat,
        ))
}
