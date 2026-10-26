use super::*;
use crate::{
    accounts::{account_supports_portfolio_scope, Account, AccountPurpose, AccountServiceTrait},
    activities::AssetResolutionInput,
    assets::{Asset, AssetKind, AssetServiceTrait, ManualAssetResolver},
    errors::ValidationError,
    Error, Result,
};
use async_trait::async_trait;
use chrono::Utc;
use rust_decimal::Decimal;
use sha2::{Digest, Sha256};
use std::sync::Arc;
use uuid::Uuid;

pub struct TradeInstructionService {
    repository: Arc<dyn TradeInstructionRepositoryTrait>,
    accounts: Arc<dyn AccountServiceTrait>,
    assets: Arc<dyn AssetServiceTrait>,
    goals: Arc<dyn crate::goals::GoalServiceTrait>,
}
fn invalid(message: &str) -> Error {
    ValidationError::InvalidInput(message.into()).into()
}
fn conflict() -> Error {
    Error::ConstraintViolation("Trade instruction changed. Refresh and try again.".into())
}
fn note(value: &str) -> Result<String> {
    let value = value.trim();
    if value.is_empty() || value.len() > 4000 {
        return Err(invalid("A resolution note of 1–4000 bytes is required"));
    }
    Ok(value.into())
}
fn check_version(row: &TradeInstruction, version: i32) -> Result<()> {
    if row.version != version {
        return Err(conflict());
    }
    Ok(())
}
fn check_completion_time(time: chrono::DateTime<Utc>) -> Result<()> {
    if time > Utc::now() {
        return Err(invalid("Actual completion time cannot be in the future"));
    }
    Ok(())
}
impl TradeInstructionService {
    pub fn new(
        repository: Arc<dyn TradeInstructionRepositoryTrait>,
        accounts: Arc<dyn AccountServiceTrait>,
        assets: Arc<dyn AssetServiceTrait>,
        goals: Arc<dyn crate::goals::GoalServiceTrait>,
    ) -> Self {
        Self {
            repository,
            accounts,
            assets,
            goals,
        }
    }
    fn account(&self, id: &str) -> Result<Account> {
        let account = self.accounts.get_account(id)?;
        if !account.is_active
            || !account_supports_portfolio_scope(&account, AccountPurpose::Holdings)
        {
            return Err(invalid("An active investment account is required"));
        }
        Ok(account)
    }
    async fn resolve_asset(
        &self,
        asset_input: AssetResolutionInput,
        currency: String,
    ) -> Result<Asset> {
        if let Some(symbol) = asset_input.symbol.as_deref() {
            let resolver = ManualAssetResolver::new(self.assets.clone());
            let kind = asset_input
                .instrument_type
                .as_deref()
                .and_then(|value| ManualAssetResolver::parse_instrument_type(Some(value)))
                .map(|value| ManualAssetResolver::kind_from_instrument_type(&value))
                .unwrap_or_else(|| {
                    resolver
                        .infer_asset_kind(
                            symbol,
                            asset_input.exchange_mic.as_deref(),
                            asset_input.kind.as_deref(),
                        )
                        .0
                });
            if kind != AssetKind::Investment {
                return Err(invalid("An investment asset is required"));
            }
        }
        let (_, asset) = ManualAssetResolver::new(self.assets.clone())
            .resolve(asset_input, currency, true, None)
            .await?
            .ok_or_else(|| invalid("An investment asset is required"))?;
        if asset.kind != AssetKind::Investment || !asset.is_active {
            return Err(invalid("An active investment asset is required"));
        }
        Ok(asset)
    }
    async fn save(
        &self,
        mut row: TradeInstruction,
        status: InstructionStatus,
    ) -> Result<TradeInstruction> {
        let version = row.version;
        row.version += 1;
        row.updated_at = Utc::now();
        self.repository.update(row, version, status).await
    }
}
fn normalize(input: &mut NewTradeInstruction) -> Result<()> {
    input.request_id = Uuid::parse_str(&input.request_id)
        .map_err(|_| invalid("requestId must be a UUID"))?
        .to_string();
    input.account_id = input.account_id.trim().into();
    input.goal_id = input.goal_id.trim().into();
    if input.goal_id.is_empty() {
        return Err(invalid("A goal is required"));
    }
    if input.quantity.is_some() == input.amount.is_some() {
        return Err(invalid("Provide exactly one of quantity or amount"));
    }
    for value in [
        &mut input.quantity,
        &mut input.amount,
        &mut input.limit_price,
    ]
    .into_iter()
    .flatten()
    {
        if *value <= Decimal::ZERO {
            return Err(invalid("Quantity, amount and limit price must be positive"));
        }
        *value = value.normalize();
    }
    if input.amount.is_some() {
        let ccy = input
            .amount_currency
            .as_deref()
            .unwrap_or("")
            .trim()
            .to_uppercase();
        if !(3..=5).contains(&ccy.len()) || !ccy.bytes().all(|b| b.is_ascii_uppercase()) {
            return Err(invalid("An amount currency is required"));
        }
        input.amount_currency = Some(ccy);
    } else if input.amount_currency.is_some() {
        return Err(invalid("amountCurrency is only valid with amount"));
    }
    input.notes = input
        .notes
        .take()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    if input.notes.as_ref().is_some_and(|value| value.len() > 8000) {
        return Err(invalid("Notes exceed 8000 bytes"));
    }
    let a = &mut input.asset;
    for field in [
        &mut a.id,
        &mut a.symbol,
        &mut a.exchange_mic,
        &mut a.kind,
        &mut a.name,
        &mut a.quote_mode,
        &mut a.quote_ccy,
        &mut a.instrument_type,
        &mut a.provider_id,
        &mut a.provider_symbol,
    ] {
        *field = field
            .take()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty());
    }
    Ok(())
}
#[async_trait]
impl TradeInstructionServiceTrait for TradeInstructionService {
    fn get(&self, id: &str) -> Result<TradeInstruction> {
        self.repository.get(id)
    }
    fn list(&self, mut filter: InstructionFilter) -> Result<InstructionPage> {
        let limit = filter.limit.unwrap_or(50);
        if !(1..=100).contains(&limit) || filter.offset.unwrap_or(0) < 0 {
            return Err(invalid("Invalid pagination"));
        }
        if let (Some(from), Some(before)) = (
            filter.expected_completion_from,
            filter.expected_completion_before,
        ) {
            if from >= before {
                return Err(invalid("Invalid expected completion range"));
            }
        }
        filter.limit = Some(limit);
        self.repository.list(filter)
    }
    async fn create(
        &self,
        mut input: NewTradeInstruction,
        source: InstructionSource,
    ) -> Result<TradeInstruction> {
        normalize(&mut input)?;
        let hash = hex::encode(Sha256::digest(
            serde_json::to_vec(&input).map_err(|e| Error::Unexpected(e.to_string()))?,
        ));
        if let Some(existing) = self.repository.get_by_request_id(&input.request_id)? {
            if existing.request_hash != hash {
                return Err(conflict());
            }
            return Ok(existing);
        }
        self.goals.get_goal(&input.goal_id)?;
        let account = self.account(&input.account_id)?;
        let asset = self
            .resolve_asset(input.asset, account.currency.clone())
            .await?;
        let now = Utc::now();
        let row = TradeInstruction {
            goal_id: input.goal_id,
            id: Uuid::new_v4().to_string(),
            request_id: input.request_id,
            request_hash: hash,
            account_id: account.id,
            account_name: account.name,
            asset_id: asset.id,
            asset_snapshot: InstructionAssetSnapshot {
                name: asset.name,
                symbol: asset.instrument_symbol.or(asset.display_code),
                exchange_mic: asset.instrument_exchange_mic,
                instrument_type: asset.instrument_type.map(|v| v.as_db_str().to_string()),
                quote_ccy: asset.quote_ccy,
            },
            side: input.side,
            quantity: input.quantity,
            amount: input.amount,
            amount_currency: input.amount_currency,
            limit_price: input.limit_price,
            notes: input.notes,
            expected_completion_at: input.expected_completion_at,
            status: InstructionStatus::Pending,
            resolution_note: None,
            source,
            created_at: now,
            updated_at: now,
            started_at: None,
            closed_at: None,
            completed_at: None,
            version: 1,
        };
        self.repository.create(row).await
    }
    async fn edit(&self, id: &str, input: UpdateTradeInstruction) -> Result<TradeInstruction> {
        let mut row = self.get(id)?;
        check_version(&row, input.expected_version)?;
        if row.status != InstructionStatus::Pending {
            return Err(conflict());
        }
        let mut content = NewTradeInstruction {
            goal_id: row.goal_id.clone(),
            request_id: row.request_id.clone(),
            account_id: input.account_id,
            asset: input.asset,
            side: input.side,
            quantity: input.quantity,
            amount: input.amount,
            amount_currency: input.amount_currency,
            limit_price: input.limit_price,
            notes: input.notes,
            expected_completion_at: input.expected_completion_at,
        };
        normalize(&mut content)?;
        let account = self.account(&content.account_id)?;
        if row.account_id != account.id {
            row.account_id = account.id;
            row.account_name = account.name;
        }
        if content.asset.id.as_deref() != Some(row.asset_id.as_str()) {
            let asset = self.resolve_asset(content.asset, account.currency).await?;
            row.asset_id = asset.id;
            row.asset_snapshot = InstructionAssetSnapshot {
                name: asset.name,
                symbol: asset.instrument_symbol.or(asset.display_code),
                exchange_mic: asset.instrument_exchange_mic,
                instrument_type: asset
                    .instrument_type
                    .map(|value| value.as_db_str().to_string()),
                quote_ccy: asset.quote_ccy,
            };
        }
        row.side = content.side;
        row.quantity = content.quantity;
        row.amount = content.amount;
        row.amount_currency = content.amount_currency;
        row.limit_price = content.limit_price;
        row.notes = content.notes;
        row.expected_completion_at = content.expected_completion_at;
        self.save(row, InstructionStatus::Pending).await
    }
    async fn delete(&self, id: &str, expected_version: i32) -> Result<()> {
        self.repository.delete(id, expected_version).await
    }
    async fn start(&self, id: &str, expected_version: i32) -> Result<TradeInstruction> {
        let mut row = self.get(id)?;
        check_version(&row, expected_version)?;
        if row.status != InstructionStatus::Pending {
            return Err(conflict());
        }
        self.account(&row.account_id)?;
        let asset = self.assets.get_asset_by_id(&row.asset_id)?;
        if !asset.is_active || asset.kind != AssetKind::Investment {
            return Err(invalid("Asset is unavailable"));
        }
        row.status = InstructionStatus::InProgress;
        row.started_at = Some(Utc::now());
        self.save(row, InstructionStatus::Pending).await
    }
    async fn complete(&self, id: &str, input: CompleteInstruction) -> Result<TradeInstruction> {
        let mut row = self.get(id)?;
        let resolution_note = input
            .resolution_note
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(note)
            .transpose()?;
        // A retried completion never overwrites the original or a corrected time.
        if row.status == InstructionStatus::Completed
            && input.expected_version == row.version - 1
            && row.resolution_note == resolution_note
            && input
                .completed_at
                .is_none_or(|time| Some(time) == row.completed_at)
        {
            return Ok(row);
        }
        check_version(&row, input.expected_version)?;
        if !row.status.is_open() {
            return Err(conflict());
        }
        let previous = row.status;
        let completed_at = input.completed_at.unwrap_or_else(Utc::now);
        check_completion_time(completed_at)?;
        row.resolution_note = resolution_note;
        row.status = InstructionStatus::Completed;
        row.closed_at = Some(Utc::now());
        row.completed_at = Some(completed_at);
        self.save(row, previous).await
    }
    async fn cancel(&self, id: &str, input: CancelInstruction) -> Result<TradeInstruction> {
        let mut row = self.get(id)?;
        check_version(&row, input.expected_version)?;
        if !row.status.is_open() {
            return Err(conflict());
        }
        let previous = row.status;
        row.resolution_note = Some(note(&input.resolution_note)?);
        row.status = InstructionStatus::Cancelled;
        row.closed_at = Some(Utc::now());
        self.save(row, previous).await
    }
    async fn correct_completion_time(
        &self,
        id: &str,
        input: CorrectCompletionTime,
    ) -> Result<TradeInstruction> {
        let mut row = self.get(id)?;
        check_version(&row, input.expected_version)?;
        if row.status != InstructionStatus::Completed {
            return Err(conflict());
        }
        check_completion_time(input.completed_at)?;
        row.completed_at = Some(input.completed_at);
        self.save(row, InstructionStatus::Completed).await
    }
}
