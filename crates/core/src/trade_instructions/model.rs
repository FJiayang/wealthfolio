use crate::activities::AssetResolutionInput;
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum InstructionStatus {
    Pending,
    InProgress,
    Completed,
    Cancelled,
}
impl InstructionStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "PENDING",
            Self::InProgress => "IN_PROGRESS",
            Self::Completed => "COMPLETED",
            Self::Cancelled => "CANCELLED",
        }
    }
    pub fn is_open(self) -> bool {
        matches!(self, Self::Pending | Self::InProgress)
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum InstructionSide {
    Buy,
    Sell,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum InstructionSource {
    Ui,
    Mcp,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NewTradeInstruction {
    pub goal_id: String,
    pub request_id: String,
    pub account_id: String,
    pub asset: AssetResolutionInput,
    pub side: InstructionSide,
    #[serde(default, with = "rust_decimal::serde::str_option")]
    pub quantity: Option<Decimal>,
    #[serde(default, with = "rust_decimal::serde::str_option")]
    pub amount: Option<Decimal>,
    pub amount_currency: Option<String>,
    #[serde(default, with = "rust_decimal::serde::str_option")]
    pub limit_price: Option<Decimal>,
    pub notes: Option<String>,
    pub expected_completion_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstructionAssetSnapshot {
    pub name: Option<String>,
    pub symbol: Option<String>,
    pub exchange_mic: Option<String>,
    pub instrument_type: Option<String>,
    pub quote_ccy: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TradeInstruction {
    pub goal_id: String,
    pub id: String,
    pub request_id: String,
    #[serde(skip_serializing, default)]
    pub request_hash: String,
    pub account_id: String,
    pub asset_id: String,
    pub account_name: String,
    pub asset_snapshot: InstructionAssetSnapshot,
    pub side: InstructionSide,
    #[serde(with = "rust_decimal::serde::str_option")]
    pub quantity: Option<Decimal>,
    #[serde(with = "rust_decimal::serde::str_option")]
    pub amount: Option<Decimal>,
    pub amount_currency: Option<String>,
    #[serde(with = "rust_decimal::serde::str_option")]
    pub limit_price: Option<Decimal>,
    pub notes: Option<String>,
    pub expected_completion_at: Option<DateTime<Utc>>,
    pub status: InstructionStatus,
    pub resolution_note: Option<String>,
    pub source: InstructionSource,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub closed_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub version: i32,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum InstructionSort {
    ExpectedCompletionAsc,
    ExpectedCompletionDesc,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InstructionFilter {
    pub goal_id: Option<String>,
    /// Omitted: open instructions; empty: all statuses.
    pub statuses: Option<Vec<InstructionStatus>>,
    pub account_id: Option<String>,
    pub side: Option<InstructionSide>,
    pub expected_completion_from: Option<DateTime<Utc>>,
    pub expected_completion_before: Option<DateTime<Utc>>,
    pub sort: Option<InstructionSort>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstructionPage {
    pub items: Vec<TradeInstruction>,
    pub total: i64,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InstructionVersion {
    pub expected_version: i32,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CompleteInstruction {
    pub expected_version: i32,
    pub resolution_note: String,
    pub completed_at: Option<DateTime<Utc>>,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CancelInstruction {
    pub expected_version: i32,
    pub resolution_note: String,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CorrectCompletionTime {
    pub expected_version: i32,
    pub completed_at: DateTime<Utc>,
}
