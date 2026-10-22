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
struct GetHoldingArgs {
    pub account_id: String,
    pub asset_id: String,
}

business_tool!(
    GetHolding,
    GetHoldingArgs,
    "get_holding",
    "Read one account holding including its valuation and lot details.",
    json!({"type":"object","properties":{"accountId":{"type":"string"},"assetId":{"type":"string"}},"required":["accountId","assetId"]}),
    [AgentScope::HoldingsRead],
    Read,
    |env, args| {
        output(
            env.holdings_service()
                .get_holding(&args.account_id, &args.asset_id, &env.base_currency())
                .await?,
        )
    }
);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GetHoldingsListArgs {
    pub filter: wealthfolio_core::portfolios::AccountScope,
    #[serde(default)]
    pub include_closed: Option<bool>,
}

business_tool!(
    GetHoldingsList,
    GetHoldingsListArgs,
    "get_holdings_list",
    "Read detailed holdings for a reporting scope, optionally including closed positions.",
    json!({"type":"object","properties":{"filter":{"type":"object","properties":{"type":{"type":"string","enum":["all","account","portfolio","accounts"]},"accountId":{"type":"string"},"portfolioId":{"type":"string"},"accountIds":{"type":"array","items":{"type":"string"},"maxItems":100}},"required":["type"]},"includeClosed":{"type":"boolean"}},"required":["filter"]}),
    [AgentScope::HoldingsRead],
    Read,
    |env, args| {
        let scope = resolve_scope(&env, &args.filter)?;
        output(
            env.holdings_service()
                .get_holdings_for_accounts_with_options(
                    &scope.account_ids,
                    &env.base_currency(),
                    &scope.scope_id,
                    args.include_closed.unwrap_or(false),
                )
                .await?,
        )
    }
);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GetAssetHoldingsArgs {
    pub asset_id: String,
}

business_tool!(
    GetAssetHoldings,
    GetAssetHoldingsArgs,
    "get_asset_holdings",
    "Read holdings of an asset across the holdings-eligible account scope.",
    json!({"type":"object","properties":{"assetId":{"type":"string"}},"required":["assetId"]}),
    [AgentScope::HoldingsRead],
    Read,
    |env, args| {
        let scope = resolve_scope(&env, &wealthfolio_core::portfolios::AccountScope::All)?;
        let mut holdings = Vec::new();
        for account_id in scope.account_ids {
            if let Some(holding) = env
                .holdings_service()
                .get_holding(&account_id, &args.asset_id, &env.base_currency())
                .await?
            {
                holdings.push(holding);
            }
        }
        output(holdings)
    }
);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GetAssetLotsArgs {
    pub asset_id: String,
    #[serde(default)]
    pub include_snapshot_positions: Option<bool>,
}

business_tool!(GetAssetLots, GetAssetLotsArgs, "get_asset_lots",
    "Read transaction lots and optional holdings snapshot positions using the existing lot view service.",
    json!({"type":"object","properties":{"assetId":{"type":"string"},"includeSnapshotPositions":{"type":"boolean"}},"required":["assetId"]}), [AgentScope::HoldingsRead], Read, |env, args| {
        output(env.holdings_service().get_asset_lot_view(&args.asset_id, args.include_snapshot_positions.unwrap_or(false)).await?)
    });

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GetSnapshotsArgs {
    pub account_id: String,
    #[serde(default)]
    pub date_from: Option<chrono::NaiveDate>,
    #[serde(default)]
    pub date_to: Option<chrono::NaiveDate>,
}

business_tool!(
    GetSnapshots,
    GetSnapshotsArgs,
    "get_snapshots",
    "Read snapshot metadata without loading all positions.",
    json!({"type":"object","properties":{"accountId":{"type":"string"},"dateFrom":{"type":"string"},"dateTo":{"type":"string"}},"required":["accountId"]}),
    [AgentScope::HoldingsRead],
    Read,
    |env, args| {
        output(env.snapshot_service().get_snapshot_metadata(
            &args.account_id,
            args.date_from,
            args.date_to,
        )?)
    }
);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GetSnapshotByDateArgs {
    pub account_id: String,
    pub date: chrono::NaiveDate,
}

business_tool!(
    GetSnapshotByDate,
    GetSnapshotByDateArgs,
    "get_snapshot_by_date",
    "Read historical snapshot holdings without live valuation.",
    json!({"type":"object","properties":{"accountId":{"type":"string"},"date":{"type":"string"}},"required":["accountId","date"]}),
    [AgentScope::HoldingsRead],
    Read,
    |env, args| {
        let snapshots = env.snapshot_service().get_holdings_keyframes(
            &args.account_id,
            Some(args.date),
            Some(args.date),
        )?;
        let snapshot = snapshots
            .into_iter()
            .find(|s| s.snapshot_date == args.date)
            .ok_or_else(|| AgentToolError::NotFound("Snapshot not found".into()))?;
        output(
            env.holdings_service()
                .holdings_from_snapshot(&snapshot, &env.base_currency())
                .await?,
        )
    }
);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PrepareHoldingsImportArgs {
    pub input: super::holdings_support::SnapshotBatch,
}

business_tool!(PrepareHoldingsImport, PrepareHoldingsImportArgs, "prepare_holdings_import",
    "Preview snapshot validation, resolved symbols and existing dates without writing. Each supplied date replaces the complete holdings and cash snapshot: omitted positions are removed. Valuations update through existing events.",
    snapshot_schema(false), [AgentScope::HoldingsRead, AgentScope::HoldingsWrite], Draft, |env, args| {
        output(super::holdings_support::holdings_batch(&env, args.input, false, false).await?)
    });

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CommitHoldingsImportArgs {
    pub input: super::holdings_support::SnapshotBatch,
}

business_tool!(CommitHoldingsImport, CommitHoldingsImportArgs, "commit_holdings_import",
    "Save reviewed snapshots; use the same input as the preview. Revalidates dates, accounts and positions before writing. Results report each saved or failed date. Each supplied date replaces the complete holdings and cash snapshot: omitted positions are removed. Valuations update through existing events.",
    snapshot_schema(false), [AgentScope::HoldingsRead, AgentScope::HoldingsWrite], Write, |env, args| {
        output(super::holdings_support::holdings_batch(&env, args.input, true, false).await?)
    });

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PrepareManualHoldingsArgs {
    pub input: super::holdings_support::SnapshotBatch,
}

business_tool!(PrepareManualHoldings, PrepareManualHoldingsArgs, "prepare_manual_holdings",
    "Preview snapshot validation, resolved symbols and existing dates without writing. Provide exactly one snapshot. Each supplied date replaces the complete holdings and cash snapshot: omitted positions are removed. Valuations update through existing events.",
    snapshot_schema(true), [AgentScope::HoldingsRead, AgentScope::HoldingsWrite], Draft, |env, args| {
        output(super::holdings_support::holdings_batch(&env, args.input, false, true).await?)
    });

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CommitManualHoldingsArgs {
    pub input: super::holdings_support::SnapshotBatch,
}

business_tool!(CommitManualHoldings, CommitManualHoldingsArgs, "commit_manual_holdings",
    "Save reviewed snapshots; use the same input as the preview. Revalidates dates, accounts and positions before writing. Results report each saved or failed date. Provide exactly one snapshot. Each supplied date replaces the complete holdings and cash snapshot: omitted positions are removed. Valuations update through existing events.",
    snapshot_schema(true), [AgentScope::HoldingsRead, AgentScope::HoldingsWrite], Write, |env, args| {
        output(super::holdings_support::holdings_batch(&env, args.input, true, true).await?)
    });

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GetAlternativeHoldingsArgs {}

business_tool!(
    GetAlternativeHoldings,
    GetAlternativeHoldingsArgs,
    "get_alternative_holdings",
    "Read alternative assets and liabilities without changing their valuations.",
    json!({"type":"object","properties":{},"required":[]}),
    [AgentScope::HoldingsRead],
    Read,
    |env, args| {
        let _ = args;
        output(env.alternative_asset_service().get_alternative_holdings()?)
    }
);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GetLoanPaymentsArgs {
    pub asset_id: String,
}

business_tool!(
    GetLoanPayments,
    GetLoanPaymentsArgs,
    "get_loan_payments",
    "Read recorded payments associated with a loan.",
    json!({"type":"object","properties":{"assetId":{"type":"string"}},"required":["assetId"]}),
    [AgentScope::HoldingsRead],
    Read,
    |env, args| {
        output(
            env.alternative_asset_service()
                .get_loan_payments(&args.asset_id)?,
        )
    }
);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CalculateLoanArgs {
    pub request: wealthfolio_core::assets::loan::LoanCalculationRequest,
}

business_tool!(
    CalculateLoan,
    CalculateLoanArgs,
    "calculate_loan",
    "Calculate a loan schedule from supplied terms and recorded balances; never modifies a loan.",
    json!({"type":"object","properties":{"request":{"type":"object","properties":{"metadata":{"type":"object","description":"Existing loan asset metadata, including loan_projection and loan_events; accepted in the same format as get_asset_profile."},"balances":{"type":"array","items":{"type":"object","properties":{"date":{"type":"string"},"balance":{"type":"number"},"notes":{"type":"string"}},"required":["date","balance"]},"maxItems":100},"asOf":{"type":"string"},"payments":{"type":"array","items":{"type":"object","properties":{"activityId":{"type":"string"},"accountId":{"type":"string"},"date":{"type":"string"},"amount":{"type":"number"},"escrow":{"type":"number"},"appliesTo":{"type":"string","description":"extra for an extra repayment, or the instalment date as YYYY-MM-DD"}},"required":["activityId","date","amount"]},"maxItems":100}},"required":["metadata","balances","asOf"]}},"required":["request"]}),
    [AgentScope::FinancialPlanningRead],
    Read,
    |env, args| {
        let _ = env;
        output(wealthfolio_core::assets::loan::calculate_loan(
            &args.request,
        ))
    }
);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RecalculateLoanArgs {
    pub request: wealthfolio_core::assets::loan::LoanRecalculationRequest,
}

business_tool!(
    RecalculateLoan,
    RecalculateLoanArgs,
    "recalculate_loan",
    "Solve a new loan payment from supplied balances and annual rate. This is a calculation only.",
    json!({"type":"object","properties":{"request":{"type":"object","properties":{"metadata":{"type":"object","description":"Existing loan asset metadata, including loan_projection and loan_events; accepted in the same format as get_asset_profile."},"balances":{"type":"array","items":{"type":"object","properties":{"date":{"type":"string"},"balance":{"type":"number"},"notes":{"type":"string"}},"required":["date","balance"]},"maxItems":100},"asOf":{"type":"string"},"payments":{"type":"array","items":{"type":"object","properties":{"activityId":{"type":"string"},"accountId":{"type":"string"},"date":{"type":"string"},"amount":{"type":"number"},"escrow":{"type":"number"},"appliesTo":{"type":"string","description":"extra for an extra repayment, or the instalment date as YYYY-MM-DD"}},"required":["activityId","date","amount"]},"maxItems":100},"annualRate":{"type":"number"}},"required":["metadata","balances","asOf","annualRate"]}},"required":["request"]}),
    [AgentScope::FinancialPlanningRead],
    Read,
    |env, args| {
        let _ = env;
        output(wealthfolio_core::assets::loan::recalculate_loan(
            &args.request,
        ))
    }
);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PreviewLoanTermsArgs {
    #[serde(default)]
    pub asset_id: Option<String>,
    pub setup: wealthfolio_core::assets::loan::LoanSetup,
}

business_tool!(
    PreviewLoanTerms,
    PreviewLoanTermsArgs,
    "preview_loan_terms",
    "Preview loan terms using existing service validation; does not save loan details.",
    json!({"type":"object","properties":{"assetId":{"type":"string"},"setup":{"type":"object","properties":{"originalAmount":{"type":"number"},"originationDate":{"type":"string"},"interestRate":{"type":"number"},"schedule":{"type":"object","properties":{"firstPaymentDate":{"type":"string"},"lastPaymentDate":{"type":"string"},"renewalMaturity":{"type":"string"},"paymentAccountId":{"type":"string"},"paymentAmount":{"type":"number"},"escrowAmount":{"type":"number"},"amortizationMonths":{"type":"integer"},"frequency":{"type":"string","enum":["monthly","biweekly","accelerated_biweekly"]},"interestMethod":{"type":"string","enum":["nominal_periodic","monthly","semiannual"]}},"required":["frequency"]}},"required":[]}},"required":["setup"]}),
    [AgentScope::FinancialPlanningRead, AgentScope::HoldingsRead],
    Read,
    |env, args| {
        output(
            env.alternative_asset_service()
                .preview_loan_terms(args.asset_id.as_deref(), &args.setup)?,
        )
    }
);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CalculateDepositsForLimitArgs {
    pub limit_id: String,
}

business_tool!(CalculateDepositsForLimit, CalculateDepositsForLimitArgs, "calculate_deposits_for_limit",
    "Calculate deposits counted against an existing contribution limit, without modifying the limit.",
    json!({"type":"object","properties":{"limitId":{"type":"string"}},"required":["limitId"]}), [AgentScope::FinancialPlanningRead], Read, |env, args| {
        output(env.contribution_limit_service().calculate_deposits_for_contribution_limit(&args.limit_id, &env.base_currency())?)
    });

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GetCurrentValuationArgs {
    pub filter: wealthfolio_core::portfolios::AccountScope,
    #[serde(default)]
    pub include_accounts: Option<bool>,
}

business_tool!(GetCurrentValuation, GetCurrentValuationArgs, "get_current_valuation",
    "Read live current valuation for an account scope using existing local snapshots, quotes and FX. Does not refresh providers.",
    json!({"type":"object","properties":{"filter":{"type":"object","properties":{"type":{"type":"string","enum":["all","account","portfolio","accounts"]},"accountId":{"type":"string"},"portfolioId":{"type":"string"},"accountIds":{"type":"array","items":{"type":"string"},"maxItems":100}},"required":["type"]},"includeAccounts":{"type":"boolean"}},"required":["filter"]}), [AgentScope::HoldingsRead], Read, |env, args| {
        let scope = resolve_scope(&env, &args.filter)?;
        let accounts = env.account_service(); let snapshots = env.snapshot_service(); let assets = env.asset_service(); let quotes = env.quote_service(); let fx = env.fx_service();
        let service = wealthfolio_core::portfolio::valuation::CurrentAccountValuationService::new(accounts.as_ref(), snapshots.as_ref(), assets.as_ref(), quotes.as_ref(), fx.as_ref());
        let cutoff = wealthfolio_core::utils::time_utils::user_today(wealthfolio_core::utils::time_utils::parse_user_timezone_or_default(&timezone(&env)?));
        output(service.get_current_valuation_for_scope(&scope.scope_id, &scope.account_ids, &env.base_currency(), cutoff, args.include_accounts.unwrap_or(false)).await?)
    });

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GetHistoricalValuationsArgs {
    pub filter: wealthfolio_core::portfolios::AccountScope,
    #[serde(default)]
    pub start_date: Option<chrono::NaiveDate>,
    #[serde(default)]
    pub end_date: Option<chrono::NaiveDate>,
}

business_tool!(GetHistoricalValuations, GetHistoricalValuationsArgs, "get_historical_valuations",
    "Read full historical valuation records for a scope. Uses the existing service worker; no provider refresh.",
    json!({"type":"object","properties":{"filter":{"type":"object","properties":{"type":{"type":"string","enum":["all","account","portfolio","accounts"]},"accountId":{"type":"string"},"portfolioId":{"type":"string"},"accountIds":{"type":"array","items":{"type":"string"},"maxItems":100}},"required":["type"]},"startDate":{"type":"string"},"endDate":{"type":"string"}},"required":["filter"]}), [AgentScope::HoldingsRead], Read, |env, args| {
        let scope = resolve_scope(&env, &args.filter)?;
        output(env.valuation_service().get_historical_valuations_for_accounts(&scope.scope_id, &scope.account_ids, &env.base_currency(), args.start_date, args.end_date).await?)
    });

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GetLatestValuationsArgs {
    pub account_ids: Vec<String>,
}

business_tool!(
    GetLatestValuations,
    GetLatestValuationsArgs,
    "get_latest_valuations",
    "Read the latest stored valuations for selected accounts.",
    json!({"type":"object","properties":{"accountIds":{"type":"array","items":{"type":"string"},"maxItems":100}},"required":["accountIds"]}),
    [AgentScope::HoldingsRead],
    Read,
    |env, args| {
        if args.account_ids.len() > 100 {
            return Err(AgentToolError::InvalidInput(
                "At most 100 items per call".into(),
            ));
        }
        output(
            env.valuation_service()
                .get_latest_valuations(&args.account_ids)?,
        )
    }
);

pub fn tools() -> Vec<Arc<dyn AgentTool>> {
    vec![
        Arc::new(GetHolding),
        Arc::new(GetHoldingsList),
        Arc::new(GetAssetHoldings),
        Arc::new(GetAssetLots),
        Arc::new(GetSnapshots),
        Arc::new(GetSnapshotByDate),
        Arc::new(PrepareHoldingsImport),
        Arc::new(CommitHoldingsImport),
        Arc::new(PrepareManualHoldings),
        Arc::new(CommitManualHoldings),
        Arc::new(GetAlternativeHoldings),
        Arc::new(GetLoanPayments),
        Arc::new(CalculateLoan),
        Arc::new(RecalculateLoan),
        Arc::new(PreviewLoanTerms),
        Arc::new(CalculateDepositsForLimit),
        Arc::new(GetCurrentValuation),
        Arc::new(GetHistoricalValuations),
        Arc::new(GetLatestValuations),
    ]
}

fn snapshot_schema(manual: bool) -> Value {
    let mut schema = json!({
        "type": "object",
        "properties": {
            "accountId": {
                "type": "string"
            },
            "snapshots": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "date": {
                            "type": "string"
                        },
                        "positions": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "assetId": {
                                        "type": "string"
                                    },
                                    "symbol": {
                                        "type": "string"
                                    },
                                    "quantity": {
                                        "type": "string"
                                    },
                                    "currency": {
                                        "type": "string"
                                    },
                                    "avgCost": {
                                        "type": "string"
                                    },
                                    "exchangeMic": {
                                        "type": "string"
                                    },
                                    "quoteCcy": {
                                        "type": "string"
                                    },
                                    "instrumentType": {
                                        "type": "string"
                                    },
                                    "quoteMode": {
                                        "type": "string",
                                        "enum": [
                                            "MARKET",
                                            "MANUAL"
                                        ]
                                    },
                                    "providerId": {
                                        "type": "string"
                                    },
                                    "providerSymbol": {
                                        "type": "string"
                                    },
                                    "name": {
                                        "type": "string"
                                    },
                                    "assetKind": {
                                        "type": "string"
                                    }
                                },
                                "required": [
                                    "symbol",
                                    "quantity",
                                    "currency"
                                ]
                            },
                            "maxItems": 1000
                        },
                        "cashBalances": {
                            "type": "object",
                            "additionalProperties": {
                                "type": "string"
                            }
                        }
                    },
                    "required": [
                        "date",
                        "positions",
                        "cashBalances"
                    ]
                },
                "maxItems": 100,
                "minItems": 1
            }
        },
        "required": [
            "accountId",
            "snapshots"
        ]
    });
    if manual {
        schema["properties"]["snapshots"]["maxItems"] = json!(1);
    }
    json!({"type":"object","properties":{"input": schema},"required":["input"]})
}
