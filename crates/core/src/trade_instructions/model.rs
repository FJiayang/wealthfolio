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
    Transfer,
    Deposit,
    Withdrawal,
    Conversion,
}
impl InstructionSide {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Buy => "BUY",
            Self::Sell => "SELL",
            Self::Transfer => "TRANSFER",
            Self::Deposit => "DEPOSIT",
            Self::Withdrawal => "WITHDRAWAL",
            Self::Conversion => "CONVERSION",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InstructionTransferMode {
    Cash,
    Securities,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InstructionTransferDirection {
    In,
    Out,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InstructionTransferInput {
    pub mode: InstructionTransferMode,
    pub is_external: bool,
    pub direction: Option<InstructionTransferDirection>,
    pub to_account_id: Option<String>,
    pub destination_currency: Option<String>,
    #[serde(default, with = "rust_decimal::serde::str_option")]
    pub destination_amount: Option<Decimal>,
    #[serde(default, with = "rust_decimal::serde::str_option")]
    pub unit_price: Option<Decimal>,
    pub cost_currency: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstructionTransfer {
    pub mode: InstructionTransferMode,
    pub is_external: bool,
    pub direction: Option<InstructionTransferDirection>,
    pub to_account_id: Option<String>,
    pub to_account_name: Option<String>,
    pub destination_currency: Option<String>,
    #[serde(with = "rust_decimal::serde::str_option")]
    pub destination_amount: Option<Decimal>,
    #[serde(with = "rust_decimal::serde::str_option")]
    pub unit_price: Option<Decimal>,
    pub cost_currency: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InstructionConversionInput {
    pub to_asset: AssetResolutionInput,
    #[serde(default, with = "rust_decimal::serde::str_option")]
    pub to_quantity: Option<Decimal>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstructionConversion {
    pub to_asset_id: String,
    pub to_asset_snapshot: InstructionAssetSnapshot,
    #[serde(with = "rust_decimal::serde::str_option")]
    pub to_quantity: Option<Decimal>,
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
    pub asset: Option<AssetResolutionInput>,
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
    // Omit new fields so legacy buy/sell request hashes remain identical.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transfer: Option<InstructionTransferInput>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversion: Option<InstructionConversionInput>,
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
    pub asset_id: Option<String>,
    pub account_name: String,
    pub asset_snapshot: Option<InstructionAssetSnapshot>,
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
    pub transfer: Option<InstructionTransfer>,
    pub conversion: Option<InstructionConversion>,
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
    pub sides: Option<Vec<InstructionSide>>,
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
pub struct UpdateTradeInstruction {
    pub expected_version: i32,
    pub account_id: String,
    pub asset: Option<AssetResolutionInput>,
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
    pub transfer: Option<InstructionTransferInput>,
    pub conversion: Option<InstructionConversionInput>,
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
    pub resolution_note: Option<String>,
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
