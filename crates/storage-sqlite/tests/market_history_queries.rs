//! Successful query persistence must remain atomic and profile-local.
#![allow(clippy::unwrap_used, clippy::panic, reason = "test code")]
use std::sync::Arc;

use chrono::{Duration, NaiveDate, Utc};
use diesel::prelude::*;
use rust_decimal::Decimal;
use wealthfolio_core::assets::{
    AssetKind, AssetRepositoryTrait, InstrumentType, NewAsset, QuoteMode,
};
use wealthfolio_core::quotes::history::{missing_history_ranges, HistoryCoverage, HistoryOutcome};
use wealthfolio_core::quotes::{AssetId, Quote, QuoteStore};
use wealthfolio_storage_sqlite::{assets::AssetRepository, db, market_data::MarketDataRepository};

struct Fixture {
    store: MarketDataRepository,
    pool: Arc<db::DbPool>,
    writer: db::WriteHandle,
    _dir: tempfile::TempDir,
}

async fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let access = db::DbAccess::plaintext(dir.path().join("app.db").to_str().unwrap());
    access.prepare().unwrap();
    access.run_migrations().unwrap();
    let pool = access.create_pool().unwrap();
    let (writer, _task) = db::write_actor::spawn_writer_with_sync_state(
        (*pool).clone(),
        Arc::new(|| {}),
        Arc::default(),
    )
    .unwrap();
    AssetRepository::new(pool.clone(), writer.clone())
        .create(NewAsset {
            id: Some("asset".into()),
            kind: AssetKind::Investment,
            name: Some("Test".into()),
            display_code: Some("TEST".into()),
            instrument_symbol: Some("TEST".into()),
            instrument_exchange_mic: Some("XNAS".into()),
            instrument_type: Some(InstrumentType::Equity),
            is_active: true,
            quote_mode: QuoteMode::Market,
            quote_ccy: "USD".into(),
            ..Default::default()
        })
        .await
        .unwrap();
    Fixture {
        store: MarketDataRepository::new(pool.clone(), writer.clone()),
        pool,
        writer,
        _dir: dir,
    }
}

fn coverage(identity: &str, asset: &str) -> HistoryCoverage {
    HistoryCoverage {
        identity: identity.into(),
        asset_id: asset.into(),
        start: NaiveDate::from_ymd_opt(2025, 1, 1).unwrap(),
        end: NaiveDate::from_ymd_opt(2025, 1, 31).unwrap(),
        fetched_at: Utc::now(),
        outcome: HistoryOutcome::Complete,
    }
}

fn quote(asset: &str, source: &str, price: i64) -> Quote {
    let date = NaiveDate::from_ymd_opt(2025, 1, 15).unwrap();
    let price = Decimal::new(price, 0);
    Quote {
        id: format!("{asset}_{date}_{source}"),
        asset_id: asset.into(),
        timestamp: date.and_hms_opt(12, 0, 0).unwrap().and_utc(),
        open: price,
        high: price,
        low: price,
        close: price,
        adjclose: price,
        currency: "USD".into(),
        data_source: source.into(),
        created_at: Utc::now(),
        ..Default::default()
    }
}

#[tokio::test]
async fn history_survives_repository_restart_and_is_profile_local() {
    let one = fixture().await;
    let record = coverage("identity-v1", "asset");
    one.store
        .commit_history_query(&record, &[quote("asset", "YAHOO", 100)], false, true, None)
        .await
        .unwrap();
    let reopened = MarketDataRepository::new(one.pool.clone(), one.writer.clone());
    let persisted = reopened.history_coverage("identity-v1").unwrap();
    assert_eq!(persisted, vec![record.clone()]);
    assert!(missing_history_ranges(record.start, record.end, &persisted, Utc::now()).is_empty());
    assert!(reopened
        .history_coverage("identity-new-currency")
        .unwrap()
        .is_empty());
    let two = fixture().await;
    assert!(two
        .store
        .history_coverage("identity-v1")
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn benchmark_history_does_not_create_assets_or_pollute_quote_lists() {
    let f = fixture().await;
    let record = coverage("benchmark-v1", "SEC:BENCH:INDEX");
    f.store
        .commit_history_query(
            &record,
            &[quote("SEC:BENCH:INDEX", "YAHOO", 123)],
            true,
            true,
            None,
        )
        .await
        .unwrap();
    assert_eq!(
        f.store
            .benchmark_history("benchmark-v1", record.start, record.end)
            .unwrap()
            .len(),
        1
    );
    assert!(f
        .store
        .benchmark_history("benchmark-other-currency", record.start, record.end)
        .unwrap()
        .is_empty());
    assert!(f.store.get_all_historical_quotes().unwrap().is_empty());
    assert!(AssetRepository::new(f.pool.clone(), f.writer.clone())
        .get_by_id("SEC:BENCH:INDEX")
        .is_err());
}

#[tokio::test]
async fn query_commit_preserves_manual_priority_and_delete_invalidates_coverage() {
    let f = fixture().await;
    let manual = quote("asset", "MANUAL", 999);
    f.store
        .upsert_quotes(std::slice::from_ref(&manual))
        .await
        .unwrap();
    let record = coverage("identity", "asset");
    f.store
        .commit_history_query(&record, &[quote("asset", "YAHOO", 100)], false, true, None)
        .await
        .unwrap();
    assert_eq!(
        f.store.get_historical_quotes("asset").unwrap(),
        vec![manual.clone()]
    );
    f.store.delete_quote(&manual.id).await.unwrap();
    assert!(f.store.history_coverage("identity").unwrap().is_empty());
}

#[tokio::test]
async fn failed_price_or_coverage_write_rolls_back_both() {
    let f = fixture().await;
    let missing_asset = coverage("missing", "not-an-asset");
    assert!(f
        .store
        .commit_history_query(
            &missing_asset,
            &[quote("not-an-asset", "YAHOO", 100)],
            false,
            true,
            None
        )
        .await
        .is_err());
    assert!(f.store.history_coverage("missing").unwrap().is_empty());
    diesel::sql_query(
        "CREATE TRIGGER fail_coverage BEFORE INSERT ON market_history_queries
        BEGIN SELECT RAISE(ABORT, 'test failure'); END",
    )
    .execute(&mut *db::get_connection(&f.pool).unwrap())
    .unwrap();
    assert!(f
        .store
        .commit_history_query(
            &coverage("identity", "asset"),
            &[quote("asset", "YAHOO", 100)],
            false,
            true,
            None
        )
        .await
        .is_err());
    assert!(f.store.get_historical_quotes("asset").unwrap().is_empty());
}

#[tokio::test]
async fn empty_queries_survive_restart_but_expire_and_partial_has_no_coverage() {
    let f = fixture().await;
    let mut record = coverage("empty", "asset");
    record.outcome = HistoryOutcome::Empty;
    f.store
        .commit_history_query(&record, &[], false, true, None)
        .await
        .unwrap();
    let reopened = MarketDataRepository::new(f.pool.clone(), f.writer.clone());
    let persisted = reopened.history_coverage("empty").unwrap();
    assert!(
        missing_history_ranges(record.start, record.end, &persisted, record.fetched_at).is_empty()
    );
    assert_eq!(
        missing_history_ranges(
            record.start,
            record.end,
            &persisted,
            record.fetched_at + Duration::hours(7)
        ),
        vec![(record.start, record.end)]
    );
    f.store
        .commit_history_query(
            &coverage("partial", "benchmark"),
            &[quote("benchmark", "YAHOO", 100)],
            true,
            false,
            None,
        )
        .await
        .unwrap();
    assert!(f.store.history_coverage("partial").unwrap().is_empty());
    assert_eq!(
        f.store
            .benchmark_history("partial", record.start, record.end)
            .unwrap()
            .len(),
        1
    );
    f.store.invalidate_history_coverage(None).await.unwrap();
    assert!(f.store.history_coverage("empty").unwrap().is_empty());
}

#[tokio::test]
async fn provider_delete_invalidates_range() {
    let f = fixture().await;
    f.store
        .commit_history_query(
            &coverage("identity", "asset"),
            &[quote("asset", "YAHOO", 100)],
            false,
            true,
            None,
        )
        .await
        .unwrap();
    f.store
        .delete_provider_quotes_for_asset(&AssetId::new("asset"))
        .await
        .unwrap();
    assert!(f.store.history_coverage("identity").unwrap().is_empty());
}

#[derive(Default)]
struct FakeProvider {
    ranges: std::sync::Mutex<Vec<(NaiveDate, NaiveDate)>>,
    response_mode: std::sync::atomic::AtomicU8,
    latest_calls: std::sync::atomic::AtomicUsize,
}

#[async_trait::async_trait]
impl wealthfolio_market_data::MarketDataProvider for FakeProvider {
    fn id(&self) -> &'static str {
        "YAHOO"
    }
    fn priority(&self) -> u8 {
        1
    }
    fn capabilities(&self) -> wealthfolio_market_data::ProviderCapabilities {
        wealthfolio_market_data::ProviderCapabilities {
            instrument_kinds: &[wealthfolio_market_data::InstrumentKind::Equity],
            coverage: wealthfolio_market_data::Coverage::global_best_effort(),
            supports_latest: true,
            supports_historical: true,
            supports_search: false,
            supports_profile: false,
            supports_dividends: false,
        }
    }
    fn rate_limit(&self) -> wealthfolio_market_data::RateLimit {
        wealthfolio_market_data::RateLimit {
            requests_per_minute: 1000,
            max_concurrency: 10,
            min_delay: std::time::Duration::ZERO,
        }
    }
    async fn get_historical_quotes_with_coverage(
        &self,
        context: &wealthfolio_market_data::QuoteContext,
        instrument: wealthfolio_market_data::ProviderInstrument,
        start: chrono::DateTime<Utc>,
        end: chrono::DateTime<Utc>,
    ) -> Result<
        wealthfolio_market_data::HistoricalQuotes,
        wealthfolio_market_data::errors::MarketDataError,
    > {
        self.get_historical_quotes(context, instrument, start, end)
            .await
            .map(|quotes| wealthfolio_market_data::HistoricalQuotes {
                quotes,
                complete: true,
            })
    }

    async fn get_latest_quote(
        &self,
        _: &wealthfolio_market_data::QuoteContext,
        _: wealthfolio_market_data::ProviderInstrument,
    ) -> Result<wealthfolio_market_data::Quote, wealthfolio_market_data::errors::MarketDataError>
    {
        self.latest_calls
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let timestamp = if self.response_mode.load(std::sync::atomic::Ordering::SeqCst) == 4 {
            Utc::now() + Duration::days(2)
        } else {
            Utc::now()
        };
        Ok(wealthfolio_market_data::Quote::new(
            timestamp,
            Decimal::new(100, 0),
            "USD".into(),
            "YAHOO".into(),
        ))
    }
    async fn get_historical_quotes(
        &self,
        _: &wealthfolio_market_data::QuoteContext,
        _: wealthfolio_market_data::ProviderInstrument,
        start: chrono::DateTime<Utc>,
        end: chrono::DateTime<Utc>,
    ) -> Result<Vec<wealthfolio_market_data::Quote>, wealthfolio_market_data::errors::MarketDataError>
    {
        self.ranges
            .lock()
            .unwrap()
            .push((start.date_naive(), end.date_naive()));
        tokio::task::yield_now().await;
        let mode = self.response_mode.load(std::sync::atomic::Ordering::SeqCst);
        if mode == 1 {
            return Ok(Vec::new());
        }
        if mode == 3 {
            return Err(
                wealthfolio_market_data::errors::MarketDataError::ProviderError {
                    provider: "YAHOO".into(),
                    message: "test provider failure".into(),
                },
            );
        }
        let mut day = start.date_naive();
        let mut quotes = Vec::new();
        while day <= end.date_naive() {
            quotes.push(wealthfolio_market_data::Quote::new(
                day.and_hms_opt(12, 0, 0).unwrap().and_utc(),
                Decimal::new(100, 0),
                "USD".into(),
                "YAHOO".into(),
            ));
            day = day.succ_opt().unwrap();
        }
        if mode == 2 {
            quotes[0].close = Decimal::new(-1, 0);
        }
        Ok(quotes)
    }
}

struct NoSecrets;
impl wealthfolio_core::secrets::SecretStore for NoSecrets {
    fn get_secret(&self, _: &str) -> wealthfolio_core::Result<Option<String>> {
        Ok(None)
    }
    fn set_secret(&self, _: &str, _: &str) -> wealthfolio_core::Result<()> {
        panic!("no secret writes")
    }
    fn delete_secret(&self, _: &str) -> wealthfolio_core::Result<()> {
        panic!("no secret writes")
    }
}

type SyncService = wealthfolio_core::quotes::QuoteSyncService<
    MarketDataRepository,
    wealthfolio_storage_sqlite::market_data::QuoteSyncStateRepository,
    AssetRepository,
    wealthfolio_storage_sqlite::activities::ActivityRepository,
>;

async fn sync_service(f: &Fixture, provider: Arc<FakeProvider>) -> SyncService {
    use wealthfolio_core::quotes::{MarketDataClient, ProviderConfig, QuoteSyncService};
    let client = MarketDataClient::new_with_extra(
        Arc::new(NoSecrets),
        vec![ProviderConfig {
            id: "YAHOO".into(),
            priority: 1,
        }],
        vec![provider],
    )
    .await
    .unwrap();
    QuoteSyncService::new(
        Arc::new(tokio::sync::RwLock::new(client)),
        Arc::new(MarketDataRepository::new(f.pool.clone(), f.writer.clone())),
        Arc::new(
            wealthfolio_storage_sqlite::market_data::QuoteSyncStateRepository::new(
                f.pool.clone(),
                f.writer.clone(),
            ),
        ),
        Arc::new(AssetRepository::new(f.pool.clone(), f.writer.clone())),
        Arc::new(
            wealthfolio_storage_sqlite::activities::ActivityRepository::new(
                f.pool.clone(),
                f.writer.clone(),
            ),
        ),
    )
}

#[tokio::test]
async fn local_hit_and_restarted_service_make_zero_additional_provider_requests() {
    let f = fixture().await;
    let provider = Arc::new(FakeProvider::default());
    let service = sync_service(&f, provider.clone()).await;
    let asset = AssetRepository::new(f.pool.clone(), f.writer.clone())
        .get_by_id("asset")
        .unwrap();
    let c = coverage("unused", "asset");
    assert_eq!(
        service
            .fetch_local_first_history(&asset, c.start, c.end, false)
            .await
            .unwrap()
            .len(),
        31
    );
    let calls = provider.ranges.lock().unwrap().len();
    assert_eq!(calls, 1);
    service
        .fetch_local_first_history(&asset, c.start, c.end, false)
        .await
        .unwrap();
    let restarted = sync_service(&f, provider.clone()).await;
    restarted
        .fetch_local_first_history(&asset, c.start, c.end, false)
        .await
        .unwrap();
    assert_eq!(provider.ranges.lock().unwrap().len(), calls);
}

#[tokio::test]
async fn queued_same_range_coalesces_but_larger_range_fetches_only_missing_prefix() {
    let f = fixture().await;
    let provider = Arc::new(FakeProvider::default());
    let service = sync_service(&f, provider.clone()).await;
    let asset = AssetRepository::new(f.pool.clone(), f.writer.clone())
        .get_by_id("asset")
        .unwrap();
    let c = coverage("unused", "asset");
    let later = c.start + Duration::days(10);
    let (a, b) = tokio::join!(
        service.fetch_local_first_history(&asset, later, c.end, false),
        service.fetch_local_first_history(&asset, later, c.end, false)
    );
    assert!(a.is_ok() && b.is_ok());
    assert_eq!(provider.ranges.lock().unwrap().len(), 1);
    service
        .fetch_local_first_history(&asset, c.start, c.end, false)
        .await
        .unwrap();
    assert_eq!(
        *provider.ranges.lock().unwrap(),
        vec![(later, c.end), (c.start, later.pred_opt().unwrap())]
    );
    assert_eq!(f.store.get_historical_quotes("asset").unwrap().len(), 31);
}

#[tokio::test]
async fn successful_empty_and_partial_responses_are_cached_but_errors_are_not() {
    for mode in [1, 2, 3] {
        let f = fixture().await;
        let provider = Arc::new(FakeProvider::default());
        provider
            .response_mode
            .store(mode, std::sync::atomic::Ordering::SeqCst);
        let service = sync_service(&f, provider.clone()).await;
        let asset = AssetRepository::new(f.pool.clone(), f.writer.clone())
            .get_by_id("asset")
            .unwrap();
        let c = coverage("unused", "asset");
        let first = service
            .fetch_local_first_history(&asset, c.start, c.end, false)
            .await;
        let second = service
            .fetch_local_first_history(&asset, c.start, c.end, false)
            .await;
        if mode == 3 {
            assert!(first.is_err() && second.is_err());
        } else {
            assert!(first.is_ok() && second.is_ok());
        }
        assert_eq!(
            provider.ranges.lock().unwrap().len(),
            if mode == 3 { 2 } else { 1 },
            "mode {mode}"
        );
    }
}

#[tokio::test]
async fn concurrent_different_ranges_preserve_both_callers_work() {
    let f = fixture().await;
    let provider = Arc::new(FakeProvider::default());
    let service = sync_service(&f, provider.clone()).await;
    let asset = AssetRepository::new(f.pool.clone(), f.writer.clone())
        .get_by_id("asset")
        .unwrap();
    let start = coverage("unused", "asset").start;
    let (a, b) = tokio::join!(
        service.fetch_local_first_history(&asset, start, start + Duration::days(9), false),
        service.fetch_local_first_history(
            &asset,
            start + Duration::days(4),
            start + Duration::days(19),
            false
        )
    );
    assert_eq!(a.unwrap().len(), 10);
    assert_eq!(b.unwrap().len(), 16);
    assert_eq!(f.store.get_historical_quotes("asset").unwrap().len(), 20);
    let ranges = provider.ranges.lock().unwrap();
    assert_eq!(ranges.len(), 2);
    // No overlapping interval is fetched twice even when the scopes differ.
    assert!(ranges[0].1 < ranges[1].0 || ranges[1].1 < ranges[0].0);
}

#[tokio::test]
async fn automatic_backfill_is_local_first_but_explicit_history_refresh_bypasses_coverage() {
    use wealthfolio_core::quotes::{QuoteSyncServiceTrait, SyncMode};
    let f = fixture().await;
    let provider = Arc::new(FakeProvider::default());
    let service = sync_service(&f, provider.clone()).await;
    let targets = Some(vec!["asset".to_owned()]);
    let first = service
        .sync(SyncMode::BackfillHistory { days: 30 }, targets.clone())
        .await
        .unwrap();
    assert!(first.quotes_synced > 0);
    let first_count = provider.ranges.lock().unwrap().len();
    service
        .sync(SyncMode::BackfillHistory { days: 30 }, targets.clone())
        .await
        .unwrap();
    assert_eq!(provider.ranges.lock().unwrap().len(), first_count);
    service
        .sync(SyncMode::RefreshAllHistory { days: 30 }, targets.clone())
        .await
        .unwrap();
    service
        .sync(SyncMode::RefreshAllHistory { days: 30 }, targets)
        .await
        .unwrap();
    assert_eq!(provider.ranges.lock().unwrap().len(), first_count + 2);
}

#[tokio::test]
async fn unknown_benchmark_reuses_persisted_history_without_catalog_insertion() {
    let f = fixture().await;
    let provider = Arc::new(FakeProvider::default());
    let service = sync_service(&f, provider.clone()).await;
    let mut asset = AssetRepository::new(f.pool.clone(), f.writer.clone())
        .get_by_id("asset")
        .unwrap();
    asset.id = "SEC:BENCH:INDEX".into();
    asset.instrument_symbol = Some("BENCH".into());
    let c = coverage("unused", &asset.id);
    let first = service
        .fetch_local_first_history(&asset, c.start, c.end, true)
        .await
        .unwrap();
    let restarted = sync_service(&f, provider.clone()).await;
    let second = restarted
        .fetch_local_first_history(&asset, c.start, c.end, true)
        .await
        .unwrap();
    assert_eq!(first, second);
    assert_eq!(provider.ranges.lock().unwrap().len(), 1);
    assert!(f.store.get_all_historical_quotes().unwrap().is_empty());
    assert!(AssetRepository::new(f.pool.clone(), f.writer.clone())
        .get_by_id(&asset.id)
        .is_err());
}

#[tokio::test]
async fn successful_identical_refresh_records_coverage_without_counting_price_changes() {
    let f = fixture().await;
    let record = coverage("identity", "asset");
    let original = quote("asset", "YAHOO", 100);
    assert_eq!(
        f.store
            .commit_history_query(&record, std::slice::from_ref(&original), false, true, None)
            .await
            .unwrap(),
        1
    );
    let mut repeated = original.clone();
    repeated.created_at = Utc::now() + Duration::seconds(1);
    repeated.id = "different-generated-id".into();
    assert_eq!(
        f.store
            .commit_history_query(&record, std::slice::from_ref(&repeated), false, true, None)
            .await
            .unwrap(),
        0
    );
    repeated.close = Decimal::new(101, 0);
    assert_eq!(
        f.store
            .commit_history_query(&record, &[repeated], false, true, None)
            .await
            .unwrap(),
        1
    );
    let benchmark = coverage("benchmark", "unknown");
    let first = quote("unknown", "YAHOO", 100);
    assert_eq!(
        f.store
            .commit_history_query(&benchmark, std::slice::from_ref(&first), true, true, None)
            .await
            .unwrap(),
        1
    );
    let mut duplicate = first;
    duplicate.created_at = Utc::now() + Duration::seconds(1);
    assert_eq!(
        f.store
            .commit_history_query(&benchmark, &[duplicate], true, true, None)
            .await
            .unwrap(),
        0
    );
}

#[tokio::test]
async fn partial_response_counts_only_changes_and_never_records_coverage() {
    let f = fixture().await;
    let record = coverage("partial", "asset");
    let quotes = [quote("asset", "YAHOO", 100)];
    assert_eq!(
        f.store
            .commit_history_query(&record, &quotes, false, false, None)
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        f.store
            .commit_history_query(&record, &quotes, false, false, None)
            .await
            .unwrap(),
        0
    );
    assert!(f.store.history_coverage("partial").unwrap().is_empty());
}

#[tokio::test]
async fn queued_profile_change_rejects_complete_and_partial_writes_atomically() {
    for complete in [true, false] {
        let f = fixture().await;
        let expected = f.store.history_write_context("asset").unwrap().unwrap();
        diesel::sql_query("UPDATE assets SET quote_ccy='EUR' WHERE id='asset'")
            .execute(&mut *db::get_connection(&f.pool).unwrap())
            .unwrap();
        let record = coverage("stale-config", "asset");
        assert!(f
            .store
            .commit_history_query(
                &record,
                &[quote("asset", "YAHOO", 100)],
                false,
                complete,
                Some(&expected)
            )
            .await
            .is_err());
        assert!(f.store.get_historical_quotes("asset").unwrap().is_empty());
        assert!(f.store.history_coverage("stale-config").unwrap().is_empty());
    }
}

#[tokio::test]
async fn new_partial_receipt_supersedes_old_complete_only_within_queried_range() {
    let f = fixture().await;
    let old = coverage("identity", "asset");
    f.store
        .commit_history_query(&old, &[quote("asset", "YAHOO", 100)], false, true, None)
        .await
        .unwrap();
    let mut partial = old.clone();
    partial.start = old.start + Duration::days(9);
    partial.end = old.start + Duration::days(19);
    partial.outcome = HistoryOutcome::Partial;
    f.store
        .commit_history_query(&partial, &[quote("asset", "YAHOO", 101)], false, true, None)
        .await
        .unwrap();
    let receipts = f.store.history_coverage("identity").unwrap();
    assert_eq!(receipts.len(), 3);
    assert_eq!(
        missing_history_ranges(
            old.start,
            old.end,
            &receipts,
            partial.fetched_at + Duration::hours(7)
        ),
        vec![(partial.start, partial.end)]
    );
    // A full-history partial response must remove every prior Complete claim.
    partial.start = old.start;
    partial.end = old.end;
    f.store
        .commit_history_query(&partial, &[quote("asset", "YAHOO", 101)], false, true, None)
        .await
        .unwrap();
    let receipts = f.store.history_coverage("identity").unwrap();
    assert_eq!(receipts.len(), 1);
    assert_eq!(
        missing_history_ranges(
            old.start,
            old.end,
            &receipts,
            partial.fetched_at + Duration::hours(7)
        ),
        vec![(old.start, old.end)]
    );
}

#[tokio::test]
async fn custom_provider_revision_changes_identity_and_transaction_guard_without_config_values() {
    let f = fixture().await;
    let initial = f.store.history_configuration_revision().unwrap();
    diesel::sql_query(
        "INSERT INTO market_data_custom_providers
        (id,code,name,description,enabled,priority,config,created_at,updated_at)
        VALUES ('custom','custom','Custom','test',1,1,NULL,'v1','v1')",
    )
    .execute(&mut *db::get_connection(&f.pool).unwrap())
    .unwrap();
    let inserted = f.store.history_configuration_revision().unwrap();
    assert_ne!(initial, inserted);
    let expected = f.store.history_write_context("asset").unwrap().unwrap();
    diesel::sql_query("UPDATE market_data_custom_providers SET updated_at='v2' WHERE id='custom'")
        .execute(&mut *db::get_connection(&f.pool).unwrap())
        .unwrap();
    assert_ne!(inserted, f.store.history_configuration_revision().unwrap());
    assert!(f
        .store
        .commit_history_query(
            &coverage("identity", "asset"),
            &[quote("asset", "YAHOO", 100)],
            false,
            true,
            Some(&expected)
        )
        .await
        .is_err());
    assert!(f.store.get_historical_quotes("asset").unwrap().is_empty());
}

#[tokio::test]
async fn benchmark_receipt_cannot_cover_a_newly_cataloged_assets_quote_store() {
    let f = fixture().await;
    let provider = Arc::new(FakeProvider::default());
    let service = sync_service(&f, provider.clone()).await;
    let asset = AssetRepository::new(f.pool.clone(), f.writer.clone())
        .get_by_id("asset")
        .unwrap();
    let assets = AssetRepository::new(f.pool.clone(), f.writer.clone());
    assets.delete(&asset.id).await.unwrap();
    let range = coverage("unused", "asset");
    // Identical canonical identity can move from benchmark-only to cataloged.
    service
        .fetch_local_first_history(&asset, range.start, range.end, true)
        .await
        .unwrap();
    assert!(f.store.get_historical_quotes(&asset.id).unwrap().is_empty());
    assets
        .create(NewAsset {
            id: Some(asset.id.clone()),
            kind: asset.kind.clone(),
            name: asset.name.clone(),
            display_code: asset.display_code.clone(),
            is_active: asset.is_active,
            quote_mode: asset.quote_mode,
            quote_ccy: asset.quote_ccy.clone(),
            instrument_type: asset.instrument_type.clone(),
            instrument_symbol: asset.instrument_symbol.clone(),
            instrument_exchange_mic: asset.instrument_exchange_mic.clone(),
            provider_config: asset.provider_config.clone(),
            metadata: asset.metadata.clone(),
            ..Default::default()
        })
        .await
        .unwrap();
    let actual = service
        .fetch_local_first_history(&asset, range.start, range.end, false)
        .await
        .unwrap();
    assert_eq!(actual.len(), 31);
    assert_eq!(provider.ranges.lock().unwrap().len(), 2);
    assert_eq!(f.store.get_historical_quotes(&asset.id).unwrap().len(), 31);
}

#[tokio::test]
async fn future_latest_response_writes_neither_quotes_nor_receipts() {
    let f = fixture().await;
    let provider = Arc::new(FakeProvider::default());
    provider
        .response_mode
        .store(4, std::sync::atomic::Ordering::SeqCst);
    let service = sync_service(&f, provider.clone()).await;
    let asset = AssetRepository::new(f.pool.clone(), f.writer.clone())
        .get_by_id("asset")
        .unwrap();
    // Preserve the provider response contract, but do not certify or persist
    // an observation beyond the current observable time.
    assert!(service
        .fetch_local_first_latest(&asset, false)
        .await
        .is_ok());
    assert_eq!(
        provider
            .latest_calls
            .load(std::sync::atomic::Ordering::SeqCst),
        1
    );
    assert!(f.store.get_historical_quotes("asset").unwrap().is_empty());
    #[derive(QueryableByName)]
    struct Count {
        #[diesel(sql_type = diesel::sql_types::BigInt)]
        count: i64,
    }
    let count = diesel::sql_query("SELECT COUNT(*) AS count FROM market_history_queries")
        .get_result::<Count>(&mut *db::get_connection(&f.pool).unwrap())
        .unwrap();
    assert_eq!(count.count, 0);
}

#[tokio::test]
async fn covered_history_resolves_latest_without_another_provider_request() {
    let f = fixture().await;
    let provider = Arc::new(FakeProvider::default());
    let service = sync_service(&f, provider.clone()).await;
    let asset = AssetRepository::new(f.pool.clone(), f.writer.clone())
        .get_by_id("asset")
        .unwrap();
    let today =
        wealthfolio_core::utils::time_utils::market_calendar_date(Utc::now(), Some("XNAS"), false);
    let history = service
        .fetch_local_first_history(&asset, today - Duration::days(10), today, false)
        .await
        .unwrap();
    let latest = service
        .fetch_local_first_latest(&asset, false)
        .await
        .unwrap();
    assert_eq!(latest, history.last().unwrap().clone());
    assert_eq!(
        provider
            .latest_calls
            .load(std::sync::atomic::Ordering::SeqCst),
        0
    );
    assert_eq!(provider.ranges.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn successful_unchanged_refresh_clears_health_error_without_price_changes() {
    use wealthfolio_core::quotes::{QuoteSyncServiceTrait, SyncMode, SyncStateStore};
    let f = fixture().await;
    let provider = Arc::new(FakeProvider::default());
    let service = sync_service(&f, provider).await;
    let targets = Some(vec!["asset".to_owned()]);
    let first = service
        .sync(SyncMode::BackfillHistory { days: 10 }, targets.clone())
        .await
        .unwrap();
    assert!(first.quotes_synced > 0);
    let states = wealthfolio_storage_sqlite::market_data::QuoteSyncStateRepository::new(
        f.pool.clone(),
        f.writer.clone(),
    );
    states
        .update_after_failure("asset", "temporary provider failure")
        .await
        .unwrap();
    let refreshed = service
        .sync(SyncMode::RefreshAllHistory { days: 10 }, targets)
        .await
        .unwrap();
    assert_eq!(refreshed.quotes_synced, 0);
    let state = states.get_by_asset_id("asset").unwrap().unwrap();
    assert_eq!(state.error_count, 0);
    assert!(state.last_error.is_none());
}

#[tokio::test]
async fn explicit_invalidation_clears_derived_benchmark_rows_before_partial_refill() {
    let f = fixture().await;
    let record = coverage("benchmark", "BENCH");
    let original = quote("BENCH", "YAHOO", 100);
    let mut old_extra = original.clone();
    old_extra.timestamp += Duration::days(1);
    old_extra.id = "old-extra-day".into();
    f.store
        .commit_history_query(&record, &[original.clone(), old_extra], true, true, None)
        .await
        .unwrap();
    f.store
        .upsert_quotes(&[quote("asset", "MANUAL", 999)])
        .await
        .unwrap();
    f.store.invalidate_history_coverage(None).await.unwrap();
    assert!(f
        .store
        .benchmark_history("benchmark", record.start, record.end)
        .unwrap()
        .is_empty());
    assert!(f.store.history_coverage("benchmark").unwrap().is_empty());
    let partial = HistoryCoverage {
        outcome: HistoryOutcome::Partial,
        ..record.clone()
    };
    let mut replacement = original;
    replacement.close = Decimal::new(101, 0);
    f.store
        .commit_history_query(
            &partial,
            std::slice::from_ref(&replacement),
            true,
            true,
            None,
        )
        .await
        .unwrap();
    assert_eq!(
        f.store
            .benchmark_history("benchmark", record.start, record.end)
            .unwrap(),
        vec![replacement]
    );
    assert_eq!(
        f.store.get_historical_quotes("asset").unwrap()[0].close,
        Decimal::new(999, 0)
    );
}

#[tokio::test]
async fn asset_invalidation_preserves_unrelated_benchmarks_and_user_quotes() {
    let f = fixture().await;
    for (identity, asset) in [("one", "BENCH-A"), ("two", "BENCH-B")] {
        f.store
            .commit_history_query(
                &coverage(identity, asset),
                &[quote(asset, "YAHOO", 100)],
                true,
                true,
                None,
            )
            .await
            .unwrap();
    }
    f.store
        .upsert_quotes(&[quote("asset", "MANUAL", 999)])
        .await
        .unwrap();
    f.store
        .invalidate_history_coverage(Some("BENCH-A"))
        .await
        .unwrap();
    let bounds = coverage("unused", "unused");
    assert!(f
        .store
        .benchmark_history("one", bounds.start, bounds.end)
        .unwrap()
        .is_empty());
    assert!(f.store.history_coverage("one").unwrap().is_empty());
    assert_eq!(
        f.store
            .benchmark_history("two", bounds.start, bounds.end)
            .unwrap()
            .len(),
        1
    );
    assert_eq!(f.store.history_coverage("two").unwrap().len(), 1);
    assert_eq!(f.store.get_historical_quotes("asset").unwrap().len(), 1);
}

#[tokio::test]
async fn direct_custom_provider_mutations_clear_only_derived_cache_tables() {
    let f = fixture().await;
    let mut provider_quote = quote("asset", "YAHOO", 100);
    provider_quote.timestamp += Duration::days(1);
    provider_quote.id = "real-provider-price".into();
    f.store
        .upsert_quotes(&[quote("asset", "MANUAL", 999), provider_quote])
        .await
        .unwrap();
    let bounds = coverage("benchmark", "BENCH");
    for mutation in [
        "INSERT INTO market_data_custom_providers
            (id,code,name,description,enabled,priority,config,created_at,updated_at)
            VALUES ('lifecycle','lifecycle','Custom','test',1,1,NULL,'v1','v1')",
        "UPDATE market_data_custom_providers SET updated_at='v2' WHERE id='lifecycle'",
        "DELETE FROM market_data_custom_providers WHERE id='lifecycle'",
    ] {
        f.store
            .commit_history_query(&bounds, &[quote("BENCH", "YAHOO", 100)], true, true, None)
            .await
            .unwrap();
        diesel::sql_query(mutation)
            .execute(&mut *db::get_connection(&f.pool).unwrap())
            .unwrap();
        assert!(f
            .store
            .benchmark_history("benchmark", bounds.start, bounds.end)
            .unwrap()
            .is_empty());
        assert!(f.store.history_coverage("benchmark").unwrap().is_empty());
        let actual = f.store.get_historical_quotes("asset").unwrap();
        assert_eq!(actual.len(), 2);
        assert!(actual
            .iter()
            .any(|q| q.data_source == "MANUAL" && q.close == Decimal::new(999, 0)));
        assert!(actual
            .iter()
            .any(|q| q.data_source == "YAHOO" && q.close == Decimal::new(100, 0)));
    }
}
