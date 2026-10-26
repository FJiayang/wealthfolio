use crate::{errors::StorageError, schema::trade_instructions};
use diesel::prelude::*;
use wealthfolio_core::trade_instructions::*;
#[derive(Queryable, Selectable, Insertable)]
#[diesel(table_name = trade_instructions)]
pub struct InstructionDb {
    pub id: String,
    pub request_id: String,
    pub request_hash: String,
    pub account_id: String,
    pub asset_id: String,
    pub account_name: String,
    pub asset_snapshot: String,
    pub side: String,
    pub quantity: Option<String>,
    pub amount: Option<String>,
    pub amount_currency: Option<String>,
    pub limit_price: Option<String>,
    pub rationale: Option<String>,
    pub expected_completion_at: Option<String>,
    pub status: String,
    pub resolution_note: Option<String>,
    pub source: String,
    pub created_at: String,
    pub updated_at: String,
    pub started_at: Option<String>,
    pub closed_at: Option<String>,
    pub completed_at: Option<String>,
    pub version: i32,
}
fn decode<T: serde::de::DeserializeOwned>(value: String) -> Result<T, StorageError> {
    serde_json::from_value(serde_json::Value::String(value))
        .map_err(|e| StorageError::SerializationError(e.to_string()))
}
impl TryFrom<TradeInstruction> for InstructionDb {
    type Error = StorageError;
    fn try_from(row: TradeInstruction) -> Result<Self, Self::Error> {
        Ok(Self {
            id: row.id,
            request_id: row.request_id,
            request_hash: row.request_hash,
            account_id: row.account_id,
            asset_id: row.asset_id,
            account_name: row.account_name,
            asset_snapshot: serde_json::to_string(&row.asset_snapshot)
                .map_err(|e| StorageError::SerializationError(e.to_string()))?,
            side: match row.side {
                InstructionSide::Buy => "BUY",
                InstructionSide::Sell => "SELL",
            }
            .into(),
            quantity: row.quantity.map(|v| v.to_string()),
            amount: row.amount.map(|v| v.to_string()),
            amount_currency: row.amount_currency,
            limit_price: row.limit_price.map(|v| v.to_string()),
            rationale: row.rationale,
            expected_completion_at: row.expected_completion_at.map(|v| v.to_rfc3339()),
            status: row.status.as_str().into(),
            resolution_note: row.resolution_note,
            source: match row.source {
                InstructionSource::Ui => "UI",
                InstructionSource::Mcp => "MCP",
            }
            .into(),
            created_at: row.created_at.to_rfc3339(),
            updated_at: row.updated_at.to_rfc3339(),
            started_at: row.started_at.map(|v| v.to_rfc3339()),
            closed_at: row.closed_at.map(|v| v.to_rfc3339()),
            completed_at: row.completed_at.map(|v| v.to_rfc3339()),
            version: row.version,
        })
    }
}
impl TryFrom<InstructionDb> for TradeInstruction {
    type Error = StorageError;
    fn try_from(row: InstructionDb) -> Result<Self, Self::Error> {
        Ok(Self {
            id: row.id,
            request_id: row.request_id,
            request_hash: row.request_hash,
            account_id: row.account_id,
            asset_id: row.asset_id,
            account_name: row.account_name,
            asset_snapshot: serde_json::from_str(&row.asset_snapshot)
                .map_err(|e| StorageError::SerializationError(e.to_string()))?,
            side: decode(row.side)?,
            quantity: row.quantity.map(decode).transpose()?,
            amount: row.amount.map(decode).transpose()?,
            amount_currency: row.amount_currency,
            limit_price: row.limit_price.map(decode).transpose()?,
            rationale: row.rationale,
            expected_completion_at: row.expected_completion_at.map(decode).transpose()?,
            status: decode(row.status)?,
            resolution_note: row.resolution_note,
            source: decode(row.source)?,
            created_at: decode(row.created_at)?,
            updated_at: decode(row.updated_at)?,
            started_at: row.started_at.map(decode).transpose()?,
            closed_at: row.closed_at.map(decode).transpose()?,
            completed_at: row.completed_at.map(decode).transpose()?,
            version: row.version,
        })
    }
}
