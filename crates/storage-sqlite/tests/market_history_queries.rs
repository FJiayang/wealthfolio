//! Successful query persistence must remain atomic and profile-local.
#![allow(clippy::unwrap_used, clippy::panic, reason = "test code")]
use std::sync::Arc;

use chrono::{Duration, NaiveDate, Utc};
use diesel::prelude::*;
use rust_decimal::Decimal;
use wealthfolio_core::assets::{
    AssetKind, AssetRepositoryTrait, InstrumentType, NewAsset, QuoteMode,
};
use wealthfolio_core::quotes::history::{
    missing_history_ranges, HistoryCoverage, HistoryOutcome, HistoryQueryKind, HistoryWriteMode,
};
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
        .commit_history_query(
            &record,
            &[quote("asset", "YAHOO", 100)],
            HistoryWriteMode::Canonical,
            true,
            None,
        )
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
            HistoryWriteMode::DerivedRange,
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
        .commit_history_query(
            &record,
            &[quote("asset", "YAHOO", 100)],
            HistoryWriteMode::Canonical,
            true,
            None,
        )
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
            HistoryWriteMode::Canonical,
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
            HistoryWriteMode::Canonical,
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
        .commit_history_query(&record, &[], HistoryWriteMode::Canonical, true, None)
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
            HistoryWriteMode::DerivedRange,
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
            HistoryWriteMode::Canonical,
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
    request_started: tokio::sync::Notify,
    release_request: tokio::sync::Notify,
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
                complete: self.response_mode.load(std::sync::atomic::Ordering::SeqCst) != 6,
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
        if mode == 5 {
            self.request_started.notify_one();
            self.release_request.notified().await;
        }
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
        let mut day = if mode == 6 {
            start
                .date_naive()
                .max(end.date_naive() - Duration::days(99))
        } else {
            start.date_naive()
        };
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
    use wealthfolio_core::quotes::{MarketDataClient, ProviderConfig};
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
    sync_service_with_client(f, client)
}

fn sync_service_with_client(
    f: &Fixture,
    client: wealthfolio_core::quotes::MarketDataClient,
) -> SyncService {
    wealthfolio_core::quotes::QuoteSyncService::new(
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
        .sync(SyncMode::RefreshAllHistory { days: 30 }, targets.clone())
        .await
        .unwrap();
    assert_eq!(provider.ranges.lock().unwrap().len(), first_count + 2);
    // The legacy endpoint/addon alias is the same explicit full-history action,
    // not an automatic backfill. Even its second, already-covered call refetches.
    service.resync(targets.clone()).await.unwrap();
    service.resync(targets).await.unwrap();
    assert_eq!(provider.ranges.lock().unwrap().len(), first_count + 4);
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
            .commit_history_query(
                &record,
                std::slice::from_ref(&original),
                HistoryWriteMode::Canonical,
                true,
                None
            )
            .await
            .unwrap(),
        1
    );
    let mut repeated = original.clone();
    repeated.created_at = Utc::now() + Duration::seconds(1);
    repeated.id = "different-generated-id".into();
    assert_eq!(
        f.store
            .commit_history_query(
                &record,
                std::slice::from_ref(&repeated),
                HistoryWriteMode::Canonical,
                true,
                None
            )
            .await
            .unwrap(),
        0
    );
    repeated.close = Decimal::new(101, 0);
    assert_eq!(
        f.store
            .commit_history_query(
                &record,
                &[repeated],
                HistoryWriteMode::Canonical,
                true,
                None
            )
            .await
            .unwrap(),
        1
    );
    let benchmark = coverage("benchmark", "unknown");
    let first = quote("unknown", "YAHOO", 100);
    assert_eq!(
        f.store
            .commit_history_query(
                &benchmark,
                std::slice::from_ref(&first),
                HistoryWriteMode::DerivedRange,
                true,
                None
            )
            .await
            .unwrap(),
        1
    );
    let mut duplicate = first;
    duplicate.created_at = Utc::now() + Duration::seconds(1);
    assert_eq!(
        f.store
            .commit_history_query(
                &benchmark,
                &[duplicate],
                HistoryWriteMode::DerivedRange,
                true,
                None
            )
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
            .commit_history_query(&record, &quotes, HistoryWriteMode::Canonical, false, None)
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        f.store
            .commit_history_query(&record, &quotes, HistoryWriteMode::Canonical, false, None)
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
        let asset = AssetRepository::new(f.pool.clone(), f.writer.clone())
            .get_by_id("asset")
            .unwrap();
        let expected = f
            .store
            .history_write_context(&asset, HistoryQueryKind::History, true)
            .unwrap()
            .unwrap();
        diesel::sql_query("UPDATE assets SET quote_ccy='EUR' WHERE id='asset'")
            .execute(&mut *db::get_connection(&f.pool).unwrap())
            .unwrap();
        let record = coverage("stale-config", "asset");
        assert!(f
            .store
            .commit_history_query(
                &record,
                &[quote("asset", "YAHOO", 100)],
                HistoryWriteMode::Canonical,
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
        .commit_history_query(
            &old,
            &[quote("asset", "YAHOO", 100)],
            HistoryWriteMode::Canonical,
            true,
            None,
        )
        .await
        .unwrap();
    let mut partial = old.clone();
    partial.start = old.start + Duration::days(9);
    partial.end = old.start + Duration::days(19);
    partial.outcome = HistoryOutcome::Partial;
    f.store
        .commit_history_query(
            &partial,
            &[quote("asset", "YAHOO", 101)],
            HistoryWriteMode::Canonical,
            true,
            None,
        )
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
        .commit_history_query(
            &partial,
            &[quote("asset", "YAHOO", 101)],
            HistoryWriteMode::Canonical,
            true,
            None,
        )
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

async fn create_custom(f: &Fixture, code: &str, fallback: bool, kind: &str, priority: i32) {
    use wealthfolio_core::custom_provider::CustomProviderRepository;
    let payload = serde_json::from_value(serde_json::json!({
        "code": code, "name": code, "priority": priority, "useAsFallback": fallback,
        "sources": [{"kind":kind,"format":"json","url":"https://example.invalid/{SYMBOL}","pricePath":"price"}]
    })).unwrap();
    wealthfolio_storage_sqlite::custom_provider::CustomProviderSqliteRepository::new(
        f.pool.clone(),
        f.writer.clone(),
    )
    .create(&payload)
    .await
    .unwrap();
}

async fn rename_custom(f: &Fixture, code: &str) {
    use wealthfolio_core::custom_provider::CustomProviderRepository;
    let payload = serde_json::from_value(serde_json::json!({"name":"Updated label"})).unwrap();
    wealthfolio_storage_sqlite::custom_provider::CustomProviderSqliteRepository::new(
        f.pool.clone(),
        f.writer.clone(),
    )
    .update(code, &payload)
    .await
    .unwrap();
}

fn assigned_asset(f: &Fixture, code: &str) -> wealthfolio_core::assets::Asset {
    let mut asset = AssetRepository::new(f.pool.clone(), f.writer.clone())
        .get_by_id("asset")
        .unwrap();
    asset.provider_config = Some(
        serde_json::json!({"preferred_provider":"CUSTOM_SCRAPER","custom_provider_code":code}),
    );
    asset
}

#[tokio::test]
async fn custom_revision_scopes_official_updates_and_guards_unknown_benchmark_commits() {
    let f = fixture().await;
    create_custom(&f, "a", false, "historical", 1).await;
    create_custom(&f, "b", false, "historical", 2).await;
    let mut a = assigned_asset(&f, "a");
    a.id = "unknown-benchmark".into();
    let b = assigned_asset(&f, "b");
    let builtin = AssetRepository::new(f.pool.clone(), f.writer.clone())
        .get_by_id("asset")
        .unwrap();
    let a_before = f
        .store
        .history_configuration_revision(&a, HistoryQueryKind::History)
        .unwrap();
    let b_before = f
        .store
        .history_configuration_revision(&b, HistoryQueryKind::History)
        .unwrap();
    let builtin_before = f
        .store
        .history_configuration_revision(&builtin, HistoryQueryKind::History)
        .unwrap();
    let expected = f
        .store
        .history_write_context(&a, HistoryQueryKind::History, false)
        .unwrap()
        .unwrap();
    rename_custom(&f, "a").await;
    assert_ne!(
        a_before,
        f.store
            .history_configuration_revision(&a, HistoryQueryKind::History)
            .unwrap()
    );
    assert_eq!(
        b_before,
        f.store
            .history_configuration_revision(&b, HistoryQueryKind::History)
            .unwrap()
    );
    assert_eq!(
        builtin_before,
        f.store
            .history_configuration_revision(&builtin, HistoryQueryKind::History)
            .unwrap()
    );
    let record = coverage("stale-custom", &a.id);
    assert!(f
        .store
        .commit_history_query(
            &record,
            &[quote(&a.id, "CUSTOM_SCRAPER:a", 100)],
            HistoryWriteMode::DerivedRange,
            true,
            Some(&expected)
        )
        .await
        .is_err());
    assert!(f
        .store
        .benchmark_history(&record.identity, record.start, record.end)
        .unwrap()
        .is_empty());
    assert!(f
        .store
        .history_coverage(&record.identity)
        .unwrap()
        .is_empty());
    // Device sync preserves the version supplied by the originating official API.
    let before = f
        .store
        .history_configuration_revision(&b, HistoryQueryKind::History)
        .unwrap();
    diesel::sql_query("UPDATE market_data_custom_providers SET name='Synced label', updated_at='2099-01-01T00:00:00Z' WHERE code='b'")
        .execute(&mut *db::get_connection(&f.pool).unwrap()).unwrap();
    assert_ne!(
        before,
        f.store
            .history_configuration_revision(&b, HistoryQueryKind::History)
            .unwrap()
    );
}

#[tokio::test]
async fn fallback_revision_tracks_eligible_order_and_actual_history_latest_fallback() {
    use wealthfolio_core::custom_provider::CustomProviderRepository;
    let f = fixture().await;
    let asset = AssetRepository::new(f.pool.clone(), f.writer.clone())
        .get_by_id("asset")
        .unwrap();
    create_custom(&f, "private", false, "historical", 0).await;
    let empty = f
        .store
        .history_configuration_revision(&asset, HistoryQueryKind::History)
        .unwrap();
    rename_custom(&f, "private").await;
    assert_eq!(
        empty,
        f.store
            .history_configuration_revision(&asset, HistoryQueryKind::History)
            .unwrap()
    );
    create_custom(&f, "latest", true, "latest", 1).await;
    let latest_fallback = f
        .store
        .history_configuration_revision(&asset, HistoryQueryKind::History)
        .unwrap();
    assert_ne!(empty, latest_fallback);
    create_custom(&f, "history", true, "historical", 2).await;
    let history = f
        .store
        .history_configuration_revision(&asset, HistoryQueryKind::History)
        .unwrap();
    assert_ne!(latest_fallback, history);
    rename_custom(&f, "latest").await;
    assert_eq!(
        history,
        f.store
            .history_configuration_revision(&asset, HistoryQueryKind::History)
            .unwrap()
    );
    create_custom(&f, "history-two", true, "historical", 3).await;
    let ordered = f
        .store
        .history_configuration_revision(&asset, HistoryQueryKind::History)
        .unwrap();
    let repo = wealthfolio_storage_sqlite::custom_provider::CustomProviderSqliteRepository::new(
        f.pool.clone(),
        f.writer.clone(),
    );
    repo.update(
        "history-two",
        &serde_json::from_value(serde_json::json!({"priority":1})).unwrap(),
    )
    .await
    .unwrap();
    assert_ne!(
        ordered,
        f.store
            .history_configuration_revision(&asset, HistoryQueryKind::History)
            .unwrap()
    );
    repo.update(
        "history",
        &serde_json::from_value(serde_json::json!({"enabled":false})).unwrap(),
    )
    .await
    .unwrap();
    let disabled = f
        .store
        .history_configuration_revision(&asset, HistoryQueryKind::History)
        .unwrap();
    repo.delete("history-two").await.unwrap();
    assert_ne!(
        disabled,
        f.store
            .history_configuration_revision(&asset, HistoryQueryKind::History)
            .unwrap()
    );
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

    // Uncacheable provider observations must not replace ordinary manual output.
    f.store
        .upsert_quotes(&[quote("asset", "MANUAL", 999)])
        .await
        .unwrap();
    let merged = service
        .fetch_local_first_latest(&asset, false)
        .await
        .unwrap();
    assert_eq!(merged.data_source, "MANUAL");
    assert_eq!(merged.close, Decimal::new(999, 0));
    assert_eq!(f.store.get_historical_quotes("asset").unwrap().len(), 1);
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
        .commit_history_query(
            &record,
            &[original.clone(), old_extra],
            HistoryWriteMode::DerivedRange,
            true,
            None,
        )
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
            HistoryWriteMode::DerivedRange,
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
                HistoryWriteMode::DerivedRange,
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
async fn unrelated_official_custom_update_preserves_cached_rows_receipts_and_user_quotes() {
    let f = fixture().await;
    create_custom(&f, "private", false, "historical", 1).await;
    let bounds = coverage("unrelated-benchmark", "BENCH");
    let original = quote("BENCH", "YAHOO", 100);
    f.store
        .commit_history_query(
            &bounds,
            std::slice::from_ref(&original),
            HistoryWriteMode::DerivedRange,
            true,
            None,
        )
        .await
        .unwrap();
    let mut provider_quote = quote("asset", "YAHOO", 100);
    provider_quote.timestamp += Duration::days(1);
    f.store
        .upsert_quotes(&[quote("asset", "MANUAL", 999), provider_quote])
        .await
        .unwrap();
    rename_custom(&f, "private").await;
    assert_eq!(
        f.store
            .benchmark_history(&bounds.identity, bounds.start, bounds.end)
            .unwrap(),
        vec![original]
    );
    assert_eq!(
        f.store.history_coverage(&bounds.identity).unwrap(),
        vec![bounds]
    );
    assert_eq!(f.store.get_historical_quotes("asset").unwrap().len(), 2);
}

#[tokio::test]
async fn successful_benchmark_partial_replaces_only_queried_interval() {
    let f = fixture().await;
    let record = coverage("derived", "BENCH");
    let middle = quote("BENCH", "YAHOO", 100);
    let mut prefix = middle.clone();
    prefix.timestamp -= Duration::days(5);
    let mut suffix = middle.clone();
    suffix.timestamp += Duration::days(5);
    let mut omitted = middle.clone();
    omitted.timestamp += Duration::days(1);
    f.store
        .commit_history_query(
            &record,
            &[prefix.clone(), middle.clone(), omitted, suffix.clone()],
            HistoryWriteMode::DerivedRange,
            true,
            None,
        )
        .await
        .unwrap();
    f.store
        .upsert_quotes(&[quote("asset", "MANUAL", 999)])
        .await
        .unwrap();
    let partial = HistoryCoverage {
        start: middle.timestamp.date_naive(),
        end: middle.timestamp.date_naive() + Duration::days(1),
        outcome: HistoryOutcome::Partial,
        ..record.clone()
    };
    // An unchanged returned row is not counted again; removal is a real change.
    assert_eq!(
        f.store
            .commit_history_query(
                &partial,
                std::slice::from_ref(&middle),
                HistoryWriteMode::DerivedRange,
                true,
                None
            )
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        f.store
            .benchmark_history("derived", record.start, record.end)
            .unwrap(),
        vec![prefix, middle, suffix]
    );
    assert!(f
        .store
        .history_coverage("derived")
        .unwrap()
        .iter()
        .any(|receipt| receipt.start == partial.start
            && receipt.end == partial.end
            && receipt.outcome == HistoryOutcome::Partial));
    assert_eq!(
        f.store.get_historical_quotes("asset").unwrap()[0].close,
        Decimal::new(999, 0)
    );
}

#[tokio::test]
async fn benchmark_empty_clears_window_and_failed_commit_preserves_old_rows() {
    let f = fixture().await;
    let record = coverage("derived", "BENCH");
    let middle = quote("BENCH", "YAHOO", 100);
    let mut prefix = middle.clone();
    prefix.timestamp -= Duration::days(5);
    f.store
        .commit_history_query(
            &record,
            &[prefix.clone(), middle.clone()],
            HistoryWriteMode::DerivedRange,
            true,
            None,
        )
        .await
        .unwrap();
    let empty = HistoryCoverage {
        start: middle.timestamp.date_naive(),
        end: middle.timestamp.date_naive(),
        outcome: HistoryOutcome::Empty,
        ..record.clone()
    };
    diesel::sql_query(
        "CREATE TRIGGER fail_derived_receipt BEFORE INSERT ON market_history_queries
         BEGIN SELECT RAISE(ABORT, 'test failure'); END",
    )
    .execute(&mut *db::get_connection(&f.pool).unwrap())
    .unwrap();
    // Even a failure after interval deletion rolls back both cache and receipt.
    assert!(f
        .store
        .commit_history_query(&empty, &[], HistoryWriteMode::DerivedRange, true, None)
        .await
        .is_err());
    assert_eq!(
        f.store
            .benchmark_history("derived", record.start, record.end)
            .unwrap(),
        vec![prefix.clone(), middle]
    );
    assert_eq!(
        f.store.history_coverage("derived").unwrap(),
        vec![record.clone()]
    );
    diesel::sql_query("DROP TRIGGER fail_derived_receipt")
        .execute(&mut *db::get_connection(&f.pool).unwrap())
        .unwrap();
    assert_eq!(
        f.store
            .commit_history_query(&empty, &[], HistoryWriteMode::DerivedRange, true, None)
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        f.store
            .benchmark_history("derived", record.start, record.end)
            .unwrap(),
        vec![prefix]
    );
}

#[tokio::test]
async fn benchmark_provider_failure_preserves_previously_verified_rows() {
    let f = fixture().await;
    let provider = Arc::new(FakeProvider::default());
    let service = sync_service(&f, provider.clone()).await;
    let mut asset = AssetRepository::new(f.pool.clone(), f.writer.clone())
        .get_by_id("asset")
        .unwrap();
    asset.id = "SEC:BENCH:INDEX".into();
    asset.instrument_symbol = Some("BENCH".into());
    let bounds = coverage("unused", &asset.id);
    let original = service
        .fetch_local_first_history(&asset, bounds.start, bounds.end, true)
        .await
        .unwrap();
    #[derive(diesel::QueryableByName)]
    struct Identity {
        #[diesel(sql_type = diesel::sql_types::Text)]
        identity: String,
    }
    let mut conn = db::get_connection(&f.pool).unwrap();
    let identity = diesel::sql_query("SELECT identity FROM market_history_queries LIMIT 1")
        .get_result::<Identity>(&mut *conn)
        .unwrap()
        .identity;
    // Remove only the proof to force verification, retaining the derived prices.
    diesel::sql_query("DELETE FROM market_history_queries")
        .execute(&mut *conn)
        .unwrap();
    drop(conn);
    provider
        .response_mode
        .store(3, std::sync::atomic::Ordering::SeqCst);
    assert!(service
        .fetch_local_first_history(&asset, bounds.start, bounds.end, true)
        .await
        .is_err());
    assert_eq!(provider.ranges.lock().unwrap().len(), 2);
    assert_eq!(
        f.store
            .benchmark_history(&identity, bounds.start, bounds.end)
            .unwrap(),
        original
    );
    assert!(f.store.history_coverage(&identity).unwrap().is_empty());
}

#[tokio::test]
async fn derived_latest_replaces_newer_cached_day_but_failed_commit_preserves_it() {
    let f = fixture().await;
    let record = coverage("latest", "BENCH");
    let mut sunday = quote("BENCH", "YAHOO", 100);
    sunday.timestamp = NaiveDate::from_ymd_opt(2025, 1, 19)
        .unwrap()
        .and_hms_opt(12, 0, 0)
        .unwrap()
        .and_utc();
    f.store
        .commit_history_query(
            &record,
            std::slice::from_ref(&sunday),
            HistoryWriteMode::DerivedLatest,
            true,
            None,
        )
        .await
        .unwrap();
    let mut friday = sunday.clone();
    friday.timestamp -= Duration::days(2);
    let friday_scope = HistoryCoverage {
        start: friday.timestamp.date_naive(),
        end: friday.timestamp.date_naive(),
        ..record.clone()
    };
    diesel::sql_query("CREATE TRIGGER fail_latest_receipt BEFORE INSERT ON market_history_queries BEGIN SELECT RAISE(ABORT, 'test failure'); END")
        .execute(&mut *db::get_connection(&f.pool).unwrap()).unwrap();
    assert!(f
        .store
        .commit_history_query(
            &friday_scope,
            std::slice::from_ref(&friday),
            HistoryWriteMode::DerivedLatest,
            true,
            None
        )
        .await
        .is_err());
    assert_eq!(
        f.store
            .benchmark_history("latest", record.start, record.end)
            .unwrap(),
        vec![sunday]
    );
    assert_eq!(
        f.store.history_coverage("latest").unwrap(),
        vec![record.clone()]
    );
    diesel::sql_query("DROP TRIGGER fail_latest_receipt")
        .execute(&mut *db::get_connection(&f.pool).unwrap())
        .unwrap();
    f.store
        .commit_history_query(
            &friday_scope,
            std::slice::from_ref(&friday),
            HistoryWriteMode::DerivedLatest,
            true,
            None,
        )
        .await
        .unwrap();
    assert_eq!(
        f.store
            .benchmark_history("latest", record.start, record.end)
            .unwrap(),
        vec![friday]
    );
    assert_eq!(
        f.store.history_coverage("latest").unwrap(),
        vec![friday_scope]
    );
}

#[tokio::test]
async fn cancelling_provider_wait_writes_nothing_and_releases_same_asset_lock() {
    let f = fixture().await;
    let provider = Arc::new(FakeProvider::default());
    provider
        .response_mode
        .store(5, std::sync::atomic::Ordering::SeqCst);
    let service = Arc::new(sync_service(&f, provider.clone()).await);
    let asset = AssetRepository::new(f.pool.clone(), f.writer.clone())
        .get_by_id("asset")
        .unwrap();
    let bounds = coverage("unused", &asset.id);
    let task_service = service.clone();
    let task_asset = asset.clone();
    let request = tokio::spawn(async move {
        task_service
            .fetch_local_first_history(&task_asset, bounds.start, bounds.end, false)
            .await
    });
    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        provider.request_started.notified(),
    )
    .await
    .unwrap();
    request.abort();
    assert!(
        tokio::time::timeout(std::time::Duration::from_secs(5), request)
            .await
            .unwrap()
            .unwrap_err()
            .is_cancelled()
    );
    assert!(f.store.get_historical_quotes("asset").unwrap().is_empty());
    #[derive(diesel::QueryableByName)]
    struct ReceiptCount {
        #[diesel(sql_type = diesel::sql_types::BigInt)]
        count: i64,
    }
    assert_eq!(
        diesel::sql_query("SELECT COUNT(*) AS count FROM market_history_queries")
            .get_result::<ReceiptCount>(&mut *db::get_connection(&f.pool).unwrap())
            .unwrap()
            .count,
        0
    );
    provider
        .response_mode
        .store(0, std::sync::atomic::Ordering::SeqCst);
    let recovered = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        service.fetch_local_first_history(&asset, bounds.start, bounds.end, false),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(!recovered.is_empty());
    assert_eq!(provider.ranges.lock().unwrap().len(), 2);
}

// The deliberately synchronous actor gate needs another runtime worker to release it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancelling_queued_commit_waiter_preserves_atomic_price_and_receipt_write() {
    let f = fixture().await;
    let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let writer = f.writer.clone();
    let blocker = tokio::spawn(async move {
        writer
            .exec(move |_| {
                entered_tx.send(()).unwrap();
                release_rx
                    .recv_timeout(std::time::Duration::from_secs(5))
                    .unwrap();
                Ok(())
            })
            .await
    });
    tokio::time::timeout(std::time::Duration::from_secs(5), entered_rx)
        .await
        .unwrap()
        .unwrap();
    let record = coverage("cancelled-waiter", "asset");
    let prices = [quote("asset", "YAHOO", 100)];
    {
        let commit =
            f.store
                .commit_history_query(&record, &prices, HistoryWriteMode::Canonical, true, None);
        tokio::pin!(commit);
        // Poll once to enqueue behind the blocked actor. Dropping the waiter
        // cannot retract an already admitted actor job; both writes must commit.
        tokio::select! {
            biased;
            _ = &mut commit => panic!("blocked writer unexpectedly completed"),
            _ = tokio::task::yield_now() => {}
        }
    }
    release_tx.send(()).unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), blocker)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), f.writer.exec(|_| Ok(())))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(f.store.get_historical_quotes("asset").unwrap(), prices);
    assert_eq!(
        f.store.history_coverage("cancelled-waiter").unwrap(),
        vec![record]
    );
}

#[tokio::test]
async fn canonical_context_captures_current_identity_not_stale_callers_asset() {
    let f = fixture().await;
    create_custom(&f, "a", false, "historical", 1).await;
    create_custom(&f, "b", false, "historical", 2).await;
    let old_profile =
        serde_json::json!({"preferred_provider":"CUSTOM_SCRAPER","custom_provider_code":"a"});
    diesel::sql_query("UPDATE assets SET provider_config=? WHERE id='asset'")
        .bind::<diesel::sql_types::Text, _>(old_profile.to_string())
        .execute(&mut *db::get_connection(&f.pool).unwrap())
        .unwrap();
    let stale_asset = AssetRepository::new(f.pool.clone(), f.writer.clone())
        .get_by_id("asset")
        .unwrap();
    // This mutation precedes context capture, unlike the post-capture guard test.
    let new_profile =
        serde_json::json!({"preferred_provider":"CUSTOM_SCRAPER","custom_provider_code":"b"});
    diesel::sql_query("UPDATE assets SET instrument_symbol='NEW', quote_ccy='EUR', provider_config=? WHERE id='asset'")
        .bind::<diesel::sql_types::Text,_>(new_profile.to_string())
        .execute(&mut *db::get_connection(&f.pool).unwrap()).unwrap();
    let context = f
        .store
        .history_write_context(&stale_asset, HistoryQueryKind::History, true)
        .unwrap()
        .unwrap();
    assert_eq!(context.asset.instrument_symbol.as_deref(), Some("NEW"));
    assert_eq!(context.asset.quote_ccy, "EUR");
    assert_eq!(context.asset.provider_config, Some(new_profile));
    assert_eq!(
        context.custom_revision,
        f.store
            .history_configuration_revision(&context.asset, HistoryQueryKind::History)
            .unwrap()
    );
    assert_ne!(
        context.custom_revision,
        f.store
            .history_configuration_revision(&stale_asset, HistoryQueryKind::History)
            .unwrap()
    );
    assert_eq!(
        context.canonical_context.unwrap().asset.instrument_symbol,
        context.asset.instrument_symbol
    );
    // Synthetic/derived assets deliberately retain their requested identity.
    let derived = f
        .store
        .history_write_context(&stale_asset, HistoryQueryKind::History, false)
        .unwrap()
        .unwrap();
    assert_eq!(
        derived.asset.instrument_symbol,
        stale_asset.instrument_symbol
    );
    assert!(derived.canonical_context.is_none());
}

// Exact pre-policy-bump serialization, deliberately confined to this upgrade test.
fn legacy_custom_revision(f: &Fixture, code: &str, kind: &str) -> String {
    use sha2::{Digest, Sha256};
    #[derive(diesel::QueryableByName)]
    struct Revision {
        #[diesel(sql_type = diesel::sql_types::Text)]
        updated_at: String,
        #[diesel(sql_type = diesel::sql_types::Bool)]
        enabled: bool,
        #[diesel(sql_type = diesel::sql_types::Integer)]
        priority: i32,
    }
    let row = diesel::sql_query(
        "SELECT updated_at,enabled,priority FROM market_data_custom_providers WHERE code=?",
    )
    .bind::<diesel::sql_types::Text, _>(code)
    .get_result::<Revision>(&mut *db::get_connection(&f.pool).unwrap())
    .unwrap();
    hex::encode(Sha256::digest(
        serde_json::to_vec(&vec![(
            code,
            kind,
            row.updated_at,
            row.enabled,
            row.priority,
        )])
        .unwrap(),
    ))
}

#[tokio::test]
async fn custom_history_policy_upgrade_retries_legacy_complete_then_bounds_partial_reuse() {
    use sha2::{Digest, Sha256};
    use wealthfolio_core::quotes::{MarketDataClient, ProviderConfig};
    let f = fixture().await;
    create_custom(&f, "custom-history", true, "historical", 1).await;
    let asset = AssetRepository::new(f.pool.clone(), f.writer.clone())
        .get_by_id("asset")
        .unwrap();
    let legacy_revision = legacy_custom_revision(&f, "custom-history", "historical");
    assert_ne!(
        legacy_revision,
        f.store
            .history_configuration_revision(&asset, HistoryQueryKind::History)
            .unwrap()
    );
    let provider = Arc::new(FakeProvider::default());
    provider
        .response_mode
        .store(6, std::sync::atomic::Ordering::SeqCst);
    let client = MarketDataClient::new_with_extra(
        Arc::new(NoSecrets),
        vec![ProviderConfig {
            id: "YAHOO".into(),
            priority: 1,
        }],
        vec![provider.clone()],
    )
    .await
    .unwrap();
    // Exact old owner identity: no public API is added solely for migration tests.
    let old_identity = serde_json::json!({
        "operation":"history", "benchmark":false, "asset":asset.id, "kind":asset.kind,
        "mode":asset.quote_mode,"currency":asset.quote_ccy,"type":asset.instrument_type,
        "symbol":asset.instrument_symbol,"exchange":asset.instrument_exchange_mic,
        "preferredProvider":asset.preferred_provider(),"overrides":asset.provider_overrides(),
        "customProviderCode":asset.provider_config.as_ref().and_then(|config|config.get("custom_provider_code")),
        "isin":asset.metadata.as_ref().and_then(|metadata|metadata.get("identifiers")).and_then(|identifiers|identifiers.get("isin")),
        "bond":asset.bond_spec(),"providers":client.history_configuration_fingerprint(),"configurationRevision":legacy_revision,
    });
    let legacy_identity = format!("{:x}", Sha256::digest(old_identity.to_string().as_bytes()));
    let mut legacy = coverage(&legacy_identity, &asset.id);
    legacy.end = NaiveDate::from_ymd_opt(2025, 12, 31).unwrap();
    let mut last_price = quote(&asset.id, "YAHOO", 100);
    last_price.timestamp = legacy.end.and_hms_opt(12, 0, 0).unwrap().and_utc();
    let manual = quote(&asset.id, "MANUAL", 999);
    f.store
        .upsert_quotes(std::slice::from_ref(&manual))
        .await
        .unwrap();
    f.store
        .commit_history_query(
            &legacy,
            std::slice::from_ref(&last_price),
            HistoryWriteMode::Canonical,
            true,
            None,
        )
        .await
        .unwrap();
    let owner = sync_service_with_client(&f, client);
    owner
        .fetch_local_first_history(&asset, legacy.start, legacy.end, false)
        .await
        .unwrap();
    assert_eq!(
        *provider.ranges.lock().unwrap(),
        vec![(legacy.start, legacy.end)]
    );
    #[derive(diesel::QueryableByName)]
    struct Receipt {
        #[diesel(sql_type = diesel::sql_types::Text)]
        identity: String,
        #[diesel(sql_type = diesel::sql_types::Text)]
        outcome: String,
    }
    let current = diesel::sql_query(
        "SELECT identity,outcome FROM market_history_queries WHERE identity != ?",
    )
    .bind::<diesel::sql_types::Text, _>(&legacy_identity)
    .get_result::<Receipt>(&mut *db::get_connection(&f.pool).unwrap())
    .unwrap();
    assert_eq!(current.outcome, "partial");
    owner
        .fetch_local_first_history(&asset, legacy.start, legacy.end, false)
        .await
        .unwrap();
    assert_eq!(provider.ranges.lock().unwrap().len(), 1);
    diesel::sql_query("UPDATE market_history_queries SET fetched_at=? WHERE identity=?")
        .bind::<diesel::sql_types::Text, _>((Utc::now() - Duration::days(2)).to_rfc3339())
        .bind::<diesel::sql_types::Text, _>(&current.identity)
        .execute(&mut *db::get_connection(&f.pool).unwrap())
        .unwrap();
    owner
        .fetch_local_first_history(&asset, legacy.start, legacy.end, false)
        .await
        .unwrap();
    assert_eq!(
        *provider.ranges.lock().unwrap(),
        vec![(legacy.start, legacy.end), (legacy.start, legacy.end)]
    );
    let prices = f.store.get_historical_quotes(&asset.id).unwrap();
    assert!(prices
        .iter()
        .any(|q| q.data_source == "MANUAL" && q.close == manual.close));
    assert!(prices
        .iter()
        .any(|q| q.timestamp == last_price.timestamp && q.close == last_price.close));
    assert_eq!(
        f.store.history_coverage(&legacy_identity).unwrap(),
        vec![legacy]
    );
}

#[tokio::test]
async fn custom_history_policy_salt_leaves_latest_and_unrelated_revisions_unchanged() {
    let f = fixture().await;
    let asset = AssetRepository::new(f.pool.clone(), f.writer.clone())
        .get_by_id("asset")
        .unwrap();
    assert_eq!(
        f.store
            .history_configuration_revision(&asset, HistoryQueryKind::History)
            .unwrap(),
        ""
    );
    create_custom(&f, "private", false, "historical", 1).await;
    assert_eq!(
        f.store
            .history_configuration_revision(&asset, HistoryQueryKind::History)
            .unwrap(),
        ""
    );
    create_custom(&f, "latest", true, "latest", 2).await;
    let legacy_latest = legacy_custom_revision(&f, "latest", "latest");
    assert_eq!(
        f.store
            .history_configuration_revision(&asset, HistoryQueryKind::Latest)
            .unwrap(),
        legacy_latest
    );
    // History falling back to that same latest source also needs the policy bump.
    assert_ne!(
        f.store
            .history_configuration_revision(&asset, HistoryQueryKind::History)
            .unwrap(),
        legacy_latest
    );
}
