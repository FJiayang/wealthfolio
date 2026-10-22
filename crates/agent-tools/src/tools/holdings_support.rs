//! Shared preview/commit path for manual holdings and structured snapshot imports.
use super::business_support::timezone;
use crate::{AgentEnvironment, AgentToolError};
use rust_decimal::Decimal;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{collections::HashMap, sync::Arc};
use wealthfolio_core::{
    accounts::TrackingMode,
    portfolio::snapshot::{
        check_holdings_import, holdings_import_data_source, validate_holdings_import_snapshot,
        CashBalanceInput, HoldingsImportPositionValidationInput,
        HoldingsImportSnapshotValidationInput, ManualHoldingInput, ManualSnapshotRequest,
        ManualSnapshotService, SnapshotSource,
    },
    utils::time_utils::{parse_user_timezone_or_default, user_today},
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SnapshotBatch {
    pub account_id: String,
    pub snapshots: Vec<SnapshotInput>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SnapshotInput {
    pub date: String,
    pub positions: Vec<PositionInput>,
    pub cash_balances: HashMap<String, String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PositionInput {
    pub asset_id: Option<String>,
    pub symbol: String,
    pub quantity: String,
    pub currency: String,
    pub avg_cost: Option<String>,
    pub exchange_mic: Option<String>,
    pub quote_ccy: Option<String>,
    pub instrument_type: Option<String>,
    pub quote_mode: Option<String>,
    pub provider_id: Option<String>,
    pub provider_symbol: Option<String>,
    pub name: Option<String>,
    pub asset_kind: Option<String>,
}

impl SnapshotInput {
    fn validation_input(&self) -> HoldingsImportSnapshotValidationInput {
        HoldingsImportSnapshotValidationInput {
            date: self.date.clone(),
            cash_balances: self
                .cash_balances
                .iter()
                .map(|(c, a)| (c.clone(), a.clone()))
                .collect(),
            positions: self
                .positions
                .iter()
                .map(|p| HoldingsImportPositionValidationInput {
                    symbol: p.symbol.clone(),
                    quantity: p.quantity.clone(),
                    currency: p.currency.clone(),
                    avg_cost: p.avg_cost.clone(),
                    exchange_mic: p.exchange_mic.clone(),
                    quote_ccy: p.quote_ccy.clone(),
                    instrument_type: p.instrument_type.clone(),
                    quote_mode: p.quote_mode.clone(),
                    provider_id: p.provider_id.clone(),
                    provider_symbol: p.provider_symbol.clone(),
                    asset_id: p.asset_id.clone(),
                })
                .collect(),
        }
    }
}

pub async fn holdings_batch(
    env: &Arc<dyn AgentEnvironment>,
    batch: SnapshotBatch,
    commit: bool,
    manual: bool,
) -> Result<Value, AgentToolError> {
    if batch.snapshots.is_empty()
        || batch.snapshots.len() > 100
        || (manual && batch.snapshots.len() != 1)
    {
        return Err(AgentToolError::InvalidInput(
            "Provide 1 to 100 snapshots (exactly one for manual holdings)".into(),
        ));
    }
    if batch
        .snapshots
        .iter()
        .any(|s| s.positions.len() > 1000 || s.cash_balances.len() > 100)
    {
        return Err(AgentToolError::InvalidInput(
            "A snapshot supports up to 1000 positions and 100 cash currencies".into(),
        ));
    }
    let account = env.account_service().get_account(&batch.account_id)?;
    if account.tracking_mode != TrackingMode::Holdings {
        return Err(AgentToolError::InvalidInput(
            "Manual snapshots require a HOLDINGS account".into(),
        ));
    }
    for position in batch
        .snapshots
        .iter()
        .flat_map(|snapshot| &snapshot.positions)
    {
        if position
            .asset_kind
            .as_deref()
            .is_some_and(|kind| !matches!(kind, "OTHER" | "INVESTMENT"))
        {
            return Err(AgentToolError::InvalidInput(
                "Only investment holdings can be saved".into(),
            ));
        }
        if let Some(asset_id) = position.asset_id.as_deref() {
            if env.asset_service().get_asset_profile(asset_id)?.asset.kind
                != wealthfolio_core::assets::AssetKind::Investment
            {
                return Err(AgentToolError::InvalidInput(
                    "Alternative asset writes are excluded".into(),
                ));
            }
        }
    }
    let timezone = timezone(env)?;
    let today = user_today(parse_user_timezone_or_default(&timezone));
    let validation: Vec<_> = batch
        .snapshots
        .iter()
        .map(SnapshotInput::validation_input)
        .collect();
    let checked = check_holdings_import(
        env.asset_service().as_ref(),
        env.snapshot_service().as_ref(),
        &batch.account_id,
        &account.currency,
        today,
        &validation,
    )
    .await?;
    if !commit {
        let mut existing_snapshots = Vec::new();
        for date in &checked.existing_dates {
            let date = chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d")
                .expect("existing_snapshot_dates formats validated ISO dates");
            existing_snapshots.extend(env.snapshot_service().get_holdings_keyframes(
                &batch.account_id,
                Some(date),
                Some(date),
            )?);
        }
        let symbols: Vec<_> = checked.symbols.into_iter().map(|s| json!({
            "symbol": s.symbol, "found": s.found, "assetName": s.asset_name, "assetId": s.asset_id,
            "currency": s.currency, "exchangeMic": s.exchange_mic,
        })).collect();
        return Ok(
            json!({"existingDates": checked.existing_dates, "existingSnapshots": existing_snapshots, "symbols": symbols,
            "validationErrors": checked.validation_errors, "validSnapshotDates": checked.valid_snapshot_dates,
            "invalidSnapshotDates": checked.invalid_snapshot_dates,
            "replacement": "Each supplied date replaces its complete holdings and cash snapshot; omitted positions are removed."}),
        );
    }
    let service = ManualSnapshotService::new(
        env.asset_service(),
        env.fx_service(),
        env.snapshot_service(),
        env.quote_service(),
    )
    .with_timezone(timezone);
    let mut results = Vec::new();
    for snapshot in &batch.snapshots {
        let result = async {
            let date = validate_holdings_import_snapshot(
                &batch.account_id,
                today,
                &snapshot.validation_input(),
            )
            .map_err(|errors| AgentToolError::InvalidInput(errors.join(" ")))?;
            let positions = snapshot
                .positions
                .iter()
                .map(|p| {
                    let quantity = p
                        .quantity
                        .parse::<Decimal>()
                        .map_err(|_| AgentToolError::InvalidInput("Invalid quantity".into()))?;
                    let average_cost = p
                        .avg_cost
                        .as_deref()
                        .filter(|v| !v.is_empty())
                        .map(str::parse::<Decimal>)
                        .transpose()
                        .map_err(|_| AgentToolError::InvalidInput("Invalid average cost".into()))?
                        .unwrap_or(Decimal::ZERO);
                    Ok(ManualHoldingInput {
                        asset_id: p.asset_id.clone(),
                        symbol: p.symbol.clone(),
                        exchange_mic: p.exchange_mic.clone(),
                        quantity,
                        currency: p.currency.clone(),
                        average_cost,
                        name: p.name.clone(),
                        data_source: holdings_import_data_source(p.quote_mode.as_deref()),
                        asset_kind: p.asset_kind.clone(),
                        quote_ccy: p.quote_ccy.clone(),
                        instrument_type: p.instrument_type.clone(),
                        provider_id: p.provider_id.clone(),
                        provider_symbol: p.provider_symbol.clone(),
                    })
                })
                .collect::<Result<Vec<_>, AgentToolError>>()?;
            let cash_balances = snapshot
                .cash_balances
                .iter()
                .map(|(currency, amount)| {
                    let amount = amount
                        .parse()
                        .map_err(|_| AgentToolError::InvalidInput("Invalid cash amount".into()))?;
                    Ok(CashBalanceInput {
                        currency: currency.clone(),
                        amount,
                    })
                })
                .collect::<Result<Vec<_>, AgentToolError>>()?;
            let ids = service
                .save_manual_snapshot(ManualSnapshotRequest {
                    account_id: batch.account_id.clone(),
                    account_currency: account.currency.clone(),
                    snapshot_date: date,
                    positions,
                    cash_balances,
                    base_currency: Some(env.base_currency()),
                    source: if manual {
                        SnapshotSource::ManualEntry
                    } else {
                        SnapshotSource::CsvImport
                    },
                })
                .await?;
            Ok::<_, AgentToolError>(ids)
        }
        .await;
        results.push(match result {
            Ok(ids) => json!({"date": snapshot.date, "saved": true, "assetIds": ids}),
            Err(error) => {
                json!({"date": snapshot.date, "saved": false, "error": error.to_string()})
            }
        });
    }
    Ok(json!({"snapshots": results, "valuationsUpdateInBackground": true}))
}
