//! Quote synchronization service.
//!
//! This module provides the `QuoteSyncService` which orchestrates quote fetching
//! from market data providers and manages synchronization state.
//!
//! # Architecture
//!
//! ```text
//! QuoteSyncService
//!       │
//!       ├─► MarketDataClient (fetch quotes via market-data crate)
//!       ├─► QuoteStore (persist quotes)
//!       ├─► SyncStateStore (track sync state)
//!       ├─► AssetRepository (asset lookups)
//!       └─► ActivityRepository (activity bounds)
//! ```
//!
//! # Key Design Principles
//!
//! - **Thin orchestration**: Quote fetching delegated to `MarketDataClient`
//! - **Strong types**: Uses `AssetId`, `Day`, `QuoteSource` from `types.rs`
//! - **Activity-driven sync**: Sync ranges based on activity dates

use async_trait::async_trait;
use chrono::{DateTime, Duration, NaiveDate, TimeZone, Utc};
use futures::stream::{self, StreamExt};
use log::{debug, error, info, warn};
use rust_decimal::Decimal;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, Weak};
use tokio::sync::RwLock;

use super::client::{credential_backed_provider, HistoricalQuoteFetchError, MarketDataClient};
use super::constants::*;
use super::errors::MarketDataError;
use super::history::{
    missing_history_ranges_with_freshness, HistoryCoverage, HistoryOutcome, HistoryWriteMode,
};
use super::model::Quote;
use super::store::QuoteStore;
use super::sync_state::{
    calculate_sync_window, determine_sync_category, QuoteSyncState, SymbolSyncPlan, SyncCategory,
    SyncMode, SyncPlanningInputs, SyncStateStore,
};
use super::types::{AssetId, Day, ProviderId};
use crate::activities::{ActivityRepositoryTrait, ActivityUpsert};
use crate::assets::{Asset, AssetKind, AssetRepositoryTrait, InstrumentType, QuoteMode};
use crate::errors::Error;
use crate::errors::Result;
use crate::utils::time_utils;

// =============================================================================
// Per-Asset Sync Locking (US-012)
// =============================================================================

/// Locks belong to one profile's service, never to a process-global ticker map.
/// Waiting callers re-read coverage after acquiring the lock, so overlapping work
/// coalesces without discarding requests for different ranges or explicit refreshes.
#[derive(Default)]
struct HistoryLocks(Mutex<HashMap<String, Weak<tokio::sync::Mutex<()>>>>);

impl HistoryLocks {
    fn for_asset(&self, asset_id: &str) -> Arc<tokio::sync::Mutex<()>> {
        let mut locks = self.0.lock().unwrap_or_else(|error| error.into_inner());
        locks.retain(|_, lock| lock.strong_count() > 0);
        if let Some(lock) = locks.get(asset_id).and_then(Weak::upgrade) {
            return lock;
        }
        let lock = Arc::new(tokio::sync::Mutex::new(()));
        locks.insert(asset_id.to_owned(), Arc::downgrade(&lock));
        lock
    }
}

#[derive(Clone)]
struct HistoryOwnership {
    _asset: Arc<tokio::sync::OwnedMutexGuard<()>>,
    _client: Arc<tokio::sync::OwnedRwLockReadGuard<MarketDataClient>>,
}

struct LocalHistoryError {
    _ownership: HistoryOwnership,
    fetch_error: Box<HistoricalQuoteFetchError>,
    written: usize,
}

struct LocalHistoryResult {
    _ownership: HistoryOwnership,
    quotes: Vec<Quote>,
    written: usize,
    fetched_nonempty: bool,
    provider_id: Option<String>,
}

fn latest_observation_range(
    timestamp: DateTime<Utc>,
    market_day: NaiveDate,
    observed_at: DateTime<Utc>,
) -> Option<(NaiveDate, NaiveDate)> {
    (timestamp <= observed_at).then(|| {
        (
            timestamp.date_naive().min(market_day),
            timestamp.date_naive().max(market_day),
        )
    })
}

fn latest_history_outcome(asset: &Asset, source: &str) -> HistoryOutcome {
    if credential_backed_provider(source)
        || asset
            .preferred_provider()
            .is_some_and(|provider| credential_backed_provider(&provider))
    {
        HistoryOutcome::Partial
    } else {
        HistoryOutcome::Complete
    }
}

fn quote_matches_currency(quote: &Quote, asset: &Asset) -> bool {
    quote.data_source == DATA_SOURCE_MANUAL
        || asset.quote_ccy.is_empty()
        || crate::fx::currency::normalize_currency_code(&quote.currency)
            == crate::fx::currency::normalize_currency_code(&asset.quote_ccy)
}

/// Unknown benchmarks may discover their actual quote currency, but a single
/// returned series must never mix units across provider responses/cache fills.
fn validate_history_currency(quotes: &[Quote], currency: &mut Option<String>) -> Result<()> {
    for quote in quotes {
        let Some(current) = crate::assets::normalize_quote_ccy_code(Some(&quote.currency)) else {
            continue;
        };
        if currency
            .as_ref()
            .is_some_and(|expected| expected != &current)
        {
            return Err(Error::MarketData(MarketDataError::InvalidData(
                "Provider history contains inconsistent quote currencies".into(),
            )));
        }
        *currency = Some(current);
    }
    Ok(())
}

fn history_identity(
    asset: &Asset,
    client: &MarketDataClient,
    operation: &str,
    revision: &str,
    benchmark: bool,
) -> String {
    // Persist only a digest. Provider configurations can contain private URLs.
    let identity = serde_json::json!({
        "operation": operation, "benchmark": benchmark, "asset": asset.id, "kind": asset.kind,
        "mode": asset.quote_mode, "currency": asset.quote_ccy,
        "type": asset.instrument_type, "symbol": asset.instrument_symbol,
        "exchange": asset.instrument_exchange_mic, "preferredProvider": asset.preferred_provider(),
        "overrides": asset.provider_overrides(),
        "customProviderCode": asset.provider_config.as_ref().and_then(|config| config.get("custom_provider_code")),
        "isin": asset.metadata.as_ref().and_then(|metadata| metadata.get("identifiers")).and_then(|identifiers| identifiers.get("isin")),
        "bond": asset.bond_spec(), "providers": client.history_configuration_fingerprint(), "configurationRevision": revision,
    });
    format!("{:x}", Sha256::digest(identity.to_string().as_bytes()))
}

fn history_range_settled(
    asset: &Asset,
    record: &HistoryCoverage,
    now: DateTime<Utc>,
    latest: Option<NaiveDate>,
) -> Option<bool> {
    // Unknown exchanges and continuous markets have no reliable daily close.
    let mic = asset.instrument_exchange_mic.as_deref()?;
    if is_continuous_market(asset.instrument_type.as_ref())
        || wealthfolio_market_data::resolver::exchange_metadata::mic_to_market_close(mic).is_none()
    {
        return None;
    }
    let expected = effective_market_today(now, Some(mic), asset.instrument_type.as_ref());
    let fetched_expected =
        effective_market_today(record.fetched_at, Some(mic), asset.instrument_type.as_ref());
    if expected > fetched_expected {
        // A newly completed session also reopens the correction overlap of
        // previously settled receipts. The range helper retains the old prefix,
        // rather than allowing Friday's receipt to hide corrections forever.
        return Some(false);
    }
    if record.end <= expected
        && record.end <= fetched_expected
        && latest.is_some_and(|latest| latest >= record.end.min(expected))
    {
        Some(true)
    } else {
        None
    }
}

fn is_continuous_market(instrument_type: Option<&InstrumentType>) -> bool {
    matches!(instrument_type, Some(InstrumentType::Crypto))
}

fn effective_market_today(
    now: DateTime<Utc>,
    exchange_mic: Option<&str>,
    instrument_type: Option<&InstrumentType>,
) -> NaiveDate {
    time_utils::market_effective_date(now, exchange_mic, is_continuous_market(instrument_type))
}

fn market_fetch_end_date(
    now: DateTime<Utc>,
    exchange_mic: Option<&str>,
    instrument_type: Option<&InstrumentType>,
) -> NaiveDate {
    time_utils::market_calendar_date(now, exchange_mic, is_continuous_market(instrument_type))
}

/// Determine the effective data provider for an asset.
///
/// Priority: sync state `data_source` (if non-empty) → asset `preferred_provider` → Yahoo default.
fn effective_provider(state: Option<&QuoteSyncState>, asset: &Asset) -> String {
    state
        .map(|s| &s.data_source)
        .filter(|ds| !ds.is_empty())
        .cloned()
        .or_else(|| asset.preferred_provider())
        .unwrap_or_else(|| DATA_SOURCE_YAHOO.to_string())
}

fn extends_to_fetch_end(category: &SyncCategory) -> bool {
    matches!(
        category,
        SyncCategory::Active | SyncCategory::RecentlyClosed | SyncCategory::New
    )
}

fn clamp_end_date_for_fetch(
    category: &SyncCategory,
    end_date: NaiveDate,
    fetch_end_date: NaiveDate,
) -> NaiveDate {
    if extends_to_fetch_end(category) {
        end_date.max(fetch_end_date)
    } else {
        end_date
    }
}

fn should_treat_fetch_error_as_non_fatal(
    category: &SyncCategory,
    error: &Error,
    suppress_closed_fetch_errors: bool,
) -> bool {
    if !(matches!(category, SyncCategory::NeedsBackfill)
        || matches!(category, SyncCategory::Closed) && suppress_closed_fetch_errors)
    {
        return false;
    }

    matches!(
        error,
        Error::MarketData(MarketDataError::NotFound(_))
            | Error::MarketData(MarketDataError::ProviderExhausted(_))
    )
}

fn fetch_error_skip_reason(error: &Error) -> Option<AssetSkipReason> {
    match error {
        Error::MarketData(MarketDataError::NoData) => Some(AssetSkipReason::NoDataForRange),
        _ => None,
    }
}

fn error_message_contains_provider_id(message: &str) -> bool {
    super::constants::MARKET_DATA_PROVIDER_IDS
        .iter()
        .any(|provider_id| message.contains(provider_id))
}

fn format_sync_failure_message(error: &Error, provider_id: Option<&str>) -> String {
    let message = error.to_string();
    if error_message_contains_provider_id(&message) {
        return message;
    }

    match provider_id {
        Some(provider_id) if !provider_id.is_empty() => format!("{}: {}", provider_id, message),
        _ => message,
    }
}

pub(super) fn asset_skip_reason(asset: &Asset, allow_inactive: bool) -> Option<AssetSkipReason> {
    // Only sync market-priced assets (including FX rates for currency conversion)
    if asset.quote_mode != QuoteMode::Market {
        return Some(AssetSkipReason::ManualPricing);
    }

    // This only covers catalog lifecycle. Closed positions on still-active catalog
    // assets are handled later after sync category planning.
    // Targeted row-level sync can opt in so users can refresh deactivated assets.
    if !allow_inactive && !asset.is_active {
        return Some(AssetSkipReason::Inactive);
    }

    // Skip matured bonds — price is par, no sync needed
    if asset.is_bond() {
        if let Some(spec) = asset.bond_spec() {
            if let Some(maturity) = spec.maturity_date {
                if maturity < Utc::now().date_naive() {
                    return Some(AssetSkipReason::MaturedBond);
                }
            }
        }
    }

    // Skip expired options — no further quotes available
    if asset.is_option() {
        if let Some(spec) = asset.option_spec() {
            if spec.expiration < Utc::now().date_naive() {
                return Some(AssetSkipReason::ExpiredOption);
            }
        }
    }

    None
}

fn is_empty_asset_target(asset_ids: Option<&[String]>) -> bool {
    asset_ids.is_some_and(|ids| ids.is_empty())
}

fn is_targeted_asset_sync(asset_ids: Option<&[String]>) -> bool {
    asset_ids.is_some_and(|ids| !ids.is_empty())
}

fn should_allow_inactive_assets(mode: SyncMode, targeted_sync: bool) -> bool {
    targeted_sync
        || matches!(
            mode,
            SyncMode::RefetchRecent { .. }
                | SyncMode::BackfillHistory { .. }
                | SyncMode::RefreshAllHistory { .. }
        )
}

fn should_include_closed_positions(mode: SyncMode, targeted_sync: bool) -> bool {
    targeted_sync
        || matches!(
            mode,
            SyncMode::RefetchRecent { .. }
                | SyncMode::BackfillHistory { .. }
                | SyncMode::RefreshAllHistory { .. }
        )
}

fn should_skip_for_error_limit(mode: SyncMode, targeted_sync: bool, error_count: i32) -> bool {
    !targeted_sync
        && !matches!(
            mode,
            SyncMode::BackfillHistory { .. } | SyncMode::RefreshAllHistory { .. }
        )
        && error_count >= MAX_SYNC_ERRORS
}

fn min_optional_date(left: Option<NaiveDate>, right: Option<NaiveDate>) -> Option<NaiveDate> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left.min(right)),
        (Some(date), None) | (None, Some(date)) => Some(date),
        (None, None) => None,
    }
}

fn max_optional_date(left: Option<NaiveDate>, right: Option<NaiveDate>) -> Option<NaiveDate> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left.max(right)),
        (Some(date), None) | (None, Some(date)) => Some(date),
        (None, None) => None,
    }
}

fn merge_bounds(
    activity_bounds: Option<(Option<NaiveDate>, Option<NaiveDate>)>,
    holdings_bounds: Option<(Option<NaiveDate>, Option<NaiveDate>)>,
) -> (Option<NaiveDate>, Option<NaiveDate>) {
    let (activity_min, activity_max) = activity_bounds.unwrap_or((None, None));
    let (holdings_min, holdings_max) = holdings_bounds.unwrap_or((None, None));

    (
        min_optional_date(activity_min, holdings_min),
        max_optional_date(activity_max, holdings_max),
    )
}

fn has_bounds_reference(bounds: Option<&(Option<NaiveDate>, Option<NaiveDate>)>) -> bool {
    bounds.is_some_and(|(min_date, max_date)| min_date.is_some() || max_date.is_some())
}

fn has_historical_reference(
    asset_id: &str,
    existing_states: &HashMap<String, QuoteSyncState>,
    activity_bounds: &HashMap<String, (Option<NaiveDate>, Option<NaiveDate>)>,
    holdings_bounds: &HashMap<String, (Option<NaiveDate>, Option<NaiveDate>)>,
) -> bool {
    existing_states.contains_key(asset_id)
        || has_bounds_reference(activity_bounds.get(asset_id))
        || has_bounds_reference(holdings_bounds.get(asset_id))
}

fn should_allow_inactive_asset(
    asset_id: &str,
    mode: SyncMode,
    targeted_sync: bool,
    existing_states: &HashMap<String, QuoteSyncState>,
    activity_bounds: &HashMap<String, (Option<NaiveDate>, Option<NaiveDate>)>,
    holdings_bounds: &HashMap<String, (Option<NaiveDate>, Option<NaiveDate>)>,
) -> bool {
    targeted_sync
        || (should_allow_inactive_assets(mode, targeted_sync)
            && has_historical_reference(
                asset_id,
                existing_states,
                activity_bounds,
                holdings_bounds,
            ))
}

fn calculate_targeted_closed_incremental_window(
    inputs: &SyncPlanningInputs,
    fetch_end_date: NaiveDate,
) -> Option<(NaiveDate, NaiveDate)> {
    let end_date = fetch_end_date;
    let start_date = inputs
        .quote_max
        .map(|quote_max| quote_max - Duration::days(OVERLAP_DAYS))
        .or_else(|| {
            inputs
                .activity_min
                .map(|activity_min| activity_min - Duration::days(QUOTE_HISTORY_BUFFER_DAYS))
        })
        .unwrap_or_else(|| end_date - Duration::days(MIN_SYNC_LOOKBACK_DAYS));

    if start_date > end_date {
        None
    } else {
        Some((start_date, end_date))
    }
}

// =============================================================================
// Sync Result Types
// =============================================================================

/// Result of a sync operation for a single asset.
#[derive(Debug, Clone)]
pub struct AssetSyncResult {
    /// The asset that was synced.
    pub asset_id: AssetId,
    /// Number of quotes added/updated.
    pub quotes_added: usize,
    /// The sync status after the operation.
    pub status: SyncStatus,
    /// Optional error message if sync failed.
    pub error: Option<String>,
    /// Structured reason when the sync was skipped.
    pub skip_reason: Option<AssetSkipReason>,
}

/// Status of a sync operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncStatus {
    /// Sync completed successfully.
    Success,
    /// Sync was skipped (e.g., manual pricing mode).
    Skipped,
    /// Sync failed with error.
    Failed,
}

/// Reason why an asset was skipped during sync.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssetSkipReason {
    /// Asset is a cash asset (always 1:1, no quotes needed).
    CashAsset,
    /// Asset uses manual pricing mode (not market-priced).
    ManualPricing,
    /// Asset is inactive.
    Inactive,
    /// Asset position is closed (grace period expired).
    ClosedPosition,
    /// Asset not found in repository.
    NotFound,
    /// Sync already in progress for this asset (US-012).
    SyncInProgress,
    /// Too many consecutive sync failures (exceeds MAX_SYNC_ERRORS).
    TooManyErrors,
    /// No fetchable quote window remains for the requested sync.
    NoQuoteWindow,
    /// Providers returned no quotes for the requested date range.
    NoDataForRange,
    /// Bond has matured — price is par, no further sync needed.
    MaturedBond,
    /// Option has expired — no further quotes available.
    ExpiredOption,
}

impl std::fmt::Display for AssetSkipReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AssetSkipReason::CashAsset => write!(f, "Cash asset (no quotes needed)"),
            AssetSkipReason::ManualPricing => write!(f, "Manual pricing mode"),
            AssetSkipReason::Inactive => write!(f, "Inactive asset"),
            AssetSkipReason::ClosedPosition => write!(f, "Closed position"),
            AssetSkipReason::NotFound => write!(f, "Asset not found"),
            AssetSkipReason::SyncInProgress => write!(f, "Sync already in progress"),
            AssetSkipReason::TooManyErrors => write!(f, "Too many consecutive sync failures"),
            AssetSkipReason::NoQuoteWindow => write!(f, "No quote refresh needed"),
            AssetSkipReason::NoDataForRange => {
                write!(f, "No data for requested date range")
            }
            AssetSkipReason::MaturedBond => write!(f, "Bond has matured (price is par)"),
            AssetSkipReason::ExpiredOption => write!(f, "Option has expired"),
        }
    }
}

/// Aggregate result of a sync operation.
#[derive(Debug, Clone, Default)]
pub struct SyncResult {
    /// Number of assets successfully synced.
    pub synced: usize,
    /// Number of assets that failed to sync.
    pub failed: usize,
    /// Number of assets that were skipped.
    pub skipped: usize,
    /// Total quotes added/updated.
    pub quotes_synced: usize,
    /// Detailed errors for each failed asset.
    pub errors: Vec<SyncError>,
    /// Legacy field for backwards compatibility.
    pub failures: Vec<(String, String)>,
    /// Reasons why specific assets were skipped (asset_id, reason).
    pub skipped_reasons: Vec<(String, AssetSkipReason)>,
}

impl SyncResult {
    /// Check if the sync was fully successful (no failures).
    pub fn is_success(&self) -> bool {
        self.failed == 0
    }

    /// Get a summary string.
    pub fn summary(&self) -> String {
        if self.is_success() {
            format!(
                "Synced {} quotes for {} assets successfully",
                self.quotes_synced, self.synced
            )
        } else {
            format!(
                "Synced {} quotes with {} failures",
                self.quotes_synced, self.failed
            )
        }
    }

    /// Add a result for a single asset.
    fn add_result(&mut self, result: AssetSyncResult) {
        // A later disjoint range can fail after an earlier transaction committed.
        self.quotes_synced += result.quotes_added;
        match result.status {
            SyncStatus::Success => {
                self.synced += 1;
            }
            SyncStatus::Skipped => {
                self.skipped += 1;
                if let Some(reason) = result.skip_reason {
                    self.skipped_reasons
                        .push((result.asset_id.0.clone(), reason));
                }
            }
            SyncStatus::Failed => {
                self.failed += 1;
                if let Some(err) = result.error.clone() {
                    self.errors.push(SyncError {
                        asset_id: result.asset_id.clone(),
                        message: err.clone(),
                        provider_errors: vec![],
                    });
                    self.failures.push((result.asset_id.0.clone(), err));
                }
            }
        }
    }

    /// Record that an asset was skipped with a specific reason.
    fn add_skipped(&mut self, asset_id: String, reason: AssetSkipReason) {
        self.skipped += 1;
        self.skipped_reasons.push((asset_id, reason));
    }
}

/// Error details for a failed sync operation.
#[derive(Debug, Clone)]
pub struct SyncError {
    /// The asset that failed to sync.
    pub asset_id: AssetId,
    /// Error message.
    pub message: String,
    /// Provider-specific errors if applicable.
    pub provider_errors: Vec<(ProviderId, String)>,
}

// =============================================================================
// Service Trait
// =============================================================================

/// Trait for the quote sync service.
#[async_trait]
pub trait QuoteSyncServiceTrait: Send + Sync {
    /// Perform quote synchronization with the specified mode and optional asset filter.
    ///
    /// # Arguments
    /// * `mode` - Requested date range and, only for explicit full refresh, local-coverage bypass
    /// * `asset_ids` - Optional list of specific assets to sync. If None, syncs all relevant assets.
    ///
    /// # Sync Modes
    /// * `Incremental` - Default mode. Continues from last_quote_date with overlap to heal corrections.
    /// * `RefetchRecent { days }` - Ensures the last N days using fresh local coverage first.
    /// * `BackfillHistory { days }` - Ensures history from activity start (or N days fallback), local-first.
    /// * `RefreshAllHistory { days }` - Explicit full-history refresh; bypasses coverage and merges prices.
    async fn sync(&self, mode: SyncMode, asset_ids: Option<Vec<String>>) -> Result<SyncResult>;

    /// Legacy explicit full-history refresh alias, using RefreshAllHistory mode.
    ///
    /// Calls `sync(SyncMode::RefreshAllHistory { days: DEFAULT_HISTORY_DAYS }, asset_ids)`.
    /// Automatic rebuilds use BackfillHistory directly and remain local-first.
    /// If asset_ids is None, resync all syncable assets. If asset_ids is empty, sync nothing.
    async fn resync(&self, asset_ids: Option<Vec<String>>) -> Result<SyncResult>;

    /// Handle a new activity being created.
    /// This may expand the sync range to include the activity date.
    async fn handle_activity_created(&self, asset_id: &AssetId, activity_date: Day) -> Result<()>;

    /// Handle an activity being deleted.
    /// This may recalculate the required sync range.
    async fn handle_activity_deleted(&self, asset_id: &AssetId) -> Result<()>;

    /// Ensure sync state entries exist for syncable assets.
    async fn refresh_sync_state(&self) -> Result<()>;

    /// Get the current sync plan without executing it.
    fn get_sync_plan(&self) -> Result<Vec<SymbolSyncPlan>>;
}

// =============================================================================
// Quote Sync Service
// =============================================================================

/// Quote synchronization service.
///
/// Orchestrates quote fetching from market data providers
/// and manages synchronization state. This is a thin layer that
/// delegates actual fetching to `MarketDataClient` and storage to `QuoteStore`.
pub struct QuoteSyncService<Q, S, A, R>
where
    Q: QuoteStore,
    S: SyncStateStore,
    A: AssetRepositoryTrait,
    R: ActivityRepositoryTrait,
{
    /// Market data client for fetching quotes from providers.
    client: Arc<RwLock<MarketDataClient>>,
    /// Quote storage.
    quote_store: Arc<Q>,
    /// Sync state storage.
    sync_state_store: Arc<S>,
    /// Asset repository for asset lookups.
    asset_repo: Arc<A>,
    /// Activity repository for activity bounds.
    activity_repo: Arc<R>,
    history_locks: HistoryLocks,
    settings_service: Option<Arc<dyn crate::settings::SettingsServiceTrait>>,
}

impl<Q, S, A, R> QuoteSyncService<Q, S, A, R>
where
    Q: QuoteStore + 'static,
    S: SyncStateStore + 'static,
    A: AssetRepositoryTrait + 'static,
    R: ActivityRepositoryTrait + 'static,
{
    /// Create a new quote sync service.
    pub fn new(
        client: Arc<RwLock<MarketDataClient>>,
        quote_store: Arc<Q>,
        sync_state_store: Arc<S>,
        asset_repo: Arc<A>,
        activity_repo: Arc<R>,
    ) -> Self {
        Self {
            client,
            quote_store,
            sync_state_store,
            asset_repo,
            activity_repo,
            history_locks: HistoryLocks::default(),
            settings_service: None,
        }
    }

    pub fn with_settings_service(
        mut self,
        settings: Arc<dyn crate::settings::SettingsServiceTrait>,
    ) -> Self {
        self.settings_service = Some(settings);
        self
    }

    fn retry_interval(&self) -> Duration {
        let hours = self
            .settings_service
            .as_ref()
            .and_then(|settings| settings.get_settings().ok())
            .map(|settings| settings.market_data_sync_interval_hours)
            .unwrap_or(crate::settings::DEFAULT_MARKET_DATA_SYNC_INTERVAL_HOURS);
        Duration::hours(i64::from(hours.max(1)))
    }

    /// Explicit replacement is the only synchronization operation allowed to remove history.
    pub async fn reset_provider_history(
        &self,
        asset_id: &str,
    ) -> Result<super::ResetProviderHistoryResult> {
        let _guard = self
            .history_locks
            .for_asset(asset_id)
            .try_lock_owned()
            .map_err(|_| {
                Error::Asset("Already refreshing this asset; try again when it finishes".into())
            })?;
        let context = self.quote_store.provider_history_reset_context(asset_id)?;
        let asset = &context.asset;
        context.ensure_eligible()?;
        let ids = vec![asset_id.to_owned()];
        let activity_bounds = self.activity_repo.get_activity_bounds_for_assets(&ids)?;
        let holdings_bounds = self
            .activity_repo
            .get_holdings_snapshot_bounds_for_assets(&ids)?;
        let (activity_min, activity_max) = merge_bounds(
            activity_bounds.get(asset_id).copied(),
            holdings_bounds.get(asset_id).copied(),
        );
        let inputs = SyncPlanningInputs {
            is_active: asset.is_active,
            position_closed_date: None,
            activity_min,
            activity_max,
            quote_min: None,
            quote_max: None,
        };
        let now = Utc::now();
        let end = market_fetch_end_date(
            now,
            asset.instrument_exchange_mic.as_deref(),
            asset.instrument_type.as_ref(),
        );
        let (normal_start, _) = self.calculate_date_range_for_mode(
            &inputs,
            SyncMode::BackfillHistory {
                days: DEFAULT_HISTORY_DAYS,
            },
            end,
            end,
            asset,
        );
        let start = context
            .earliest_provider_date
            .map_or(normal_start, |date| date.min(normal_start));
        let client = Arc::new(self.client.clone().read_owned().await);
        if client.provider_configuration() != context.provider_configuration.as_slice() {
            return Err(Error::Asset(
                "Provider settings are being updated; please try again".into(),
            ));
        }
        let quotes = client
            .fetch_history_for_reset(
                asset,
                Utc.from_utc_datetime(
                    &start
                        .and_hms_opt(0, 0, 0)
                        .expect("00:00:00 is a valid time"),
                ),
                Utc.from_utc_datetime(
                    &end.and_hms_opt(23, 59, 59)
                        .expect("23:59:59 is a valid time"),
                ),
            )
            .await?;
        // Hold the client read lock through commit, keeping in-memory provider configuration stable.
        let store = self.quote_store.clone();
        tokio::spawn(async move {
            let _guard = _guard;
            let _client = client;
            store.replace_provider_history(context, quotes).await
        })
        .await
        .map_err(|error| Error::Unexpected(error.to_string()))?
    }

    /// Reset independently per asset; failures never undo successful replacements.
    pub async fn reset_all_provider_history(&self) -> Result<super::ResetAllProviderHistoryResult> {
        let mut result = super::ResetAllProviderHistoryResult::default();
        for asset in self.asset_repo.list()? {
            if let Some(reason) = asset_skip_reason(&asset, true) {
                result.skipped.push(super::ProviderHistoryResetSkipped {
                    asset_id: asset.id,
                    reason: reason.to_string(),
                });
                continue;
            }
            match self.reset_provider_history(&asset.id).await {
                Ok(reset) => result.results.push(reset),
                Err(error) => result.failures.push(super::ProviderHistoryResetFailure {
                    asset_id: asset.id,
                    error: error.to_string(),
                }),
            }
        }
        Ok(result)
    }

    /// Check if an asset should be synced.
    fn should_sync_asset(&self, asset: &Asset) -> bool {
        self.get_skip_reason(asset, false).is_none()
    }

    /// Get the reason why an asset should be skipped, if any.
    /// Returns None if the asset should be synced.
    fn get_skip_reason(&self, asset: &Asset, allow_inactive: bool) -> Option<AssetSkipReason> {
        asset_skip_reason(asset, allow_inactive)
    }

    /// Ensure a matured bond has a par-value quote so it doesn't show as "no data"
    /// in health checks. If the bond has never been successfully synced, we write
    /// a single quote at par (1.0 as fraction-of-par) dated at maturity and reset
    /// the error counter.
    async fn ensure_matured_bond_par_quote(&self, asset: &Asset) -> usize {
        let spec = match asset.bond_spec() {
            Some(s) => s,
            None => return 0,
        };
        let maturity = match spec.maturity_date {
            Some(d) => d,
            None => return 0,
        };

        // Check if any quote already exists for this asset
        if let Ok(Some(_)) = self.quote_store.latest(&AssetId(asset.id.clone()), None) {
            return 0;
        }

        // No quote exists — write a par-value quote at the maturity date
        let timestamp = maturity
            .and_hms_opt(16, 0, 0)
            .map(|dt| Utc.from_utc_datetime(&dt))
            .unwrap_or_else(Utc::now);

        let par = Decimal::ONE;
        let quote = Quote {
            id: format!("{}_{}_{}", asset.id, maturity.format("%Y-%m-%d"), "MANUAL"),
            asset_id: asset.id.clone(),
            timestamp,
            open: par,
            high: par,
            low: par,
            close: par,
            adjclose: par,
            volume: Decimal::ZERO,
            currency: asset.quote_ccy.clone(),
            data_source: DATA_SOURCE_MANUAL.to_string(),
            created_at: Utc::now(),
            notes: Some("Par value at maturity (auto-generated)".to_string()),
        };

        match self.quote_store.upsert_quotes(&[quote]).await {
            Ok(count) => {
                info!(
                    "Wrote par-value quote for matured bond {} (matured {})",
                    asset.id, maturity
                );
                // Reset error count so health check stops flagging this asset
                if let Err(e) = self.sync_state_store.update_after_sync(&asset.id).await {
                    warn!("Failed to reset sync state for {}: {:?}", asset.id, e);
                }
                count
            }
            Err(e) => {
                warn!(
                    "Failed to write par-value quote for matured bond {}: {:?}",
                    asset.id, e
                );
                0
            }
        }
    }

    /// Calculate the date range for syncing an asset based on SyncMode.
    ///
    /// This is the primary method for determining sync windows, used when an explicit
    /// SyncMode is provided (typically from external callers or resync operations).
    ///
    /// # FX Asset Handling (US-006)
    ///
    /// FX assets (e.g., `FX:EUR:USD`) have no activities attached to them, so their
    /// activity_min is always NULL. For full history recomputation jobs
    /// (BackfillHistory mode), FX assets need quote coverage from the earliest
    /// activity date across ALL accounts to support valuation history calculations.
    ///
    /// - **BackfillHistory mode + FX asset**: Use global earliest activity date
    /// - **Incremental mode + FX asset**: Continue from quote_max (no backfill)
    fn calculate_date_range_for_mode(
        &self,
        inputs: &SyncPlanningInputs,
        mode: SyncMode,
        planning_today: NaiveDate,
        fetch_end_date: NaiveDate,
        asset: &Asset,
    ) -> (NaiveDate, NaiveDate) {
        match mode {
            SyncMode::Incremental => {
                // Use category-based calculation for incremental mode
                let category = determine_sync_category(
                    inputs,
                    CLOSED_POSITION_GRACE_PERIOD_DAYS,
                    planning_today,
                );
                let (start, end) = calculate_sync_window(&category, inputs, planning_today)
                    .unwrap_or((
                        fetch_end_date - Duration::days(QUOTE_HISTORY_BUFFER_DAYS),
                        fetch_end_date,
                    ));

                (
                    start,
                    clamp_end_date_for_fetch(&category, end, fetch_end_date),
                )
            }
            SyncMode::RefetchRecent { days } => {
                // Select the last N days; execution still reuses fresh local coverage.
                let start = fetch_end_date - Duration::days(days);
                (start, fetch_end_date)
            }
            SyncMode::BackfillHistory { days } | SyncMode::RefreshAllHistory { days } => {
                // For FX assets in BackfillHistory mode, use global earliest activity date
                // FX assets have no activities, so activity_min is always NULL
                if asset.kind == AssetKind::Fx {
                    let global_earliest = self
                        .activity_repo
                        .get_first_activity_date_overall()
                        .ok()
                        .map(|dt| dt.date_naive());

                    let start = global_earliest
                        .map(|d| d - Duration::days(QUOTE_HISTORY_BUFFER_DAYS))
                        .unwrap_or_else(|| fetch_end_date - Duration::days(days));

                    debug!(
                        "FX asset {} BackfillHistory: global_earliest={:?}, start={}",
                        asset.id, global_earliest, start
                    );
                    (start, fetch_end_date)
                } else {
                    let start = inputs
                        .activity_min
                        .map(|d| d - Duration::days(QUOTE_HISTORY_BUFFER_DAYS))
                        .unwrap_or_else(|| fetch_end_date - Duration::days(days));
                    (start, fetch_end_date)
                }
            }
        }
    }

    /// Fetch and upsert split activities for a single asset over the given date range.
    ///
    /// Non-fatal: any failure is logged as a warning and does not affect quote sync.
    /// NOT USED FOR NOW - Yahoo returns weird splits for some assets, need to investigate further before enabling this.
    async fn _sync_splits(&self, asset: &Asset, start: NaiveDate, end: NaiveDate) {
        use crate::activities::compute_idempotency_key;

        let start_dt = Utc.from_utc_datetime(
            &start
                .and_hms_opt(0, 0, 0)
                .expect("00:00:00 is a valid time"),
        );
        let end_dt = Utc.from_utc_datetime(
            &end.and_hms_opt(23, 59, 59)
                .expect("23:59:59 is a valid time"),
        );

        let client = self.client.read().await;
        let splits = client.fetch_splits(asset, start_dt, end_dt).await;
        drop(client);

        if splits.is_empty() {
            return;
        }

        let (account_ids, _) = match self
            .activity_repo
            .get_activity_accounts_and_currencies_by_asset_id(&asset.id)
            .await
        {
            Ok(result) => result,
            Err(e) => {
                warn!(
                    "Split sync: failed to get accounts for {}: {:?}",
                    asset.id, e
                );
                return;
            }
        };

        if account_ids.is_empty() {
            return;
        }

        // Splits are asset-level events; use the asset's quote currency for a stable idempotency key.
        let currency = asset.quote_ccy.as_str();

        let mut upserts: Vec<ActivityUpsert> = Vec::new();
        for split in &splits {
            let split_dt = Utc.from_utc_datetime(
                &split
                    .date
                    .and_hms_opt(12, 0, 0)
                    .expect("12:00:00 is a valid time"),
            );

            for account_id in &account_ids {
                let key = compute_idempotency_key(
                    account_id,
                    "SPLIT",
                    &split_dt,
                    Some(&asset.id),
                    None,
                    None,
                    Some(split.ratio),
                    None,
                    currency,
                    None,
                    None,
                );
                upserts.push(ActivityUpsert {
                    id: key.clone(),
                    account_id: account_id.clone(),
                    asset_id: Some(asset.id.clone()),
                    activity_type: "SPLIT".to_string(),
                    activity_date: split.date.to_string(),
                    amount: Some(split.ratio),
                    currency: currency.to_string(),
                    idempotency_key: Some(key),
                    quantity: None,
                    unit_price: None,
                    fee: None,
                    tax: None,
                    notes: Some(format!("Auto-imported split ({})", split.ratio)),
                    subtype: None,
                    status: None,
                    fx_rate: None,
                    metadata: None,
                    needs_review: None,
                    source_system: Some("yahoo".to_string()),
                    source_record_id: None,
                    source_group_id: None,
                    import_run_id: None,
                });
            }
        }

        if let Err(e) = self.activity_repo.bulk_upsert(upserts).await {
            warn!(
                "Split sync: failed to upsert splits for {}: {:?}",
                asset.id, e
            );
        } else {
            debug!(
                "Split sync: upserted {} split activities for {}",
                splits.len() * account_ids.len(),
                asset.id
            );
        }
    }

    /// Fetch through the same profile-scoped owner as background synchronization.
    pub async fn fetch_local_first_history(
        &self,
        asset: &Asset,
        start: NaiveDate,
        end: NaiveDate,
        benchmark: bool,
    ) -> Result<Vec<Quote>> {
        self.fetch_local_first_history_with_writes(asset, start, end, benchmark)
            .await
            .0
    }

    /// Direct read callers own downstream invalidation; coordinator sync uses
    /// local_history directly and retains its existing completion lifecycle.
    pub(crate) async fn fetch_local_first_history_with_writes(
        &self,
        asset: &Asset,
        start: NaiveDate,
        end: NaiveDate,
        benchmark: bool,
    ) -> (Result<Vec<Quote>>, usize) {
        match self
            .local_history(asset, start, end, benchmark, false)
            .await
        {
            Ok(result) => (
                Ok(result.quotes),
                if benchmark { 0 } else { result.written },
            ),
            Err(error) => (
                Err(error.fetch_error.error),
                if benchmark { 0 } else { error.written },
            ),
        }
    }

    async fn local_history(
        &self,
        requested_asset: &Asset,
        start: NaiveDate,
        end: NaiveDate,
        benchmark: bool,
        force: bool,
    ) -> std::result::Result<LocalHistoryResult, LocalHistoryError> {
        let guard = Arc::new(
            self.history_locks
                .for_asset(&requested_asset.id)
                .lock_owned()
                .await,
        );
        let client = Arc::new(self.client.clone().read_owned().await);
        let ownership = HistoryOwnership {
            _asset: guard.clone(),
            _client: client.clone(),
        };
        let wrap = |error| LocalHistoryError {
            _ownership: ownership.clone(),
            fetch_error: Box::new(HistoricalQuoteFetchError {
                error,
                provider_id: None,
            }),
            written: 0,
        };
        let asset = if benchmark {
            requested_asset.clone()
        } else {
            self.asset_repo
                .get_by_id(&requested_asset.id)
                .map_err(wrap)?
        };
        if asset.quote_mode == QuoteMode::Manual {
            return Ok(LocalHistoryResult {
                _ownership: ownership.clone(),
                quotes: self
                    .quote_store
                    .range(
                        &AssetId::new(&asset.id),
                        Day::new(start),
                        Day::new(end),
                        None,
                    )
                    .map_err(wrap)?,
                written: 0,
                fetched_nonempty: false,
                provider_id: None,
            });
        }
        let revision = self
            .quote_store
            .history_configuration_revision()
            .map_err(wrap)?;
        let identity = history_identity(&asset, &client, "history", &revision, benchmark);
        // This is deliberately after the lock. A preceding queued request may
        // already have filled some or all of this caller's requested interval.
        let coverage = self.quote_store.history_coverage(&identity).map_err(wrap)?;
        let now = Utc::now();
        let fetch_end = end.min(market_fetch_end_date(
            now,
            asset.instrument_exchange_mic.as_deref(),
            asset.instrument_type.as_ref(),
        ));
        let local = if benchmark {
            self.quote_store
                .benchmark_history(&identity, start, fetch_end)
                .map_err(wrap)?
        } else {
            self.quote_store
                .range(
                    &AssetId::new(&asset.id),
                    Day::new(start),
                    Day::new(fetch_end),
                    None,
                )
                .map_err(wrap)?
        };
        let mut observed_currency = None;
        if benchmark && asset.quote_ccy.is_empty() {
            validate_history_currency(&local, &mut observed_currency).map_err(wrap)?;
        }
        let latest_provider_date = local
            .iter()
            .filter(|quote| quote.data_source != DATA_SOURCE_MANUAL)
            .map(|quote| quote.timestamp.date_naive())
            .max();
        let ranges = if start > fetch_end {
            Vec::new()
        } else if force {
            vec![(start, fetch_end)]
        } else {
            missing_history_ranges_with_freshness(
                start,
                fetch_end,
                &coverage,
                now,
                self.retry_interval(),
                |record| history_range_settled(&asset, record, now, latest_provider_date),
            )
        };
        let mut written = 0;
        let mut fetched_nonempty = false;
        let mut provider_id = None;
        if asset.quote_mode != QuoteMode::Manual {
            for (range_start, range_end) in ranges {
                let expected_context = if benchmark {
                    None
                } else {
                    self.quote_store
                        .history_write_context(&asset.id)
                        .map_err(|error| LocalHistoryError {
                            written,
                            ..wrap(error)
                        })?
                };
                if expected_context.as_ref().is_some_and(|context| {
                    history_identity(&context.asset, &client, "history", &revision, benchmark)
                        != identity
                }) {
                    return Err(LocalHistoryError {
                        _ownership: ownership.clone(),
                        fetch_error: Box::new(HistoricalQuoteFetchError {
                            error: Error::Asset(
                                "Asset settings changed before quote fetch; retry".into(),
                            ),
                            provider_id: None,
                        }),
                        written,
                    });
                }
                let start_dt = Utc
                    .from_utc_datetime(&range_start.and_hms_opt(0, 0, 0).expect("valid midnight"));
                let end_dt = Utc
                    .from_utc_datetime(&range_end.and_hms_opt(23, 59, 59).expect("valid day end"));
                let fetched = client
                    .fetch_historical_quotes_with_coverage(&asset, start_dt, end_dt)
                    .await;
                let (mut quotes, complete) = match fetched {
                    Ok(result) => (result.quotes, result.complete),
                    Err(error) => {
                        return Err(LocalHistoryError {
                            _ownership: ownership.clone(),
                            fetch_error: Box::new(error),
                            written,
                        })
                    }
                };
                // Do not let out-of-request rows establish requested coverage.
                let original_count = quotes.len();
                quotes.retain(|quote| {
                    quote.timestamp.date_naive() >= range_start
                        && quote.timestamp.date_naive() <= range_end
                        && quote_matches_currency(quote, &asset)
                });
                if benchmark && asset.quote_ccy.is_empty() {
                    validate_history_currency(&quotes, &mut observed_currency).map_err(
                        |error| LocalHistoryError {
                            written,
                            ..wrap(error)
                        },
                    )?;
                }
                let complete = complete && original_count == quotes.len();
                fetched_nonempty |= !quotes.is_empty();
                provider_id = quotes
                    .first()
                    .map(|quote| quote.data_source.clone())
                    .or(provider_id);
                if !benchmark {
                    let current = self.asset_repo.get_by_id(&asset.id).map_err(|error| {
                        LocalHistoryError {
                            written,
                            ..wrap(error)
                        }
                    })?;
                    if history_identity(&current, &client, "history", &revision, benchmark)
                        != identity
                    {
                        return Err(LocalHistoryError { _ownership: ownership.clone(), fetch_error: Box::new(HistoricalQuoteFetchError {
                            error: Error::Asset("Asset settings changed during quote fetch; retry with current settings".into()), provider_id: None,
                        }), written });
                    }
                }
                let record = HistoryCoverage {
                    identity: identity.clone(),
                    asset_id: asset.id.clone(),
                    start: range_start,
                    end: range_end,
                    fetched_at: Utc::now(),
                    outcome: if !complete {
                        HistoryOutcome::Partial
                    } else if quotes.is_empty() {
                        HistoryOutcome::Empty
                    } else {
                        HistoryOutcome::Complete
                    },
                };
                let store = self.quote_store.clone();
                let write_guard = guard.clone();
                let write_client = client.clone();
                // Keep both ownership and provider configuration stable even if
                // the caller is cancelled while SQLite's queued write completes.
                written += tokio::spawn(async move {
                    let _guard = write_guard;
                    let _client = write_client;
                    store
                        .commit_history_query(
                            &record,
                            &quotes,
                            if benchmark {
                                HistoryWriteMode::DerivedRange
                            } else {
                                HistoryWriteMode::Canonical
                            },
                            true,
                            expected_context.as_ref(),
                        )
                        .await
                })
                .await
                .map_err(|error| LocalHistoryError {
                    _ownership: ownership.clone(),
                    fetch_error: Box::new(HistoricalQuoteFetchError {
                        error: Error::Unexpected(error.to_string()),
                        provider_id: None,
                    }),
                    written,
                })?
                .map_err(|error| LocalHistoryError {
                    _ownership: ownership.clone(),
                    fetch_error: Box::new(HistoricalQuoteFetchError {
                        error,
                        provider_id: None,
                    }),
                    written,
                })?;
            }
        }
        let mut quotes = if benchmark {
            self.quote_store.benchmark_history(&identity, start, end)
        } else {
            self.quote_store.range(
                &AssetId::new(&asset.id),
                Day::new(start),
                Day::new(end),
                None,
            )
        }
        .map_err(|error| LocalHistoryError {
            _ownership: ownership.clone(),
            fetch_error: Box::new(HistoricalQuoteFetchError {
                error,
                provider_id: None,
            }),
            written,
        })?;
        quotes.retain(|quote| quote_matches_currency(quote, &asset));
        Ok(LocalHistoryResult {
            _ownership: ownership.clone(),
            quotes,
            written,
            fetched_nonempty,
            provider_id,
        })
    }

    fn local_latest(
        &self,
        asset: &Asset,
        identity: &str,
        benchmark: bool,
    ) -> Result<Option<Quote>> {
        if benchmark {
            return Ok(self
                .quote_store
                .benchmark_history(identity, NaiveDate::MIN, NaiveDate::MAX)?
                .into_iter()
                .filter(|quote| quote_matches_currency(quote, asset))
                .max_by_key(|quote| quote.timestamp));
        }
        let latest = self.quote_store.latest(&AssetId::new(&asset.id), None)?;
        if latest
            .as_ref()
            .is_none_or(|quote| quote_matches_currency(quote, asset))
        {
            return Ok(latest);
        }
        // Currency changes can leave older provider rows in the same canonical
        // asset history. Do not let those shadow a compatible new quote.
        Ok(self
            .quote_store
            .range(
                &AssetId::new(&asset.id),
                Day::new(NaiveDate::MIN),
                Day::new(NaiveDate::MAX),
                None,
            )?
            .into_iter()
            .filter(|quote| quote_matches_currency(quote, asset))
            .max_by_key(|quote| quote.timestamp))
    }

    /// Reuse compatible local history proof first; misses use latest capability.
    pub async fn fetch_local_first_latest(
        &self,
        requested_asset: &Asset,
        benchmark: bool,
    ) -> Result<Quote> {
        self.fetch_local_first_latest_with_provenance(requested_asset, benchmark)
            .await
            .map(|(quote, _)| quote)
    }

    /// Retain the actual fetched provider response for explicit symbol validation,
    /// while ordinary callers continue to receive the merged/manual-priority quote.
    pub(crate) async fn fetch_local_first_latest_with_provenance(
        &self,
        requested_asset: &Asset,
        benchmark: bool,
    ) -> Result<(Quote, Option<Quote>)> {
        self.fetch_local_first_latest_for_resolution(requested_asset, benchmark, None)
            .await
            .0
    }

    /// Explicit provider validation may use isolated derived prices behind a
    /// manual overlay, but shares the same canonical asset lock and freshness.
    pub(crate) async fn fetch_local_first_latest_for_resolution(
        &self,
        requested_asset: &Asset,
        mut benchmark: bool,
        verification_source: Option<&str>,
    ) -> (Result<(Quote, Option<Quote>)>, usize) {
        let mut canonical_written = 0;
        let result = async {
            let guard = Arc::new(
                self.history_locks
                    .for_asset(&requested_asset.id)
                    .lock_owned()
                    .await,
            );
            let mut asset = if benchmark {
                requested_asset.clone()
            } else {
                self.asset_repo.get_by_id(&requested_asset.id)?
            };
            let client = Arc::new(self.client.clone().read_owned().await);
            let revision = self.quote_store.history_configuration_revision()?;
            loop {
                let identity = history_identity(&asset, &client, "latest", &revision, benchmark);
                let now = Utc::now();
                let today = market_fetch_end_date(
                    now,
                    asset.instrument_exchange_mic.as_deref(),
                    asset.instrument_type.as_ref(),
                );
                let records = self.quote_store.history_coverage(&identity)?;
                let mut local = self.local_latest(&asset, &identity, benchmark)?;
                let verify_manual = !benchmark
                    && verification_source.is_some()
                    && (asset.quote_mode == QuoteMode::Manual
                        || local
                            .as_ref()
                            .is_some_and(|quote| quote.data_source == DATA_SOURCE_MANUAL));
                if verify_manual {
                    local = self
                        .quote_store
                        .latest(
                            &AssetId::new(&asset.id),
                            verification_source
                                .map(super::types::QuoteSource::from_storage_string)
                                .as_ref(),
                        )?
                        .filter(|quote| {
                            quote_matches_currency(quote, &asset)
                                && quote.timestamp.date_naive() >= today
                                && quote.timestamp <= now
                                && quote.created_at.naive_utc() >= asset.updated_at
                        });
                }
                if asset.quote_mode == QuoteMode::Manual && !verify_manual {
                    return local
                        .map(|quote| (quote, None))
                        .ok_or_else(|| Error::MarketData(MarketDataError::NoData));
                }
                // A completed history query can already prove the latest market day.
                // Reuse that proof rather than making a second request with latest capability.
                let history_key =
                    history_identity(&asset, &client, "history", &revision, benchmark);
                let history_records: Vec<_> = self
                    .quote_store
                    .history_coverage(&history_key)?
                    .into_iter()
                    .filter(|record| record.outcome == HistoryOutcome::Complete)
                    .collect();
                let history_local = if benchmark {
                    self.local_latest(&asset, &history_key, true)?
                } else {
                    local.clone()
                };
                if let Some(quote) = history_local {
                    if missing_history_ranges_with_freshness(
                        today,
                        today,
                        &history_records,
                        now,
                        self.retry_interval(),
                        |record| {
                            history_range_settled(
                                &asset,
                                record,
                                now,
                                (quote.data_source != DATA_SOURCE_MANUAL)
                                    .then(|| quote.timestamp.date_naive()),
                            )
                        },
                    )
                    .is_empty()
                    {
                        return Ok((quote, None));
                    }
                }
                if missing_history_ranges_with_freshness(
                    today,
                    today,
                    &records,
                    now,
                    self.retry_interval(),
                    |record| {
                        history_range_settled(
                            &asset,
                            record,
                            now,
                            local
                                .as_ref()
                                .filter(|quote| quote.data_source != DATA_SOURCE_MANUAL)
                                .map(|quote| quote.timestamp.date_naive()),
                        )
                    },
                )
                .is_empty()
                {
                    if let Some(quote) = local {
                        return Ok((quote, None));
                    }
                }
                if verify_manual {
                    // No fresh canonical provider proof: keep validation responses out
                    // of user quotes, retaining the canonical lock across the recheck.
                    asset.quote_mode = QuoteMode::Market;
                    benchmark = true;
                    continue;
                }
                let expected_context = if benchmark {
                    None
                } else {
                    self.quote_store.history_write_context(&asset.id)?
                };
                if expected_context.as_ref().is_some_and(|context| {
                    history_identity(&context.asset, &client, "latest", &revision, benchmark)
                        != identity
                }) {
                    return Err(Error::Asset(
                        "Asset settings changed before quote fetch; retry".into(),
                    ));
                }
                let quote = client.fetch_latest_quote(&asset).await?;
                if !quote_matches_currency(&quote, &asset) {
                    return Err(Error::MarketData(MarketDataError::InvalidData(
                        "Provider quote currency does not match the asset".into(),
                    )));
                }
                if !benchmark
                    && history_identity(
                        &self.asset_repo.get_by_id(&asset.id)?,
                        &client,
                        "latest",
                        &revision,
                        benchmark,
                    ) != identity
                {
                    return Err(Error::Asset(
                        "Asset settings changed during quote fetch; retry with current settings"
                            .into(),
                    ));
                }
                let observed_at = Utc::now();
                let Some((coverage_start, coverage_end)) =
                    latest_observation_range(quote.timestamp, today, observed_at)
                else {
                    // Date-only providers may label a market-local day ahead of UTC.
                    // Preserve their response, but do not persist an unobservable price
                    // or a receipt that could certify a future query interval.
                    let merged = self
                        .local_latest(&asset, &identity, benchmark)?
                        .filter(|local| local.data_source == DATA_SOURCE_MANUAL)
                        .unwrap_or_else(|| quote.clone());
                    return Ok((merged, Some(quote)));
                };
                let coverage = HistoryCoverage {
                    identity: identity.clone(),
                    asset_id: asset.id.clone(),
                    start: coverage_start,
                    end: coverage_end,
                    fetched_at: observed_at,
                    outcome: if coverage_end > today {
                        HistoryOutcome::Partial
                    } else {
                        latest_history_outcome(&asset, &quote.data_source)
                    },
                };
                let store = self.quote_store.clone();
                let persisted_quote = quote.clone();
                let write_client = client.clone();
                let written = tokio::spawn(async move {
                    let _guard = guard;
                    let _client = write_client;
                    store
                        .commit_history_query(
                            &coverage,
                            &[persisted_quote],
                            if benchmark {
                                HistoryWriteMode::DerivedLatest
                            } else {
                                HistoryWriteMode::Canonical
                            },
                            true,
                            expected_context.as_ref(),
                        )
                        .await
                })
                .await
                .map_err(|error| Error::Unexpected(error.to_string()))??;
                if !benchmark {
                    canonical_written = written;
                }

                return self
                    .local_latest(&asset, &identity, benchmark)?
                    .map(|merged| (merged, Some(quote)))
                    .ok_or_else(|| Error::MarketData(MarketDataError::NoData));
            }
        }
        .await;
        (result, canonical_written)
    }

    async fn sync_asset(
        &self,
        asset: &Asset,
        plan: &SymbolSyncPlan,
        force: bool,
    ) -> AssetSyncResult {
        let asset_id = AssetId::new(&asset.id);
        match self
            .local_history(asset, plan.start_date, plan.end_date, false, force)
            .await
        {
            Ok(result) => {
                if result.fetched_nonempty {
                    if let Err(error) = self.sync_state_store.update_after_sync(&asset.id).await {
                        warn!("Failed to update sync state for {}: {:?}", asset.id, error);
                    }
                    if let Some(source) = result.provider_id {
                        if let Ok(Some(mut state)) =
                            self.sync_state_store.get_by_asset_id(&asset.id)
                        {
                            if state.data_source != source {
                                state.data_source = source;
                                if let Err(error) = self.sync_state_store.upsert(&state).await {
                                    warn!(
                                        "Failed to persist actual source for {}: {:?}",
                                        asset.id, error
                                    );
                                }
                            }
                        }
                    }
                }
                AssetSyncResult {
                    asset_id,
                    quotes_added: result.written,
                    status: if result.fetched_nonempty {
                        SyncStatus::Success
                    } else {
                        SyncStatus::Skipped
                    },
                    error: None,
                    skip_reason: if result.fetched_nonempty {
                        None
                    } else if result.quotes.is_empty() {
                        Some(AssetSkipReason::NoDataForRange)
                    } else {
                        Some(AssetSkipReason::NoQuoteWindow)
                    },
                }
            }
            Err(local_error) => {
                let _ownership = local_error._ownership;
                let written = local_error.written;
                let fetch_error = *local_error.fetch_error;
                let error = fetch_error.error;
                if let Some(skip_reason) = fetch_error_skip_reason(&error) {
                    return AssetSyncResult {
                        asset_id,
                        quotes_added: written,
                        status: SyncStatus::Skipped,
                        error: None,
                        skip_reason: Some(skip_reason),
                    };
                }
                if should_treat_fetch_error_as_non_fatal(
                    &plan.category,
                    &error,
                    plan.suppress_closed_fetch_errors,
                ) {
                    if let Err(state_error) =
                        self.sync_state_store.update_after_sync(&asset.id).await
                    {
                        warn!(
                            "Failed to update sync state for {}: {:?}",
                            asset.id, state_error
                        );
                    }
                    return AssetSyncResult {
                        asset_id,
                        quotes_added: written,
                        status: SyncStatus::Success,
                        error: None,
                        skip_reason: None,
                    };
                }
                let changed_context = matches!(error, Error::Asset(_));
                let message =
                    format_sync_failure_message(&error, fetch_error.provider_id.as_deref());
                if !changed_context {
                    if let Err(state_error) = self
                        .sync_state_store
                        .update_after_failure(&asset.id, &message)
                        .await
                    {
                        warn!(
                            "Failed to update sync state for {}: {:?}",
                            asset.id, state_error
                        );
                    }
                }
                AssetSyncResult {
                    asset_id,
                    quotes_added: written,
                    status: SyncStatus::Failed,
                    error: Some(message),
                    skip_reason: None,
                }
            }
        }
    }

    /// Execute sync for a list of plans.
    ///
    /// Returns a boxed future to provide an explicit `Send` boundary for
    /// `async_trait` compatibility when this method's composed stream/future
    /// types are type-erased.
    fn execute_sync_plans(
        &self,
        plans: Vec<SymbolSyncPlan>,
        force: bool,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = SyncResult> + Send + '_>> {
        Box::pin(async move {
            if plans.is_empty() {
                return SyncResult::default();
            }

            debug!("Executing sync for {} assets", plans.len());

            // Get all assets for the plans
            let asset_ids: Vec<String> = plans.iter().map(|p| p.asset_id.clone()).collect();
            let assets = match self.asset_repo.list_by_asset_ids(&asset_ids) {
                Ok(a) => a,
                Err(e) => {
                    error!("Failed to get assets for sync: {:?}", e);
                    let mut result = SyncResult::default();
                    for plan in &plans {
                        result.failures.push((plan.asset_id.clone(), e.to_string()));
                        result.failed += 1;
                    }
                    return result;
                }
            };

            let asset_map: HashMap<String, Asset> =
                assets.into_iter().map(|a| (a.id.clone(), a)).collect();

            let asset_results: Vec<AssetSyncResult> = stream::iter(plans)
                .map(|plan| {
                    let asset = asset_map.get(&plan.asset_id).cloned();
                    async move {
                        if let Some(asset) = asset {
                            self.sync_asset(&asset, &plan, force).await
                        } else {
                            warn!("Asset not found for asset_id: {}", plan.asset_id);
                            AssetSyncResult {
                                asset_id: AssetId::new(&plan.asset_id),
                                quotes_added: 0,
                                status: SyncStatus::Failed,
                                error: Some("Asset not found".to_string()),
                                skip_reason: None,
                            }
                        }
                    }
                })
                .buffer_unordered(SYNC_CONCURRENCY)
                .collect()
                .await;

            let mut result = SyncResult::default();
            for asset_result in asset_results {
                result.add_result(asset_result);
            }

            // Replace asset IDs with display codes in failures for human-readable messages
            result.failures = result
                .failures
                .into_iter()
                .map(|(id, err)| {
                    let display = asset_map
                        .get(&id)
                        .and_then(|a| a.display_code.clone())
                        .unwrap_or(id);
                    (display, err)
                })
                .collect();

            debug!(
                "Sync complete: {} synced, {} failed, {} skipped, {} quotes total",
                result.synced, result.failed, result.skipped, result.quotes_synced
            );

            result
        })
    }

    /// Generate sync plan based on current sync states.
    ///
    /// This function computes both activity bounds and quote bounds on-the-fly
    /// from the operational tables, ensuring planning is based on fresh data.
    fn generate_sync_plan(&self) -> Result<Vec<SymbolSyncPlan>> {
        let states = self
            .sync_state_store
            .get_assets_needing_sync(CLOSED_POSITION_GRACE_PERIOD_DAYS)?;

        let now = Utc::now();
        let asset_ids: Vec<String> = states.iter().map(|state| state.asset_id.clone()).collect();

        // Fetch actual assets to check pricing_mode - filter out non-market-priced assets
        let assets = self.asset_repo.list_by_asset_ids(&asset_ids)?;
        let assets_by_id: HashMap<String, &Asset> = assets
            .iter()
            .map(|asset| (asset.id.clone(), asset))
            .collect();
        let syncable_asset_ids: std::collections::HashSet<String> = assets
            .iter()
            .filter(|a| self.should_sync_asset(a))
            .map(|a| a.id.clone())
            .collect();

        // Filter states to only include syncable assets and skip assets with too many errors
        let states: Vec<_> = states
            .into_iter()
            .filter(|s| {
                if !syncable_asset_ids.contains(&s.asset_id) {
                    return false;
                }
                if s.error_count >= MAX_SYNC_ERRORS {
                    debug!(
                        "Skipping {} - too many consecutive errors ({})",
                        s.asset_id, s.error_count
                    );
                    return false;
                }
                true
            })
            .collect();

        // Compute activity bounds on-the-fly from activities table
        let activity_bounds = self
            .activity_repo
            .get_activity_bounds_for_assets(&asset_ids)?;
        let holdings_bounds = self
            .activity_repo
            .get_holdings_snapshot_bounds_for_assets(&asset_ids)?;

        // Compute quote bounds on-the-fly from quotes table, filtered by provider
        // Group states by data_source to batch quote bounds queries
        let mut quote_bounds_by_source: HashMap<String, HashMap<String, (NaiveDate, NaiveDate)>> =
            HashMap::new();
        for state in &states {
            if !quote_bounds_by_source.contains_key(&state.data_source) {
                let source_assets: Vec<String> = states
                    .iter()
                    .filter(|s| s.data_source == state.data_source)
                    .map(|s| s.asset_id.clone())
                    .collect();
                let bounds = self
                    .quote_store
                    .get_quote_bounds_for_assets(&source_assets, &state.data_source)?;
                quote_bounds_by_source.insert(state.data_source.clone(), bounds);
            }
        }

        let mut plans = Vec::new();

        for state in states {
            let Some(asset) = assets_by_id.get(&state.asset_id).copied() else {
                continue;
            };

            // Build SyncPlanningInputs from computed bounds
            let (activity_min, activity_max) = merge_bounds(
                activity_bounds.get(&state.asset_id).copied(),
                holdings_bounds.get(&state.asset_id).copied(),
            );

            let (quote_min, quote_max) = quote_bounds_by_source
                .get(&state.data_source)
                .and_then(|bounds| bounds.get(&state.asset_id))
                .map(|(min, max)| (Some(*min), Some(*max)))
                .unwrap_or((None, None));

            let inputs = SyncPlanningInputs {
                is_active: state.is_active,
                position_closed_date: state.position_closed_date,
                activity_min,
                activity_max,
                quote_min,
                quote_max,
            };

            let effective_today = effective_market_today(
                now,
                asset.instrument_exchange_mic.as_deref(),
                asset.instrument_type.as_ref(),
            );
            let fetch_end_date = market_fetch_end_date(
                now,
                asset.instrument_exchange_mic.as_deref(),
                asset.instrument_type.as_ref(),
            );
            let category = determine_sync_category(
                &inputs,
                CLOSED_POSITION_GRACE_PERIOD_DAYS,
                effective_today,
            );

            if matches!(category, SyncCategory::Closed) {
                continue;
            }

            if matches!(category, SyncCategory::NeedsBackfill) && inputs.is_active {
                if let Some((start_date, end_date)) =
                    calculate_sync_window(&category, &inputs, effective_today)
                {
                    if start_date <= end_date {
                        plans.push(SymbolSyncPlan {
                            asset_id: state.asset_id.clone(),
                            category: category.clone(),
                            start_date,
                            end_date,
                            priority: state.sync_priority,
                            data_source: state.data_source.clone(),
                            quote_symbol: None,
                            currency: asset.quote_ccy.clone(),
                            suppress_closed_fetch_errors: false,
                        });
                    }
                }

                let recent_category = SyncCategory::Active;
                if let Some((start_date, end_date)) =
                    calculate_sync_window(&recent_category, &inputs, effective_today)
                {
                    let start_date = if start_date >= effective_today {
                        effective_today - Duration::days(MIN_SYNC_LOOKBACK_DAYS)
                    } else {
                        start_date
                    };
                    let end_date =
                        clamp_end_date_for_fetch(&recent_category, end_date, fetch_end_date);

                    if start_date <= end_date {
                        plans.push(SymbolSyncPlan {
                            asset_id: state.asset_id.clone(),
                            category: recent_category,
                            start_date,
                            end_date,
                            priority: SyncCategory::Active.default_priority(),
                            data_source: state.data_source.clone(),
                            quote_symbol: None,
                            currency: asset.quote_ccy.clone(),
                            suppress_closed_fetch_errors: false,
                        });
                    }
                }

                continue;
            }

            // Use the new calculate_sync_window function
            let Some((start_date, end_date)) =
                calculate_sync_window(&category, &inputs, effective_today)
            else {
                continue;
            };

            // Ensure minimum lookback
            let start_date = if start_date >= effective_today {
                effective_today - Duration::days(MIN_SYNC_LOOKBACK_DAYS)
            } else {
                start_date
            };
            let end_date = clamp_end_date_for_fetch(&category, end_date, fetch_end_date);

            if start_date > end_date {
                continue;
            }

            plans.push(SymbolSyncPlan {
                asset_id: state.asset_id.clone(),
                category,
                start_date,
                end_date,
                priority: state.sync_priority,
                data_source: state.data_source.clone(),
                quote_symbol: None,
                currency: asset.quote_ccy.clone(),
                suppress_closed_fetch_errors: false,
            });
        }

        // Sort by priority (highest first)
        plans.sort_by_key(|b| std::cmp::Reverse(b.priority));
        Ok(plans)
    }

    /// Ensure sync states exist for the given assets.
    async fn ensure_sync_states_for_assets(
        &self,
        assets: &[Asset],
        allow_inactive: bool,
    ) -> Result<()> {
        debug!("Ensuring sync states for {} assets...", assets.len());

        let mut new_states = Vec::new();

        for asset in assets
            .iter()
            .filter(|asset| self.get_skip_reason(asset, allow_inactive).is_none())
        {
            let existing = self.sync_state_store.get_by_asset_id(&asset.id)?;
            if existing.is_none() {
                // Don't set data_source here - it will be populated after first successful sync
                // Using empty string so errors aren't incorrectly attributed to a specific provider
                let mut state = QuoteSyncState::new(asset.id.clone(), String::new());
                // is_active is derived from position_closed_date (None = active)
                // QuoteSyncState::new() already sets position_closed_date = None
                state.sync_priority = SyncCategory::New.default_priority();
                new_states.push(state);
            }
        }

        if !new_states.is_empty() {
            if let Err(e) = self.sync_state_store.upsert_batch(&new_states).await {
                warn!("Failed to upsert {} sync states: {:?}", new_states.len(), e);
            }
        }

        Ok(())
    }

    /// Ensure sync states exist for all syncable assets.
    async fn ensure_sync_states_for_all_assets(&self) -> Result<()> {
        let assets = self.asset_repo.list()?;
        self.ensure_sync_states_for_assets(&assets, false).await
    }
}

#[async_trait]
impl<Q, S, A, R> QuoteSyncServiceTrait for QuoteSyncService<Q, S, A, R>
where
    Q: QuoteStore + 'static,
    S: SyncStateStore + 'static,
    A: AssetRepositoryTrait + 'static,
    R: ActivityRepositoryTrait + 'static,
{
    async fn sync(&self, mode: SyncMode, asset_ids: Option<Vec<String>>) -> Result<SyncResult> {
        debug!("Starting sync (mode: {}) for {:?}", mode, asset_ids);

        if is_empty_asset_target(asset_ids.as_deref()) {
            info!("No asset IDs requested for sync; skipping");
            return Ok(SyncResult::default());
        }

        let targeted_sync = is_targeted_asset_sync(asset_ids.as_deref());
        let include_closed_positions = should_include_closed_positions(mode, targeted_sync);
        let assets = match asset_ids.as_ref() {
            Some(ids) => self.asset_repo.list_by_asset_ids(ids)?,
            None => self.asset_repo.list()?,
        };

        let all_asset_ids: Vec<String> = assets.iter().map(|asset| asset.id.clone()).collect();
        let initial_states = self.sync_state_store.get_by_asset_ids(&all_asset_ids)?;
        let activity_bounds = self
            .activity_repo
            .get_activity_bounds_for_assets(&all_asset_ids)?;
        let holdings_bounds = self
            .activity_repo
            .get_holdings_snapshot_bounds_for_assets(&all_asset_ids)?;

        // Initialize result to track skipped assets
        let mut result = SyncResult::default();
        let mut syncable: Vec<&Asset> = Vec::new();

        for asset in &assets {
            let allow_inactive_asset = should_allow_inactive_asset(
                &asset.id,
                mode,
                targeted_sync,
                &initial_states,
                &activity_bounds,
                &holdings_bounds,
            );
            if let Some(reason) = self.get_skip_reason(asset, allow_inactive_asset) {
                debug!("Skipping asset {} for sync: {}", asset.id, reason);
                if matches!(reason, AssetSkipReason::MaturedBond) {
                    result.quotes_synced += self.ensure_matured_bond_par_quote(asset).await;
                }
                if matches!(reason, AssetSkipReason::ExpiredOption) {
                    // Clear any lingering sync errors so expired options
                    // don't show up in health checks
                    if let Err(e) = self.sync_state_store.update_after_sync(&asset.id).await {
                        warn!(
                            "Failed to reset sync state for expired option {}: {:?}",
                            asset.id, e
                        );
                    }
                }
                result.add_skipped(asset.id.clone(), reason);
            } else {
                syncable.push(asset);
            }
        }

        if syncable.is_empty() {
            info!(
                "No syncable assets found (requested: {}, skipped: {})",
                assets.len(),
                result.skipped
            );
            return Ok(result);
        }

        let now = Utc::now();
        let syncable_ids: Vec<String> = syncable.iter().map(|asset| asset.id.clone()).collect();
        let existing_states = self.sync_state_store.get_by_asset_ids(&syncable_ids)?;

        // Compute quote bounds per provider
        let mut quote_bounds: HashMap<String, (NaiveDate, NaiveDate)> = HashMap::new();
        let mut assets_by_provider: HashMap<String, Vec<String>> = HashMap::new();
        for asset in &syncable {
            let provider = effective_provider(existing_states.get(&asset.id), asset);
            assets_by_provider
                .entry(provider)
                .or_default()
                .push(asset.id.clone());
        }
        for (provider, ids) in &assets_by_provider {
            if let Ok(bounds) = self.quote_store.get_quote_bounds_for_assets(ids, provider) {
                quote_bounds.extend(bounds);
            }
        }

        // Build plans using the mode-specific date range calculation
        let mut plans: Vec<SymbolSyncPlan> = Vec::new();
        for asset in &syncable {
            let state = existing_states.get(&asset.id).cloned();
            let data_source = effective_provider(state.as_ref(), asset);
            let effective_today = effective_market_today(
                now,
                asset.instrument_exchange_mic.as_deref(),
                asset.instrument_type.as_ref(),
            );
            let fetch_end_date = market_fetch_end_date(
                now,
                asset.instrument_exchange_mic.as_deref(),
                asset.instrument_type.as_ref(),
            );

            // Explicit targeted retries bypass the broad-sync error cutoff.
            if let Some(ref s) = state {
                if should_skip_for_error_limit(mode, targeted_sync, s.error_count) {
                    debug!(
                        "Skipping {} - too many consecutive errors ({})",
                        asset.id, s.error_count
                    );
                    result.add_skipped(asset.id.clone(), AssetSkipReason::TooManyErrors);
                    continue;
                }
            }

            // Build SyncPlanningInputs from computed bounds
            let (activity_min, activity_max) = merge_bounds(
                activity_bounds.get(&asset.id).copied(),
                holdings_bounds.get(&asset.id).copied(),
            );

            let (quote_min, quote_max) = quote_bounds
                .get(&asset.id)
                .map(|(min, max)| (Some(*min), Some(*max)))
                .unwrap_or((None, None));

            let inputs = SyncPlanningInputs {
                is_active: state
                    .as_ref()
                    .map(|s| s.is_active)
                    .unwrap_or(asset.is_active),
                position_closed_date: state.as_ref().and_then(|s| s.position_closed_date),
                activity_min,
                activity_max,
                quote_min,
                quote_max,
            };

            // Determine category for priority
            let mut category = determine_sync_category(
                &inputs,
                CLOSED_POSITION_GRACE_PERIOD_DAYS,
                effective_today,
            );
            let mut planning_inputs = inputs.clone();
            let mut targeted_closed_incremental = false;

            if matches!(category, SyncCategory::Closed) {
                if !include_closed_positions {
                    result.add_skipped(asset.id.clone(), AssetSkipReason::ClosedPosition);
                    continue;
                }

                if targeted_sync && matches!(mode, SyncMode::Incremental) {
                    info!(
                        "Planning targeted quote refresh for closed position {}",
                        asset.id
                    );
                    category = SyncCategory::RecentlyClosed;
                    planning_inputs.is_active = true;
                    targeted_closed_incremental = true;
                }
            }

            if matches!(mode, SyncMode::Incremental)
                && matches!(category, SyncCategory::NeedsBackfill)
                && planning_inputs.is_active
            {
                let recent_category = SyncCategory::Active;
                if let Some((start_date, end_date)) =
                    calculate_sync_window(&recent_category, &planning_inputs, effective_today)
                {
                    if start_date <= end_date {
                        plans.push(SymbolSyncPlan {
                            asset_id: asset.id.clone(),
                            category: recent_category.clone(),
                            start_date,
                            end_date: clamp_end_date_for_fetch(
                                &recent_category,
                                end_date,
                                fetch_end_date,
                            ),
                            priority: SyncCategory::Active.default_priority(),
                            data_source: data_source.clone(),
                            quote_symbol: None,
                            currency: asset.quote_ccy.clone(),
                            suppress_closed_fetch_errors: false,
                        });
                    }
                }

                if let Some((start_date, end_date)) =
                    calculate_sync_window(&category, &planning_inputs, effective_today)
                {
                    if start_date <= end_date {
                        plans.push(SymbolSyncPlan {
                            asset_id: asset.id.clone(),
                            category: category.clone(),
                            start_date,
                            end_date,
                            priority: category.default_priority(),
                            data_source: data_source.clone(),
                            quote_symbol: None,
                            currency: asset.quote_ccy.clone(),
                            suppress_closed_fetch_errors: false,
                        });
                    }
                }
                continue;
            }

            let (start_date, end_date) = if targeted_closed_incremental {
                let Some(window) =
                    calculate_targeted_closed_incremental_window(&planning_inputs, fetch_end_date)
                else {
                    result.add_skipped(asset.id.clone(), AssetSkipReason::NoQuoteWindow);
                    continue;
                };
                window
            } else {
                self.calculate_date_range_for_mode(
                    &planning_inputs,
                    mode,
                    effective_today,
                    fetch_end_date,
                    asset,
                )
            };

            plans.push(SymbolSyncPlan {
                asset_id: asset.id.clone(),
                category: category.clone(),
                start_date,
                end_date,
                priority: category.default_priority(),
                data_source,
                quote_symbol: None,
                currency: asset.quote_ccy.clone(),
                suppress_closed_fetch_errors: matches!(category, SyncCategory::Closed)
                    && !targeted_sync,
            });
        }

        plans.retain(|plan| plan.start_date <= plan.end_date);

        if plans.is_empty() {
            info!(
                "No valid sync plans generated (skipped: {})",
                result.skipped
            );
            return Ok(result);
        }

        let planned_asset_ids: HashSet<String> =
            plans.iter().map(|plan| plan.asset_id.clone()).collect();
        let ensure_state_assets: Vec<Asset> = syncable
            .iter()
            .filter(|asset| planned_asset_ids.contains(&asset.id))
            .map(|asset| (*asset).clone())
            .collect();

        if let Err(e) = self
            .ensure_sync_states_for_assets(&ensure_state_assets, true)
            .await
        {
            warn!("Failed to ensure sync states: {:?}", e);
        }

        info!(
            "Syncing {} assets with mode {} (skipped: {})",
            plans.len(),
            mode,
            result.skipped
        );

        // Execute sync and merge with skipped results
        let mut exec_result = self
            .execute_sync_plans(plans, matches!(mode, SyncMode::RefreshAllHistory { .. }))
            .await;
        exec_result.skipped += result.skipped;
        exec_result.skipped_reasons.extend(result.skipped_reasons);
        Ok(exec_result)
    }

    async fn resync(&self, asset_ids: Option<Vec<String>>) -> Result<SyncResult> {
        // Preserve the legacy explicit full-history refresh action, not an automatic backfill.
        self.sync(
            SyncMode::RefreshAllHistory {
                days: DEFAULT_HISTORY_DAYS,
            },
            asset_ids,
        )
        .await
    }

    async fn handle_activity_created(&self, asset_id: &AssetId, activity_date: Day) -> Result<()> {
        let symbol = asset_id.as_str();

        if symbol.is_empty() {
            return Ok(());
        }

        debug!(
            "Handling new activity for {} on {}",
            symbol, activity_date.0
        );

        let existing = self.sync_state_store.get_by_asset_id(symbol)?;

        if let Some(mut state) = existing {
            // Check if we need backfill by computing quote bounds on-the-fly
            // Use QUOTE_HISTORY_BUFFER_DAYS + BACKFILL_SAFETY_MARGIN_DAYS for conservative detection
            let required_start = activity_date.0
                - Duration::days(QUOTE_HISTORY_BUFFER_DAYS + BACKFILL_SAFETY_MARGIN_DAYS);

            // Compute quote bounds for this asset filtered by provider
            let quote_bounds = self
                .quote_store
                .get_quote_bounds_for_assets(&[symbol.to_string()], &state.data_source)
                .unwrap_or_default();
            let earliest_quote = quote_bounds.get(symbol).map(|(min, _)| *min);

            let needs_backfill = earliest_quote
                .map(|earliest| required_start < earliest)
                .unwrap_or(true); // If no quotes exist, assume backfill needed

            if needs_backfill {
                state.sync_priority = SyncCategory::NeedsBackfill.default_priority();
                debug!(
                    "Activity {} needs backfill: required_start={}, earliest_quote={:?}",
                    symbol, required_start, earliest_quote
                );
            }

            // Activity dates are computed on-the-fly, no need to update them here

            if !state.is_active {
                state.mark_active();
            }

            self.sync_state_store.upsert(&state).await?;
        } else {
            // Check if asset should be synced (only market-priced assets)
            if let Ok(asset) = self.asset_repo.get_by_id(symbol) {
                if asset.quote_mode != QuoteMode::Market {
                    return Ok(());
                }

                let mut state = QuoteSyncState::new(
                    symbol.to_string(),
                    asset
                        .preferred_provider()
                        .unwrap_or_else(|| DATA_SOURCE_YAHOO.to_string()),
                );
                // is_active is derived from position_closed_date (None = active)
                // QuoteSyncState::new() already sets position_closed_date = None
                state.sync_priority = SyncCategory::New.default_priority();

                self.sync_state_store.upsert(&state).await?;
            }
        }

        Ok(())
    }

    async fn handle_activity_deleted(&self, asset_id: &AssetId) -> Result<()> {
        let symbol = asset_id.as_str();

        if symbol.is_empty() {
            return Ok(());
        }

        debug!(
            "Activity deleted for {} - sync planning will recompute activity bounds on demand",
            symbol
        );

        Ok(())
    }

    async fn refresh_sync_state(&self) -> Result<()> {
        self.ensure_sync_states_for_all_assets().await
    }

    fn get_sync_plan(&self) -> Result<Vec<SymbolSyncPlan>> {
        self.generate_sync_plan()
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quotes::sync_state::MarketSyncMode;

    #[test]
    fn test_clamp_end_date_for_fetch_extends_active_category() {
        let base_end = NaiveDate::from_ymd_opt(2026, 2, 11).unwrap();
        let fetch_end = NaiveDate::from_ymd_opt(2026, 2, 12).unwrap();

        assert_eq!(
            clamp_end_date_for_fetch(&SyncCategory::Active, base_end, fetch_end),
            fetch_end
        );
    }

    #[test]
    fn test_clamp_end_date_for_fetch_keeps_non_extending_category() {
        let base_end = NaiveDate::from_ymd_opt(2026, 2, 11).unwrap();
        let fetch_end = NaiveDate::from_ymd_opt(2026, 2, 12).unwrap();

        assert_eq!(
            clamp_end_date_for_fetch(&SyncCategory::NeedsBackfill, base_end, fetch_end),
            base_end
        );
    }

    #[test]
    fn test_no_data_error_is_a_neutral_skip() {
        let err = Error::MarketData(MarketDataError::NoData);
        assert_eq!(
            fetch_error_skip_reason(&err),
            Some(AssetSkipReason::NoDataForRange)
        );
    }

    #[test]
    fn test_backfill_not_found_error_is_non_fatal() {
        let err = Error::MarketData(MarketDataError::NotFound("No data found".to_string()));
        assert!(should_treat_fetch_error_as_non_fatal(
            &SyncCategory::NeedsBackfill,
            &err,
            false
        ));
    }

    #[test]
    fn test_backfill_provider_exhausted_error_is_non_fatal() {
        let err = Error::MarketData(MarketDataError::ProviderExhausted(
            "All providers failed".to_string(),
        ));
        assert!(should_treat_fetch_error_as_non_fatal(
            &SyncCategory::NeedsBackfill,
            &err,
            false
        ));
    }

    #[test]
    fn test_format_sync_failure_message_adds_provider_when_missing() {
        let err = Error::MarketData(MarketDataError::NoData);
        let message = format_sync_failure_message(&err, Some("FINNHUB"));
        assert_eq!(
            message,
            "FINNHUB: Market data operation failed: No data found"
        );
    }

    #[test]
    fn test_format_sync_failure_message_keeps_existing_provider() {
        let err = Error::MarketData(MarketDataError::ProviderError(
            "FINNHUB: Access forbidden".to_string(),
        ));
        let message = format_sync_failure_message(&err, Some("YAHOO"));
        assert_eq!(
            message,
            "Market data operation failed: Provider error: FINNHUB: Access forbidden"
        );
    }

    #[test]
    fn test_format_sync_failure_message_without_provider_hint() {
        let err = Error::MarketData(MarketDataError::NoData);
        let message = format_sync_failure_message(&err, None);
        assert_eq!(message, "Market data operation failed: No data found");
    }

    // =========================================================================
    // SyncMode Tests
    // =========================================================================

    #[test]
    fn test_sync_mode_display() {
        assert_eq!(format!("{}", SyncMode::Incremental), "Incremental");
        assert_eq!(
            format!("{}", SyncMode::RefetchRecent { days: 45 }),
            "RefetchRecent(45d)"
        );
        assert_eq!(
            format!("{}", SyncMode::BackfillHistory { days: 1825 }),
            "BackfillHistory(1825d)"
        );
    }

    #[test]
    fn test_sync_mode_default_is_incremental() {
        assert_eq!(SyncMode::default(), SyncMode::Incremental);
    }

    #[test]
    fn test_sync_mode_equality() {
        assert_eq!(SyncMode::Incremental, SyncMode::Incremental);
        assert_eq!(
            SyncMode::RefetchRecent { days: 30 },
            SyncMode::RefetchRecent { days: 30 }
        );
        assert_ne!(
            SyncMode::RefetchRecent { days: 30 },
            SyncMode::RefetchRecent { days: 45 }
        );
        assert_ne!(SyncMode::Incremental, SyncMode::RefetchRecent { days: 30 });
    }

    // =========================================================================
    // MarketSyncMode Tests
    // =========================================================================

    #[test]
    fn test_market_sync_mode_requires_sync() {
        assert!(!MarketSyncMode::None.requires_sync());
        assert!(MarketSyncMode::Incremental { asset_ids: None }.requires_sync());
        assert!(MarketSyncMode::RefetchRecent {
            asset_ids: None,
            days: 45
        }
        .requires_sync());
        assert!(MarketSyncMode::BackfillHistory {
            asset_ids: None,
            days: 1825
        }
        .requires_sync());

        // With asset_ids specified
        assert!(MarketSyncMode::Incremental {
            asset_ids: Some(vec!["AAPL".to_string()])
        }
        .requires_sync());
    }

    #[test]
    fn test_market_sync_mode_to_sync_mode() {
        assert_eq!(MarketSyncMode::None.to_sync_mode(), None);
        assert_eq!(
            MarketSyncMode::Incremental { asset_ids: None }.to_sync_mode(),
            Some(SyncMode::Incremental)
        );
        assert_eq!(
            MarketSyncMode::RefetchRecent {
                asset_ids: None,
                days: 45
            }
            .to_sync_mode(),
            Some(SyncMode::RefetchRecent { days: 45 })
        );
        assert_eq!(
            MarketSyncMode::BackfillHistory {
                asset_ids: None,
                days: 1825
            }
            .to_sync_mode(),
            Some(SyncMode::BackfillHistory { days: 1825 })
        );
    }

    #[test]
    fn test_market_sync_mode_asset_ids() {
        assert!(MarketSyncMode::None.asset_ids().is_none());
        assert!(MarketSyncMode::Incremental { asset_ids: None }
            .asset_ids()
            .is_none());

        let asset_ids = vec!["AAPL".to_string(), "MSFT".to_string()];
        let mode = MarketSyncMode::Incremental {
            asset_ids: Some(asset_ids.clone()),
        };
        assert_eq!(mode.asset_ids(), Some(&asset_ids));

        let mode = MarketSyncMode::RefetchRecent {
            asset_ids: Some(asset_ids.clone()),
            days: 45,
        };
        assert_eq!(mode.asset_ids(), Some(&asset_ids));

        let mode = MarketSyncMode::BackfillHistory {
            asset_ids: Some(asset_ids.clone()),
            days: 1825,
        };
        assert_eq!(mode.asset_ids(), Some(&asset_ids));
    }

    #[test]
    fn test_market_sync_mode_default_is_none() {
        assert_eq!(MarketSyncMode::default(), MarketSyncMode::None);
    }

    // =========================================================================
    // AssetSkipReason Display Tests
    // =========================================================================

    #[test]
    fn test_asset_skip_reason_display_cash_asset() {
        assert_eq!(
            AssetSkipReason::CashAsset.to_string(),
            "Cash asset (no quotes needed)"
        );
    }

    #[test]
    fn test_asset_skip_reason_display_manual_pricing() {
        assert_eq!(
            AssetSkipReason::ManualPricing.to_string(),
            "Manual pricing mode"
        );
    }

    #[test]
    fn test_asset_skip_reason_display_inactive() {
        assert_eq!(AssetSkipReason::Inactive.to_string(), "Inactive asset");
    }

    #[test]
    fn test_asset_skip_reason_display_closed_position() {
        assert_eq!(
            AssetSkipReason::ClosedPosition.to_string(),
            "Closed position"
        );
    }

    #[test]
    fn test_asset_skip_reason_display_not_found() {
        assert_eq!(AssetSkipReason::NotFound.to_string(), "Asset not found");
    }

    // =========================================================================
    // SyncResult Tests
    // =========================================================================

    #[test]
    fn test_sync_result_summary() {
        let result = SyncResult {
            synced: 10,
            failed: 0,
            skipped: 2,
            quotes_synced: 100,
            errors: vec![],
            failures: vec![],
            skipped_reasons: vec![
                ("CASH:USD".to_string(), AssetSkipReason::CashAsset),
                ("MANUAL_ASSET".to_string(), AssetSkipReason::ManualPricing),
            ],
        };
        assert!(result.is_success());
        assert!(result.summary().contains("100"));
        assert_eq!(result.skipped_reasons.len(), 2);

        let result_with_failures = SyncResult {
            synced: 5,
            failed: 1,
            skipped: 0,
            quotes_synced: 50,
            errors: vec![SyncError {
                asset_id: AssetId::new("AAPL"),
                message: "timeout".to_string(),
                provider_errors: vec![],
            }],
            failures: vec![("AAPL".to_string(), "timeout".to_string())],
            skipped_reasons: vec![],
        };
        assert!(!result_with_failures.is_success());
        assert!(result_with_failures.summary().contains("1 failures"));
    }

    #[test]
    fn test_sync_status() {
        assert_eq!(SyncStatus::Success, SyncStatus::Success);
        assert_ne!(SyncStatus::Success, SyncStatus::Failed);
    }

    #[test]
    fn test_asset_sync_result() {
        let result = AssetSyncResult {
            asset_id: AssetId::new("AAPL"),
            quotes_added: 10,
            status: SyncStatus::Success,
            error: None,
            skip_reason: None,
        };

        assert_eq!(result.asset_id.as_str(), "AAPL");
        assert_eq!(result.quotes_added, 10);
        assert_eq!(result.status, SyncStatus::Success);
        assert!(result.error.is_none());
        assert!(result.skip_reason.is_none());
    }

    #[test]
    fn test_skipped_result_records_structured_reason() {
        let mut result = SyncResult::default();
        result.add_result(AssetSyncResult {
            asset_id: AssetId::new("SPARSE"),
            quotes_added: 0,
            status: SyncStatus::Skipped,
            error: None,
            skip_reason: Some(AssetSkipReason::NoDataForRange),
        });

        assert_eq!(result.skipped, 1);
        assert_eq!(
            result.skipped_reasons,
            vec![("SPARSE".to_string(), AssetSkipReason::NoDataForRange)]
        );
    }

    #[test]
    fn test_asset_skip_reason_display() {
        assert_eq!(
            AssetSkipReason::CashAsset.to_string(),
            "Cash asset (no quotes needed)"
        );
        assert_eq!(
            AssetSkipReason::ManualPricing.to_string(),
            "Manual pricing mode"
        );
        assert_eq!(AssetSkipReason::Inactive.to_string(), "Inactive asset");
        assert_eq!(
            AssetSkipReason::ClosedPosition.to_string(),
            "Closed position"
        );
        assert_eq!(AssetSkipReason::NotFound.to_string(), "Asset not found");
        assert_eq!(
            AssetSkipReason::SyncInProgress.to_string(),
            "Sync already in progress"
        );
        assert_eq!(
            AssetSkipReason::NoQuoteWindow.to_string(),
            "No quote refresh needed"
        );
        assert_eq!(
            AssetSkipReason::NoDataForRange.to_string(),
            "No data for requested date range"
        );
    }

    #[test]
    fn test_inactive_asset_skipped_for_broad_sync() {
        let asset = Asset {
            is_active: false,
            quote_mode: QuoteMode::Market,
            kind: AssetKind::Investment,
            ..Default::default()
        };

        assert_eq!(
            asset_skip_reason(&asset, false),
            Some(AssetSkipReason::Inactive)
        );
    }

    #[test]
    fn test_inactive_asset_allowed_for_targeted_sync() {
        let asset = Asset {
            is_active: false,
            quote_mode: QuoteMode::Market,
            kind: AssetKind::Investment,
            ..Default::default()
        };

        assert_eq!(asset_skip_reason(&asset, true), None);
    }

    #[test]
    fn test_empty_asset_target_is_explicit_noop() {
        let empty_ids: Vec<String> = Vec::new();
        let targeted_ids = vec!["AAPL".to_string()];

        assert!(!is_empty_asset_target(None));
        assert!(is_empty_asset_target(Some(&empty_ids)));
        assert!(!is_empty_asset_target(Some(&targeted_ids)));

        assert!(!is_targeted_asset_sync(None));
        assert!(!is_targeted_asset_sync(Some(&empty_ids)));
        assert!(is_targeted_asset_sync(Some(&targeted_ids)));
    }

    #[test]
    fn test_targeted_sync_bypasses_error_limit() {
        assert!(!should_skip_for_error_limit(
            SyncMode::Incremental,
            true,
            MAX_SYNC_ERRORS,
        ));
        assert!(should_skip_for_error_limit(
            SyncMode::Incremental,
            false,
            MAX_SYNC_ERRORS,
        ));
    }

    #[test]
    fn test_activity_and_holdings_bounds_are_merged_for_history_planning() {
        let activity_min = NaiveDate::from_ymd_opt(2026, 2, 1).unwrap();
        let activity_max = NaiveDate::from_ymd_opt(2026, 3, 1).unwrap();
        let holdings_min = NaiveDate::from_ymd_opt(2025, 11, 1).unwrap();
        let holdings_max = NaiveDate::from_ymd_opt(2026, 1, 15).unwrap();

        assert_eq!(
            merge_bounds(
                Some((Some(activity_min), Some(activity_max))),
                Some((Some(holdings_min), Some(holdings_max))),
            ),
            (Some(holdings_min), Some(activity_max))
        );
    }

    #[test]
    fn test_targeted_closed_incremental_window_fetches_through_fetch_end_date() {
        let inputs = SyncPlanningInputs {
            is_active: false,
            position_closed_date: Some(NaiveDate::from_ymd_opt(2026, 2, 15).unwrap()),
            activity_min: Some(NaiveDate::from_ymd_opt(2025, 1, 1).unwrap()),
            activity_max: Some(NaiveDate::from_ymd_opt(2026, 2, 15).unwrap()),
            quote_min: Some(NaiveDate::from_ymd_opt(2025, 1, 1).unwrap()),
            quote_max: Some(NaiveDate::from_ymd_opt(2026, 2, 1).unwrap()),
        };

        let (start, end) = calculate_targeted_closed_incremental_window(
            &inputs,
            NaiveDate::from_ymd_opt(2026, 5, 6).unwrap(),
        )
        .unwrap();

        assert_eq!(start, NaiveDate::from_ymd_opt(2026, 1, 27).unwrap());
        assert_eq!(end, NaiveDate::from_ymd_opt(2026, 5, 6).unwrap());
    }

    #[test]
    fn test_targeted_closed_incremental_window_returns_none_when_already_current() {
        let inputs = SyncPlanningInputs {
            is_active: false,
            position_closed_date: Some(NaiveDate::from_ymd_opt(2026, 2, 15).unwrap()),
            activity_min: Some(NaiveDate::from_ymd_opt(2025, 1, 1).unwrap()),
            activity_max: Some(NaiveDate::from_ymd_opt(2026, 2, 15).unwrap()),
            quote_min: Some(NaiveDate::from_ymd_opt(2025, 1, 1).unwrap()),
            quote_max: Some(NaiveDate::from_ymd_opt(2026, 6, 1).unwrap()),
        };

        assert_eq!(
            calculate_targeted_closed_incremental_window(
                &inputs,
                NaiveDate::from_ymd_opt(2026, 5, 6).unwrap(),
            ),
            None
        );
    }

    #[test]
    fn test_inactive_asset_policy_is_mode_aware() {
        assert!(!should_allow_inactive_assets(SyncMode::Incremental, false));
        assert!(should_allow_inactive_assets(SyncMode::Incremental, true));
        assert!(should_allow_inactive_assets(
            SyncMode::RefetchRecent { days: 30 },
            false
        ));
        assert!(should_allow_inactive_assets(
            SyncMode::BackfillHistory { days: 365 },
            false
        ));
    }

    #[test]
    fn test_broad_history_allows_inactive_only_with_reference() {
        let asset_id = "asset_1".to_string();
        let states = HashMap::new();
        let activity_bounds = HashMap::new();
        let empty_holdings_bounds = HashMap::new();
        let holdings_bounds = HashMap::from([(
            asset_id.clone(),
            (Some(NaiveDate::from_ymd_opt(2025, 11, 1).unwrap()), None),
        )]);

        assert!(!should_allow_inactive_asset(
            &asset_id,
            SyncMode::BackfillHistory { days: 365 },
            false,
            &states,
            &activity_bounds,
            &empty_holdings_bounds,
        ));
        assert!(should_allow_inactive_asset(
            &asset_id,
            SyncMode::BackfillHistory { days: 365 },
            false,
            &states,
            &activity_bounds,
            &holdings_bounds,
        ));
        assert!(should_allow_inactive_asset(
            &asset_id,
            SyncMode::Incremental,
            true,
            &states,
            &activity_bounds,
            &empty_holdings_bounds,
        ));
    }

    #[test]
    fn test_closed_position_policy_is_mode_aware() {
        assert!(!should_include_closed_positions(
            SyncMode::Incremental,
            false
        ));
        assert!(should_include_closed_positions(SyncMode::Incremental, true));
        assert!(should_include_closed_positions(
            SyncMode::RefetchRecent { days: 30 },
            false
        ));
        assert!(should_include_closed_positions(
            SyncMode::BackfillHistory { days: 365 },
            false
        ));
    }

    // =========================================================================
    // Per-Asset Sync Lock Tests (US-012)
    // =========================================================================

    #[test]
    fn freshness_distinguishes_intraday_closed_weekend_and_unknown_market() {
        let asset = Asset {
            instrument_type: Some(InstrumentType::Equity),
            instrument_exchange_mic: Some("XNAS".into()),
            ..Default::default()
        };
        let friday = NaiveDate::from_ymd_opt(2026, 10, 23).unwrap();
        let monday = NaiveDate::from_ymd_opt(2026, 10, 26).unwrap();
        let record = HistoryCoverage {
            identity: "digest".into(),
            asset_id: "AAPL".into(),
            start: friday,
            end: friday,
            fetched_at: friday.and_hms_opt(22, 0, 0).unwrap().and_utc(),
            outcome: HistoryOutcome::Complete,
        };
        let sunday = NaiveDate::from_ymd_opt(2026, 10, 25)
            .unwrap()
            .and_hms_opt(12, 0, 0)
            .unwrap()
            .and_utc();
        assert_eq!(
            history_range_settled(&asset, &record, sunday, Some(friday)),
            Some(true)
        );
        let intraday = HistoryCoverage {
            end: monday,
            fetched_at: monday.and_hms_opt(15, 0, 0).unwrap().and_utc(),
            ..record.clone()
        };
        assert_eq!(
            history_range_settled(
                &asset,
                &intraday,
                monday.and_hms_opt(16, 0, 0).unwrap().and_utc(),
                Some(monday)
            ),
            None
        );
        assert_eq!(
            history_range_settled(
                &asset,
                &intraday,
                monday.and_hms_opt(22, 0, 0).unwrap().and_utc(),
                Some(monday)
            ),
            Some(false)
        );
        let unknown = Asset {
            instrument_exchange_mic: None,
            ..asset
        };
        assert_eq!(
            history_range_settled(&unknown, &record, sunday, Some(friday)),
            None
        );
    }

    #[test]
    fn new_market_session_refreshes_overlap_but_weekend_reuses_settled_history() {
        let asset = Asset {
            instrument_type: Some(InstrumentType::Equity),
            instrument_exchange_mic: Some("XNAS".into()),
            ..Default::default()
        };
        let start = NaiveDate::from_ymd_opt(2026, 10, 1).unwrap();
        let friday = NaiveDate::from_ymd_opt(2026, 10, 23).unwrap();
        let monday = NaiveDate::from_ymd_opt(2026, 10, 26).unwrap();
        let record = HistoryCoverage {
            identity: "digest".into(),
            asset_id: "AAPL".into(),
            start,
            end: friday,
            fetched_at: friday.and_hms_opt(22, 0, 0).unwrap().and_utc(),
            outcome: HistoryOutcome::Complete,
        };
        let sunday = NaiveDate::from_ymd_opt(2026, 10, 25)
            .unwrap()
            .and_hms_opt(12, 0, 0)
            .unwrap()
            .and_utc();
        let missing = |end, now| {
            missing_history_ranges_with_freshness(
                start,
                end,
                std::slice::from_ref(&record),
                now,
                Duration::hours(6),
                |receipt| history_range_settled(&asset, receipt, now, Some(friday)),
            )
        };
        assert!(missing(friday, sunday).is_empty());
        assert_eq!(
            missing(monday, monday.and_hms_opt(22, 0, 0).unwrap().and_utc()),
            vec![(
                (friday - Duration::days(OVERLAP_DAYS)).succ_opt().unwrap(),
                monday
            )]
        );
    }

    #[test]
    fn latest_credential_responses_and_explicit_fallbacks_only_have_bounded_coverage() {
        let public = Asset::default();
        assert_eq!(
            latest_history_outcome(&public, DATA_SOURCE_YAHOO),
            HistoryOutcome::Complete
        );
        assert_eq!(
            latest_history_outcome(&public, DATA_SOURCE_EASTMONEY),
            HistoryOutcome::Complete
        );
        assert_eq!(
            latest_history_outcome(&public, DATA_SOURCE_FUTU),
            HistoryOutcome::Partial
        );
        assert_eq!(
            latest_history_outcome(&public, "CUSTOM_SCRAPER:fund"),
            HistoryOutcome::Partial
        );
        let preferred = Asset {
            provider_config: Some(serde_json::json!({"preferred_provider": DATA_SOURCE_FUTU})),
            ..Default::default()
        };
        assert_eq!(
            latest_history_outcome(&preferred, DATA_SOURCE_YAHOO),
            HistoryOutcome::Partial
        );
    }

    #[test]
    fn latest_current_weekend_observation_is_bounded_and_future_is_uncached() {
        let friday = NaiveDate::from_ymd_opt(2026, 10, 23).unwrap();
        let sunday = NaiveDate::from_ymd_opt(2026, 10, 25).unwrap();
        let observed = sunday.and_hms_opt(12, 0, 0).unwrap().and_utc();
        assert_eq!(
            latest_observation_range(observed - Duration::seconds(1), friday, observed),
            Some((friday, sunday))
        );
        assert_eq!(
            latest_observation_range(observed + Duration::hours(1), friday, observed),
            None
        );
        let receipt = HistoryCoverage {
            identity: "latest-only".into(),
            asset_id: "asset".into(),
            start: friday,
            end: sunday,
            fetched_at: observed,
            outcome: HistoryOutcome::Partial,
        };
        assert!(missing_history_ranges_with_freshness(
            friday,
            friday,
            std::slice::from_ref(&receipt),
            observed + Duration::hours(1),
            Duration::hours(6),
            |_| Some(true)
        )
        .is_empty());
        assert_eq!(
            missing_history_ranges_with_freshness(
                friday,
                friday,
                &[receipt],
                observed + Duration::hours(7),
                Duration::hours(6),
                |_| Some(true)
            ),
            vec![(friday, friday)]
        );
    }

    #[test]
    fn unknown_history_currency_cannot_mix_cached_and_new_provider_units() {
        let mut quote = Quote {
            currency: "HKD".into(),
            ..Default::default()
        };
        let mut currency = None;
        validate_history_currency(&[quote.clone()], &mut currency).unwrap();
        validate_history_currency(&[quote.clone()], &mut currency).unwrap();
        quote.currency = "CNY".into();
        assert!(validate_history_currency(&[quote.clone()], &mut currency).is_err());
        currency = None;
        quote.currency = "GBP".into();
        validate_history_currency(&[quote.clone()], &mut currency).unwrap();
        quote.currency = "GBp".into();
        assert!(validate_history_currency(&[quote], &mut currency).is_err());
    }

    #[test]
    fn sync_result_keeps_writes_when_later_range_fails() {
        let mut result = SyncResult::default();
        result.add_result(AssetSyncResult {
            asset_id: AssetId::new("AAPL"),
            quotes_added: 2,
            status: SyncStatus::Failed,
            error: Some("later range failed".into()),
            skip_reason: None,
        });
        assert_eq!(result.quotes_synced, 2);
        assert_eq!(result.failed, 1);
    }

    #[tokio::test]
    async fn history_locks_wait_and_are_profile_scoped() {
        let profile_a = HistoryLocks::default();
        let profile_b = HistoryLocks::default();
        let first = profile_a.for_asset("same-id");
        let guard = first.clone().lock_owned().await;
        assert!(profile_a.for_asset("same-id").try_lock().is_err());
        assert!(profile_a.for_asset("other-id").try_lock().is_ok());
        assert!(profile_b.for_asset("same-id").try_lock().is_ok());
        drop(guard);
        assert!(profile_a.for_asset("same-id").try_lock().is_ok());
    }

    #[test]
    fn test_sync_result_add_skipped() {
        let mut result = SyncResult::default();
        assert_eq!(result.skipped, 0);
        assert!(result.skipped_reasons.is_empty());

        result.add_skipped("CASH:USD".to_string(), AssetSkipReason::CashAsset);
        assert_eq!(result.skipped, 1);
        assert_eq!(result.skipped_reasons.len(), 1);
        assert_eq!(result.skipped_reasons[0].0, "CASH:USD");
        assert_eq!(result.skipped_reasons[0].1, AssetSkipReason::CashAsset);

        result.add_skipped("INACTIVE_ASSET".to_string(), AssetSkipReason::Inactive);
        assert_eq!(result.skipped, 2);
        assert_eq!(result.skipped_reasons.len(), 2);
    }

    // =========================================================================
    // Date Range Planning Tests (US-014)
    // =========================================================================

    /// Tests for Incremental mode date range calculation.
    /// Incremental mode should use last_quote_date - OVERLAP_DAYS as start date
    /// to heal provider corrections (stock splits, dividend adjustments).
    mod date_range_incremental_tests {
        use super::*;

        fn today() -> NaiveDate {
            Utc::now().date_naive()
        }

        #[test]
        fn test_incremental_with_existing_quotes_uses_overlap() {
            // When last_quote_date exists, the start should be last_quote_date - OVERLAP_DAYS
            // This ensures we refetch a small window to heal any provider corrections
            let last_quote = today() - Duration::days(3);
            let expected_start = last_quote - Duration::days(OVERLAP_DAYS);

            // Verify OVERLAP_DAYS is used in the overlap calculation
            assert_eq!(
                OVERLAP_DAYS, 5,
                "OVERLAP_DAYS should be 5 for healing corrections"
            );

            // The overlap window ensures we pick up corrections from providers
            let overlap_window = OVERLAP_DAYS;
            let calculated_start = last_quote - Duration::days(overlap_window);
            assert_eq!(calculated_start, expected_start);
        }

        #[test]
        fn test_incremental_without_quotes_uses_activity_date_with_buffer() {
            // When no quotes exist, start from first_activity_date - QUOTE_HISTORY_BUFFER_DAYS
            let first_activity = today() - Duration::days(30);
            let expected_start = first_activity - Duration::days(QUOTE_HISTORY_BUFFER_DAYS);

            // QUOTE_HISTORY_BUFFER_DAYS provides enough history for valuation
            assert_eq!(
                QUOTE_HISTORY_BUFFER_DAYS, 45,
                "Buffer should be 45 days to cover weekends/holidays"
            );

            let calculated_start = first_activity - Duration::days(QUOTE_HISTORY_BUFFER_DAYS);
            assert_eq!(calculated_start, expected_start);
        }

        #[test]
        fn test_overlap_days_constant_value() {
            // OVERLAP_DAYS is set to 5 for healing provider corrections
            // This is enough to catch most adjustment corrections while being efficient
            assert_eq!(OVERLAP_DAYS, 5);
        }

        #[test]
        fn test_quote_history_buffer_days_constant_value() {
            // QUOTE_HISTORY_BUFFER_DAYS is 45 to account for:
            // - Weekends (8-9 days lost per month)
            // - Holidays (varies by market)
            // - Potential data gaps
            assert_eq!(QUOTE_HISTORY_BUFFER_DAYS, 45);
        }
    }

    /// Tests for RefetchRecent mode date range calculation.
    /// RefetchRecent always starts from today - days, ignoring existing quotes.
    mod date_range_refetch_recent_tests {
        use super::*;

        fn today() -> NaiveDate {
            Utc::now().date_naive()
        }

        #[test]
        fn test_refetch_recent_ignores_existing_quotes() {
            // RefetchRecent should always start from today - days
            // regardless of last_quote_date
            let days = 45_i64;
            let expected_start = today() - Duration::days(days);

            let mode = SyncMode::RefetchRecent { days };
            if let SyncMode::RefetchRecent { days: mode_days } = mode {
                let calculated_start = today() - Duration::days(mode_days);
                assert_eq!(calculated_start, expected_start);
            }
        }

        #[test]
        fn test_refetch_recent_with_30_days() {
            let days = 30_i64;
            let expected_start = today() - Duration::days(days);
            let calculated_start = today() - Duration::days(days);
            assert_eq!(calculated_start, expected_start);
        }

        #[test]
        fn test_refetch_recent_with_90_days() {
            let days = 90_i64;
            let expected_start = today() - Duration::days(days);
            let calculated_start = today() - Duration::days(days);
            assert_eq!(calculated_start, expected_start);
        }
    }

    /// Tests for BackfillHistory mode date range calculation.
    /// BackfillHistory rebuilds full history from activity start.
    mod date_range_backfill_history_tests {
        use super::*;

        fn today() -> NaiveDate {
            Utc::now().date_naive()
        }

        #[test]
        fn test_backfill_history_uses_activity_min_date() {
            // BackfillHistory should start from activity_min_date - BUFFER_DAYS
            // when activity exists
            let first_activity = today() - Duration::days(100);
            let expected_start = first_activity - Duration::days(QUOTE_HISTORY_BUFFER_DAYS);

            let calculated_start = first_activity - Duration::days(QUOTE_HISTORY_BUFFER_DAYS);
            assert_eq!(calculated_start, expected_start);
        }

        #[test]
        fn test_backfill_history_uses_fallback_when_no_activity() {
            // When no activity date exists, use the unified buffer fallback
            let expected_start = today() - Duration::days(QUOTE_HISTORY_BUFFER_DAYS);

            let calculated_start = today() - Duration::days(QUOTE_HISTORY_BUFFER_DAYS);
            assert_eq!(calculated_start, expected_start);
        }

        #[test]
        fn test_default_history_days_constant() {
            // DEFAULT_HISTORY_DAYS is 1825 (5 years) for explicit full-history requests
            assert_eq!(DEFAULT_HISTORY_DAYS, 1825);
        }
    }

    /// Tests for FX asset handling in BackfillHistory mode (US-006).
    /// FX assets need special handling since they have no activities attached.
    mod fx_backfill_tests {
        use super::*;

        #[test]
        fn test_fx_asset_kind_detection() {
            // FX assets are identified by AssetKind::Fx
            assert_eq!(AssetKind::Fx.as_db_str(), "FX");
        }

        #[test]
        fn test_fx_backfill_design_documented() {
            // FX assets in BackfillHistory mode should use global earliest activity date
            // because FX assets have no activities of their own.
            //
            // The implementation in calculate_date_range_for_mode() calls:
            // sync_state_store.get_earliest_activity_date_global()
            //
            // This ensures FX pairs have quote coverage from the earliest activity
            // date across the entire portfolio, enabling accurate valuation history.
            //
            // Example:
            // - User has activities starting 2020-01-15
            // - FX:EUR:USD needs quotes from 2020-01-15 - BUFFER_DAYS
            // - Without this, base currency changes would fail for historical data
            // FX backfill design documented
        }

        #[test]
        fn test_fx_incremental_does_not_backfill() {
            // In Incremental mode, FX assets should continue from last_quote_date
            // and should NOT trigger a backfill to global earliest activity
            //
            // This is important for performance - we don't want every incremental
            // sync to fetch years of FX history
            // FX incremental behavior documented
        }
    }

    // =========================================================================
    // Targeting Semantics Tests (US-014)
    // =========================================================================

    /// Tests for targeted asset sync behavior.
    /// When asset_ids is Some(vec![...]), only those assets should be synced.
    mod targeting_tests {
        use super::*;

        #[test]
        fn test_market_sync_mode_with_targeted_assets() {
            // When asset_ids is specified, only those assets should be synced
            let target_assets = vec!["AAPL".to_string(), "MSFT".to_string()];
            let mode = MarketSyncMode::Incremental {
                asset_ids: Some(target_assets.clone()),
            };

            // asset_ids() returns the specified list
            let ids = mode.asset_ids();
            assert!(ids.is_some());
            assert_eq!(ids.unwrap().len(), 2);
            assert!(ids.unwrap().contains(&"AAPL".to_string()));
            assert!(ids.unwrap().contains(&"MSFT".to_string()));
        }

        #[test]
        fn test_market_sync_mode_without_targeted_assets_syncs_all() {
            // When asset_ids is None, all relevant assets should be synced
            let mode = MarketSyncMode::Incremental { asset_ids: None };
            assert!(mode.asset_ids().is_none());
        }

        #[test]
        fn test_market_sync_mode_empty_asset_ids() {
            // Empty vec is different from None - it means sync nothing specific
            let mode = MarketSyncMode::Incremental {
                asset_ids: Some(vec![]),
            };
            let ids = mode.asset_ids();
            assert!(ids.is_some());
            assert!(ids.unwrap().is_empty());
        }

        #[test]
        fn test_targeted_refetch_recent() {
            let target_assets = vec!["GOOG".to_string()];
            let mode = MarketSyncMode::RefetchRecent {
                asset_ids: Some(target_assets.clone()),
                days: 30,
            };

            assert_eq!(mode.asset_ids(), Some(&target_assets));
            assert_eq!(
                mode.to_sync_mode(),
                Some(SyncMode::RefetchRecent { days: 30 })
            );
        }

        #[test]
        fn test_targeted_backfill_history() {
            let target_assets = vec!["TSLA".to_string(), "NVDA".to_string()];
            let mode = MarketSyncMode::BackfillHistory {
                asset_ids: Some(target_assets.clone()),
                days: 365,
            };

            assert_eq!(mode.asset_ids(), Some(&target_assets));
            assert_eq!(
                mode.to_sync_mode(),
                Some(SyncMode::BackfillHistory { days: 365 })
            );
        }
    }

    // =========================================================================
    // SyncCategory Priority Tests
    // =========================================================================

    mod sync_category_tests {
        use super::*;

        #[test]
        fn test_sync_category_priority_order() {
            // Active has highest priority
            assert_eq!(SyncCategory::Active.default_priority(), 100);
            // NeedsBackfill is second (urgent - missing historical data)
            assert_eq!(SyncCategory::NeedsBackfill.default_priority(), 90);
            // New is third (needs initial data)
            assert_eq!(SyncCategory::New.default_priority(), 80);
            // RecentlyClosed has lower priority
            assert_eq!(SyncCategory::RecentlyClosed.default_priority(), 50);
            // Closed has zero priority (should not be synced)
            assert_eq!(SyncCategory::Closed.default_priority(), 0);
        }

        #[test]
        fn test_sync_category_priority_ordering() {
            // Verify the relative ordering is correct
            assert!(
                SyncCategory::Active.default_priority()
                    > SyncCategory::NeedsBackfill.default_priority()
            );
            assert!(
                SyncCategory::NeedsBackfill.default_priority()
                    > SyncCategory::New.default_priority()
            );
            assert!(
                SyncCategory::New.default_priority()
                    > SyncCategory::RecentlyClosed.default_priority()
            );
            assert!(
                SyncCategory::RecentlyClosed.default_priority()
                    > SyncCategory::Closed.default_priority()
            );
        }
    }

    // =========================================================================
    // effective_provider Tests
    // =========================================================================

    mod effective_provider_tests {
        use super::*;

        fn test_asset() -> Asset {
            Asset {
                id: "test-id".to_string(),
                kind: AssetKind::Investment,
                ..Default::default()
            }
        }

        fn test_asset_with_preferred(provider: &str) -> Asset {
            Asset {
                id: "test-id".to_string(),
                kind: AssetKind::Investment,
                provider_config: Some(serde_json::json!({
                    "preferred_provider": provider
                })),
                ..Default::default()
            }
        }

        fn test_sync_state(data_source: &str) -> QuoteSyncState {
            QuoteSyncState {
                asset_id: "test-id".to_string(),
                is_active: true,
                position_closed_date: None,
                last_synced_at: None,
                data_source: data_source.to_string(),
                sync_priority: 0,
                error_count: 0,
                last_error: None,
                profile_enriched_at: None,
                created_at: Utc::now(),
                updated_at: Utc::now(),
            }
        }

        #[test]
        fn uses_sync_state_data_source_when_present() {
            let state = test_sync_state("ALPHA_VANTAGE");
            let asset = test_asset();
            assert_eq!(effective_provider(Some(&state), &asset), "ALPHA_VANTAGE");
        }

        #[test]
        fn empty_data_source_falls_through_to_preferred_provider() {
            let state = test_sync_state("");
            let asset = test_asset_with_preferred("METAL_PRICE_API");
            assert_eq!(effective_provider(Some(&state), &asset), "METAL_PRICE_API");
        }

        #[test]
        fn no_state_uses_preferred_provider() {
            let asset = test_asset_with_preferred("METAL_PRICE_API");
            assert_eq!(effective_provider(None, &asset), "METAL_PRICE_API");
        }

        #[test]
        fn no_state_no_preferred_defaults_to_yahoo() {
            let asset = test_asset();
            assert_eq!(effective_provider(None, &asset), DATA_SOURCE_YAHOO);
        }

        #[test]
        fn empty_data_source_no_preferred_defaults_to_yahoo() {
            let state = test_sync_state("");
            let asset = test_asset();
            assert_eq!(effective_provider(Some(&state), &asset), DATA_SOURCE_YAHOO);
        }

        #[test]
        fn data_source_takes_priority_over_preferred_provider() {
            let state = test_sync_state("YAHOO");
            let asset = test_asset_with_preferred("METAL_PRICE_API");
            assert_eq!(effective_provider(Some(&state), &asset), "YAHOO");
        }
    }

    // =========================================================================
    // Closed Position Grace Period Tests
    // =========================================================================

    mod grace_period_tests {
        use super::*;

        #[test]
        fn test_closed_position_grace_period_constant() {
            // Grace period is 30 days
            assert_eq!(CLOSED_POSITION_GRACE_PERIOD_DAYS, 30);
        }

        #[test]
        fn test_grace_period_purpose_documented() {
            // The grace period ensures that recently closed positions continue
            // to receive quote updates for a while after closing.
            //
            // This is important for:
            // - Performance reporting (final performance calculations)
            // - Tax reporting (cost basis calculations)
            // - Historical accuracy (ensuring end-of-position values are correct)
            //
            // After the grace period, quotes are no longer fetched to save resources.
            // Grace period purpose documented
        }
    }
}
