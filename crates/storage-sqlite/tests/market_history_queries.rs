//! Boundary supplementation and guarded price persistence remain profile-local.
#![allow(clippy::unwrap_used, clippy::panic, reason = "test code")]
use std::sync::Arc;

use chrono::{Duration, NaiveDate, Utc};
use diesel::prelude::*;
use rust_decimal::Decimal;
use wealthfolio_core::assets::{
    AssetKind, AssetRepositoryTrait, InstrumentType, NewAsset, QuoteMode,
};
use wealthfolio_core::quotes::history::{HistoryQueryKind, HistoryWrite, HistoryWriteMode};
use wealthfolio_core::quotes::types::Day;
use wealthfolio_core::quotes::{Quote, QuoteStore};
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

fn write_range(identity: &str, asset: &str) -> HistoryWrite {
    HistoryWrite {
        identity: identity.into(),
        asset_id: asset.into(),
        start: NaiveDate::from_ymd_opt(2025, 1, 1).unwrap(),
        end: NaiveDate::from_ymd_opt(2025, 1, 31).unwrap(),
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
    async fn get_latest_quote(
        &self,
        _: &wealthfolio_market_data::QuoteContext,
        _: wealthfolio_market_data::ProviderInstrument,
    ) -> Result<wealthfolio_market_data::Quote, wealthfolio_market_data::errors::MarketDataError>
    {
        self.latest_calls
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        // Match the application's daily-quote convention: UTC noon labels the
        // exchange-local date, even before noon has elapsed in real time.
        let today = Utc::now()
            .with_timezone(
                &wealthfolio_core::utils::time_utils::parse_user_timezone("America/New_York")
                    .unwrap(),
            )
            .date_naive();
        let day = if self.response_mode.load(std::sync::atomic::Ordering::SeqCst) == 4 {
            today + Duration::days(2)
        } else {
            today
        };
        let timestamp = day.and_hms_opt(12, 0, 0).unwrap().and_utc();
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
async fn benchmark_history_does_not_create_assets_or_pollute_quote_lists() {
    let f = fixture().await;
    let record = write_range("benchmark-v1", "SEC:BENCH:INDEX");
    f.store
        .commit_history_prices(
            &record,
            &[quote("SEC:BENCH:INDEX", "YAHOO", 123)],
            HistoryWriteMode::DerivedRange,
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
async fn local_hit_and_restarted_service_make_zero_additional_provider_requests() {
    let f = fixture().await;
    let provider = Arc::new(FakeProvider::default());
    let service = sync_service(&f, provider.clone()).await;
    let asset = AssetRepository::new(f.pool.clone(), f.writer.clone())
        .get_by_id("asset")
        .unwrap();
    let c = write_range("unused", "asset");
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
    let c = write_range("unused", "asset");
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
async fn concurrent_different_ranges_preserve_both_callers_work() {
    let f = fixture().await;
    let provider = Arc::new(FakeProvider::default());
    let service = sync_service(&f, provider.clone()).await;
    let asset = AssetRepository::new(f.pool.clone(), f.writer.clone())
        .get_by_id("asset")
        .unwrap();
    let start = write_range("unused", "asset").start;
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
async fn unknown_benchmark_reuses_persisted_history_without_catalog_insertion() {
    let f = fixture().await;
    let provider = Arc::new(FakeProvider::default());
    let service = sync_service(&f, provider.clone()).await;
    let mut asset = AssetRepository::new(f.pool.clone(), f.writer.clone())
        .get_by_id("asset")
        .unwrap();
    asset.id = "SEC:BENCH:INDEX".into();
    asset.instrument_symbol = Some("BENCH".into());
    let c = write_range("unused", &asset.id);
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
async fn identical_refresh_does_not_count_price_changes() {
    let f = fixture().await;
    let record = write_range("identity", "asset");
    let original = quote("asset", "YAHOO", 100);
    assert_eq!(
        f.store
            .commit_history_prices(
                &record,
                std::slice::from_ref(&original),
                HistoryWriteMode::Canonical,
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
            .commit_history_prices(
                &record,
                std::slice::from_ref(&repeated),
                HistoryWriteMode::Canonical,
                None
            )
            .await
            .unwrap(),
        0
    );
    repeated.close = Decimal::new(101, 0);
    assert_eq!(
        f.store
            .commit_history_prices(&record, &[repeated], HistoryWriteMode::Canonical, None)
            .await
            .unwrap(),
        1
    );
    let benchmark = write_range("benchmark", "unknown");
    let first = quote("unknown", "YAHOO", 100);
    assert_eq!(
        f.store
            .commit_history_prices(
                &benchmark,
                std::slice::from_ref(&first),
                HistoryWriteMode::DerivedRange,
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
            .commit_history_prices(
                &benchmark,
                &[duplicate],
                HistoryWriteMode::DerivedRange,
                None
            )
            .await
            .unwrap(),
        0
    );
}

#[tokio::test]
async fn covered_history_resolves_latest_without_another_provider_request() {
    let f = fixture().await;
    let provider = Arc::new(FakeProvider::default());
    let service = sync_service(&f, provider.clone()).await;
    let asset = AssetRepository::new(f.pool.clone(), f.writer.clone())
        .get_by_id("asset")
        .unwrap();
    let today = Utc::now()
        .with_timezone(
            &wealthfolio_core::utils::time_utils::parse_user_timezone("America/New_York").unwrap(),
        )
        .date_naive();
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
async fn future_cached_quote_cannot_hide_today_or_suppress_a_provider_request() {
    for has_today in [false, true] {
        let f = fixture().await;
        let provider = Arc::new(FakeProvider::default());
        let service = sync_service(&f, provider.clone()).await;
        let asset = AssetRepository::new(f.pool.clone(), f.writer.clone())
            .get_by_id("asset")
            .unwrap();
        let today = Utc::now()
            .with_timezone(
                &wealthfolio_core::utils::time_utils::parse_user_timezone("America/New_York")
                    .unwrap(),
            )
            .date_naive();
        let mut future = quote("asset", "YAHOO", 999);
        future.timestamp = (today + Duration::days(2))
            .and_hms_opt(12, 0, 0)
            .unwrap()
            .and_utc();
        f.store.upsert_quotes(&[future]).await.unwrap();
        if has_today {
            let mut current = quote("asset", "YAHOO", 123);
            current.timestamp = today.and_hms_opt(12, 0, 0).unwrap().and_utc();
            f.store.upsert_quotes(&[current]).await.unwrap();
        }
        let latest = service
            .fetch_local_first_latest(&asset, false)
            .await
            .unwrap();
        assert_eq!(latest.timestamp.date_naive(), today);
        assert_eq!(
            latest.close,
            Decimal::new(if has_today { 123 } else { 100 }, 0)
        );
        assert_eq!(
            provider
                .latest_calls
                .load(std::sync::atomic::Ordering::SeqCst),
            usize::from(!has_today)
        );
        let history = service
            .fetch_local_first_history(&asset, today, today + Duration::days(2), false)
            .await
            .unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].timestamp.date_naive(), today);
        assert!(provider.ranges.lock().unwrap().is_empty());
    }
}

#[tokio::test]
async fn future_provider_quote_is_rejected_without_persisting_a_cache_hit() {
    for benchmark in [false, true] {
        let f = fixture().await;
        let provider = Arc::new(FakeProvider::default());
        provider
            .response_mode
            .store(4, std::sync::atomic::Ordering::SeqCst);
        let service = sync_service(&f, provider.clone()).await;
        let asset = AssetRepository::new(f.pool.clone(), f.writer.clone())
            .get_by_id("asset")
            .unwrap();
        for _ in 0..2 {
            let error = service
                .fetch_local_first_latest(&asset, benchmark)
                .await
                .unwrap_err();
            assert!(error.to_string().contains("current market date"));
        }
        assert_eq!(
            provider
                .latest_calls
                .load(std::sync::atomic::Ordering::SeqCst),
            2
        );
        assert!(f
            .store
            .range_compatible(&asset, Day::new(NaiveDate::MIN), Day::new(NaiveDate::MAX))
            .unwrap()
            .is_empty());
        #[derive(diesel::QueryableByName)]
        struct Count {
            #[diesel(sql_type = diesel::sql_types::BigInt)]
            count: i64,
        }
        let mut conn = f.pool.get().unwrap();
        let count = diesel::sql_query("SELECT COUNT(*) AS count FROM benchmark_history")
            .get_result::<Count>(&mut *conn)
            .unwrap();
        assert_eq!(count.count, 0);
    }
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancelling_queued_commit_waiter_preserves_admitted_price_write() {
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
    let record = write_range("cancelled-waiter", "asset");
    let prices = [quote("asset", "YAHOO", 100)];
    {
        let commit =
            f.store
                .commit_history_prices(&record, &prices, HistoryWriteMode::Canonical, None);
        tokio::pin!(commit);
        // Poll once to enqueue behind the blocked actor. Dropping the waiter
        // cannot retract an already admitted actor job; the admitted price write must commit.
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
}

#[tokio::test]
async fn local_boundaries_fetch_only_exact_prefix_and_suffix_not_internal_gaps() {
    let f = fixture().await;
    let provider = Arc::new(FakeProvider::default());
    let service = sync_service(&f, provider.clone()).await;
    let asset = AssetRepository::new(f.pool.clone(), f.writer.clone())
        .get_by_id("asset")
        .unwrap();
    let range = write_range("unused", "asset");
    let mut first = quote("asset", "YAHOO", 10);
    first.timestamp = (range.start + Duration::days(10))
        .and_hms_opt(12, 0, 0)
        .unwrap()
        .and_utc();
    first.id = "boundary-first".into();
    let mut last = first.clone();
    last.id = "boundary-last".into();
    last.timestamp = (range.end - Duration::days(10))
        .and_hms_opt(12, 0, 0)
        .unwrap()
        .and_utc();
    f.store
        .upsert_quotes(&[first.clone(), last.clone()])
        .await
        .unwrap();
    let result = service
        .fetch_local_first_history(&asset, range.start, range.end, false)
        .await
        .unwrap();
    assert_eq!(
        *provider.ranges.lock().unwrap(),
        vec![
            (
                range.start,
                first.timestamp.date_naive().pred_opt().unwrap()
            ),
            (last.timestamp.date_naive().succ_opt().unwrap(), range.end),
        ]
    );
    assert_eq!(result.len(), 22); // Internal days 12–20 remain absent.
    service
        .fetch_local_first_history(&asset, range.start, range.end, false)
        .await
        .unwrap();
    assert_eq!(provider.ranges.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn empty_and_failed_ranges_are_requested_again_without_receipts() {
    for mode in [1, 3] {
        let f = fixture().await;
        let provider = Arc::new(FakeProvider::default());
        provider
            .response_mode
            .store(mode, std::sync::atomic::Ordering::SeqCst);
        let service = sync_service(&f, provider.clone()).await;
        let asset = AssetRepository::new(f.pool.clone(), f.writer.clone())
            .get_by_id("asset")
            .unwrap();
        let range = write_range("unused", "asset");
        let first = service
            .fetch_local_first_history(&asset, range.start, range.end, false)
            .await;
        let second = service
            .fetch_local_first_history(&asset, range.start, range.end, false)
            .await;
        assert_eq!(first.is_err(), mode == 3);
        assert_eq!(second.is_err(), mode == 3);
        assert_eq!(provider.ranges.lock().unwrap().len(), 2);
    }
}

#[tokio::test]
async fn explicit_sync_always_requests_even_when_local_boundaries_cover_range() {
    use wealthfolio_core::quotes::{QuoteSyncServiceTrait, SyncMode};
    let f = fixture().await;
    let provider = Arc::new(FakeProvider::default());
    let service = sync_service(&f, provider.clone()).await;
    for mode in [
        SyncMode::BackfillHistory { days: 30 },
        SyncMode::BackfillHistory { days: 30 },
        SyncMode::RefreshAllHistory { days: 30 },
    ] {
        service
            .sync(mode, Some(vec!["asset".into()]))
            .await
            .unwrap();
    }
    assert_eq!(provider.ranges.lock().unwrap().len(), 3);
}

#[tokio::test]
async fn currency_filter_precedes_daily_source_priority_and_manual_still_wins() {
    use wealthfolio_core::quotes::Day;
    let f = fixture().await;
    let asset = AssetRepository::new(f.pool.clone(), f.writer.clone())
        .get_by_id("asset")
        .unwrap();
    let mut wrong = quote("asset", "YAHOO", 10);
    wrong.currency = "EUR".into();
    let compatible = quote("asset", "HTTP_API", 20);
    f.store
        .upsert_quotes(&[wrong, compatible.clone()])
        .await
        .unwrap();
    let day = Day::new(compatible.timestamp.date_naive());
    assert_eq!(
        f.store.range_compatible(&asset, day, day).unwrap(),
        vec![compatible]
    );
    let manual = quote("asset", "MANUAL", 30);
    f.store
        .upsert_quotes(std::slice::from_ref(&manual))
        .await
        .unwrap();
    assert_eq!(
        f.store.range_compatible(&asset, day, day).unwrap(),
        vec![manual]
    );
}

#[tokio::test]
async fn configuration_change_rejects_stale_write_without_deleting_prices() {
    let f = fixture().await;
    let asset = AssetRepository::new(f.pool.clone(), f.writer.clone())
        .get_by_id("asset")
        .unwrap();
    let original = quote("asset", "YAHOO", 99);
    f.store
        .upsert_quotes(std::slice::from_ref(&original))
        .await
        .unwrap();
    let expected = f
        .store
        .history_write_context(&asset, HistoryQueryKind::History, true)
        .unwrap()
        .unwrap();
    diesel::sql_query("UPDATE assets SET quote_ccy='EUR' WHERE id='asset'")
        .execute(&mut *db::get_connection(&f.pool).unwrap())
        .unwrap();
    assert!(f
        .store
        .commit_history_prices(
            &write_range("identity", "asset"),
            &[quote("asset", "YAHOO", 100)],
            HistoryWriteMode::Canonical,
            Some(&expected)
        )
        .await
        .is_err());
    assert_eq!(
        f.store.get_historical_quotes("asset").unwrap(),
        vec![original]
    );
}

#[tokio::test]
async fn benchmark_empty_response_merges_nothing_and_preserves_existing_prices() {
    let f = fixture().await;
    let range = write_range("benchmark", "unknown");
    let old = quote("unknown", "YAHOO", 100);
    f.store
        .commit_history_prices(
            &range,
            std::slice::from_ref(&old),
            HistoryWriteMode::DerivedRange,
            None,
        )
        .await
        .unwrap();
    assert_eq!(
        f.store
            .commit_history_prices(&range, &[], HistoryWriteMode::DerivedRange, None)
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        f.store
            .benchmark_history("benchmark", range.start, range.end)
            .unwrap(),
        vec![old]
    );
}

#[tokio::test]
async fn invalid_price_batch_rolls_back_all_admitted_rows() {
    let f = fixture().await;
    diesel::sql_query("CREATE TRIGGER reject_test_price BEFORE INSERT ON quotes WHEN NEW.close='13' BEGIN SELECT RAISE(ABORT, 'test rejection'); END")
        .execute(&mut *db::get_connection(&f.pool).unwrap()).unwrap();
    let a = quote("asset", "YAHOO", 100);
    let mut b = quote("asset", "YAHOO", 13);
    b.timestamp += Duration::days(1);
    b.id = "invalid-second-day".into();
    assert!(f
        .store
        .commit_history_prices(
            &write_range("identity", "asset"),
            &[a, b],
            HistoryWriteMode::Canonical,
            None
        )
        .await
        .is_err());
    assert!(f.store.get_historical_quotes("asset").unwrap().is_empty());
}

#[tokio::test]
async fn cleanup_migration_removes_proofs_but_preserves_prices_and_benchmarks() {
    use diesel::connection::SimpleConnection;
    let f = fixture().await;
    f.store
        .upsert_quotes(&[quote("asset", "MANUAL", 99)])
        .await
        .unwrap();
    let range = write_range("legacy-benchmark-identity", "unknown");
    f.store
        .commit_history_prices(
            &range,
            &[quote("unknown", "YAHOO", 100)],
            HistoryWriteMode::DerivedRange,
            None,
        )
        .await
        .unwrap();
    let mut conn = db::get_connection(&f.pool).unwrap();
    conn.batch_execute(include_str!(
        "../migrations/2026-10-25-000002_remove_history_coverage/down.sql"
    ))
    .unwrap();
    conn.batch_execute("INSERT INTO market_history_queries VALUES ('legacy-benchmark-identity','unknown','2025-01-01','2025-01-31','2025-02-01T00:00:00Z','complete')").unwrap();
    conn.batch_execute(include_str!(
        "../migrations/2026-10-25-000002_remove_history_coverage/up.sql"
    ))
    .unwrap();
    #[derive(QueryableByName)]
    struct Count {
        #[diesel(sql_type = diesel::sql_types::BigInt)]
        count: i64,
    }
    let obsolete = diesel::sql_query("SELECT COUNT(*) AS count FROM sqlite_master WHERE name IN ('market_history_queries','market_history_queries_asset','invalidate_asset_history_queries')").get_result::<Count>(&mut *conn).unwrap();
    assert_eq!(obsolete.count, 0);
    drop(conn);
    assert_eq!(f.store.get_historical_quotes("asset").unwrap().len(), 1);
    assert_eq!(
        f.store
            .benchmark_history(&range.identity, range.start, range.end)
            .unwrap()
            .len(),
        1
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
    let bounds = write_range("unused", &asset.id);
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
#[tokio::test]
async fn transient_custom_snapshot_guards_changes_without_invalidating_other_sources() {
    let f = fixture().await;
    create_custom(&f, "a", false, "historical", 1).await;
    create_custom(&f, "b", false, "historical", 2).await;
    let mut asset = assigned_asset(&f, "a");
    asset.id = "benchmark".into();
    let context = f
        .store
        .history_write_context(&asset, HistoryQueryKind::History, false)
        .unwrap()
        .unwrap();
    assert!(!context.custom_providers.is_empty());
    rename_custom(&f, "b").await;
    let after_unrelated = f
        .store
        .history_write_context(&asset, HistoryQueryKind::History, false)
        .unwrap()
        .unwrap();
    assert_eq!(context.custom_providers, after_unrelated.custom_providers);
    let range = write_range("benchmark", "benchmark");
    f.store
        .commit_history_prices(
            &range,
            &[quote("benchmark", "YAHOO", 100)],
            HistoryWriteMode::DerivedRange,
            Some(&context),
        )
        .await
        .unwrap();
    // Force a distinct saved revision even on fast same-millisecond clocks.
    diesel::sql_query(
        "UPDATE market_data_custom_providers SET updated_at='2099-01-01T00:00:00Z' WHERE code='a'",
    )
    .execute(&mut *db::get_connection(&f.pool).unwrap())
    .unwrap();
    assert!(f
        .store
        .commit_history_prices(
            &range,
            &[quote("benchmark", "YAHOO", 200)],
            HistoryWriteMode::DerivedRange,
            Some(&context)
        )
        .await
        .is_err());
    assert_eq!(
        f.store
            .benchmark_history(&range.identity, range.start, range.end)
            .unwrap()[0]
            .close,
        Decimal::new(100, 0)
    );
}
#[tokio::test]
async fn manual_asset_reads_only_local_prices_without_fetching_boundaries() {
    let f = fixture().await;
    diesel::sql_query("UPDATE assets SET quote_mode='MANUAL' WHERE id='asset'")
        .execute(&mut *db::get_connection(&f.pool).unwrap())
        .unwrap();
    let provider = Arc::new(FakeProvider::default());
    let service = sync_service(&f, provider.clone()).await;
    let asset = AssetRepository::new(f.pool.clone(), f.writer.clone())
        .get_by_id("asset")
        .unwrap();
    let manual = quote("asset", "MANUAL", 99);
    f.store
        .upsert_quotes(std::slice::from_ref(&manual))
        .await
        .unwrap();
    let range = write_range("unused", "asset");
    assert_eq!(
        service
            .fetch_local_first_history(&asset, range.start, range.end, false)
            .await
            .unwrap(),
        vec![manual]
    );
    assert!(provider.ranges.lock().unwrap().is_empty());
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
    let stale_snapshot = f
        .store
        .history_write_context(&stale_asset, HistoryQueryKind::History, false)
        .unwrap()
        .unwrap();
    assert_ne!(context.custom_providers, stale_snapshot.custom_providers);
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
#[tokio::test]
async fn benchmark_fetch_failure_preserves_previously_saved_prices() {
    let f = fixture().await;
    let provider = Arc::new(FakeProvider::default());
    let service = sync_service(&f, provider.clone()).await;
    let mut asset = AssetRepository::new(f.pool.clone(), f.writer.clone())
        .get_by_id("asset")
        .unwrap();
    asset.id = "benchmark-failure".into();
    let range = write_range("unused", &asset.id);
    let prices = service
        .fetch_local_first_history(&asset, range.start, range.end, true)
        .await
        .unwrap();
    provider
        .response_mode
        .store(3, std::sync::atomic::Ordering::SeqCst);
    assert!(service
        .fetch_local_first_history(&asset, range.start - Duration::days(1), range.end, true)
        .await
        .is_err());
    assert_eq!(
        service
            .fetch_local_first_history(&asset, range.start, range.end, true)
            .await
            .unwrap(),
        prices
    );
    assert_eq!(provider.ranges.lock().unwrap().len(), 2);
}
