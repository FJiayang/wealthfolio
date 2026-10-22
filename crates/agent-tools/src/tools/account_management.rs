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
struct CreateAccountArgs {
    pub input: wealthfolio_core::accounts::NewAccount,
}

business_tool!(CreateAccount, CreateAccountArgs, "create_account",
    "Save an account using existing validation. Broker connection identity cannot be changed. Updating isArchived archives or restores the account.",
    json!({"type":"object","properties":{"input":{"type":"object","properties":{"name":{"type":"string"},"accountType":{"type":"string"},"group":{"type":"string"},"currency":{"type":"string"},"isDefault":{"type":"boolean"},"isActive":{"type":"boolean"},"platformId":{"type":"string"},"accountNumber":{"type":"string"},"meta":{"type":"string"},"isArchived":{"type":"boolean"},"trackingMode":{"type":"string","enum":["HOLDINGS","TRANSACTIONS"]}},"required":["name","accountType","currency","isDefault","isActive"]}},"required":["input"]}), [AgentScope::AccountsWrite], Write, |env, args| {
        validate_account_connection(&args.input.provider, &args.input.provider_account_id)?; output(env.account_service().create_account(args.input).await?)
    });

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct UpdateAccountArgs {
    pub input: wealthfolio_core::accounts::AccountUpdate,
}

business_tool!(UpdateAccount, UpdateAccountArgs, "update_account",
    "Save an account using existing validation. Broker connection identity cannot be changed. Updating isArchived archives or restores the account.",
    json!({"type":"object","properties":{"input":{"type":"object","properties":{"name":{"type":"string"},"accountType":{"type":"string"},"group":{"type":"string"},"isDefault":{"type":"boolean"},"isActive":{"type":"boolean"},"platformId":{"type":"string"},"accountNumber":{"type":"string"},"meta":{"type":"string"},"isArchived":{"type":"boolean"},"trackingMode":{"type":"string","enum":["HOLDINGS","TRANSACTIONS"]},"id":{"type":"string"}},"required":["id","name","accountType","isDefault","isActive"]}},"required":["input"]}), [AgentScope::AccountsWrite], Write, |env, args| {
        let mut input = args.input;
        let id = input.id.as_deref().ok_or_else(|| AgentToolError::InvalidInput("Account ID is required".into()))?;
        let existing = env.account_service().get_account(id)?;
        if (input.provider.is_some() && input.provider != existing.provider) || (input.provider_account_id.is_some() && input.provider_account_id != existing.provider_account_id) {
            return Err(AgentToolError::InvalidInput("Broker connection identity cannot be changed".into()));
        }
        input.provider = existing.provider; input.provider_account_id = existing.provider_account_id;
        output(env.account_service().update_account(input).await?)
    });

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GetAccountArgs {
    pub account_id: String,
}

business_tool!(
    GetAccount,
    GetAccountArgs,
    "get_account",
    "Get complete account details, including archived accounts.",
    json!({"type":"object","properties":{"accountId":{"type":"string"}},"required":["accountId"]}),
    [AgentScope::AccountsRead],
    Read,
    |env, args| { output(env.account_service().get_account(&args.account_id)?) }
);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ListAccountsArgs {
    #[serde(default)]
    pub is_active: Option<bool>,
    #[serde(default)]
    pub is_archived: Option<bool>,
}

business_tool!(
    ListAccounts,
    ListAccountsArgs,
    "list_accounts",
    "List complete accounts with optional active and archived filters.",
    json!({"type":"object","properties":{"isActive":{"type":"boolean"},"isArchived":{"type":"boolean"}},"required":[]}),
    [AgentScope::AccountsRead],
    Read,
    |env, args| {
        output(
            env.account_service()
                .list_accounts(args.is_active, args.is_archived, None)?,
        )
    }
);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CreatePortfolioArgs {
    pub input: wealthfolio_core::portfolios::NewPortfolio,
}

business_tool!(CreatePortfolio, CreatePortfolioArgs, "create_portfolio",
    "Save a named portfolio and its complete account membership. Omitted account memberships are removed; accounts and activities are not deleted.",
    json!({"type":"object","properties":{"input":{"type":"object","properties":{"name":{"type":"string"},"description":{"type":"string"},"sortOrder":{"type":"integer"},"accountIds":{"type":"array","items":{"type":"string"},"maxItems":100}},"required":["name","accountIds"]}},"required":["input"]}), [AgentScope::AccountsWrite], Write, |env, args| {
        output(env.portfolio_service().create_portfolio(args.input).await?)
    });

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct UpdatePortfolioArgs {
    pub input: wealthfolio_core::portfolios::PortfolioUpdate,
}

business_tool!(UpdatePortfolio, UpdatePortfolioArgs, "update_portfolio_entry",
    "Save a named portfolio and its complete account membership. Omitted account memberships are removed; accounts and activities are not deleted.",
    json!({"type":"object","properties":{"input":{"type":"object","properties":{"name":{"type":"string"},"description":{"type":"string"},"sortOrder":{"type":"integer"},"accountIds":{"type":"array","items":{"type":"string"},"maxItems":100},"id":{"type":"string"}},"required":["id","name","accountIds"]}},"required":["input"]}), [AgentScope::AccountsWrite], Write, |env, args| {
        output(env.portfolio_service().update_portfolio(args.input).await?)
    });

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GetPortfolioArgs {
    pub portfolio_id: String,
}

business_tool!(
    GetPortfolio,
    GetPortfolioArgs,
    "get_portfolio",
    "Get a named portfolio with all member account IDs.",
    json!({"type":"object","properties":{"portfolioId":{"type":"string"}},"required":["portfolioId"]}),
    [AgentScope::AccountsRead],
    Read,
    |env, args| { output(env.portfolio_service().get_portfolio(&args.portfolio_id)?) }
);

pub fn tools() -> Vec<Arc<dyn AgentTool>> {
    vec![
        Arc::new(CreateAccount),
        Arc::new(UpdateAccount),
        Arc::new(GetAccount),
        Arc::new(ListAccounts),
        Arc::new(CreatePortfolio),
        Arc::new(UpdatePortfolio),
        Arc::new(GetPortfolio),
    ]
}
