use super::*;
use crate::settings::Settings;

fn instant(value: &str) -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::parse_from_rfc3339(value)
        .unwrap()
        .with_timezone(&chrono::Utc)
}

#[test]
fn daily_sync_window_and_persisted_attempt() {
    let settings = Settings::default();
    assert!(!daily_sync_due(&settings, None, instant("2026-10-25T21:59:00+08:00")).unwrap());
    assert!(daily_sync_due(&settings, None, instant("2026-10-25T22:00:00+08:00")).unwrap());
    assert!(daily_sync_due(&settings, None, instant("2026-10-25T22:30:00+08:00")).unwrap());
    let attempted = Some("2026-10-25T22:00:00+08:00");
    assert!(!daily_sync_due(&settings, attempted, instant("2026-10-25T23:59:00+08:00")).unwrap());
    assert!(!daily_sync_due(&settings, attempted, instant("2026-10-26T10:00:00+08:00")).unwrap());
    assert!(daily_sync_due(&settings, attempted, instant("2026-10-26T22:00:00+08:00")).unwrap());
}

#[test]
fn daily_sync_rejects_invalid_state_and_future_attempts() {
    let mut settings = Settings::default();
    let now = instant("2026-10-25T23:00:00+08:00");
    assert!(daily_sync_due(&settings, Some("invalid"), now).is_err());
    assert!(!daily_sync_due(&settings, Some("2026-10-26T22:00:00+08:00"), now).unwrap());
    settings.market_data_sync_time = "25:00".into();
    assert!(daily_sync_due(&settings, None, now).is_err());
    settings.market_data_sync_time = "22:00".into();
    settings.market_data_sync_timezone = "invalid".into();
    assert!(daily_sync_due(&settings, None, now).is_err());
}

#[test]
fn daily_sync_dst_gap_repeat_and_timezone_change() {
    let mut settings = Settings {
        market_data_sync_timezone: "America/New_York".into(),
        market_data_sync_time: "02:30".into(),
        ..Settings::default()
    };
    assert!(!daily_sync_due(&settings, None, instant("2026-03-08T01:59:00-05:00")).unwrap());
    assert!(daily_sync_due(&settings, None, instant("2026-03-08T03:00:00-04:00")).unwrap());
    settings.market_data_sync_time = "01:30".into();
    assert!(!daily_sync_due(
        &settings,
        Some("2026-11-01T01:30:00-04:00"),
        instant("2026-11-01T01:30:00-05:00")
    )
    .unwrap());
    settings.market_data_sync_time = "00:00".into();
    assert!(!daily_sync_due(
        &settings,
        Some("2026-10-25T22:00:00+08:00"),
        instant("2026-10-25T23:00:00+08:00")
    )
    .unwrap());
}

#[derive(Default)]
struct TestSettings {
    marker: std::sync::Mutex<Option<String>>,
    benchmarks: std::sync::Mutex<Option<String>>,
    fail_write: std::sync::atomic::AtomicBool,
}
#[async_trait]
impl SettingsServiceTrait for TestSettings {
    fn get_settings(&self) -> Result<Settings> {
        Ok(Settings {
            market_data_sync_time: "00:00".into(),
            ..Settings::default()
        })
    }
    async fn update_settings(&self, _: &crate::settings::SettingsUpdate) -> Result<()> {
        unimplemented!()
    }
    fn get_base_currency(&self) -> Result<Option<String>> {
        unimplemented!()
    }
    fn is_auto_update_check_enabled(&self) -> Result<bool> {
        unimplemented!()
    }
    fn is_sync_enabled(&self) -> Result<bool> {
        unimplemented!()
    }
    fn get_setting_value(&self, key: &str) -> Result<Option<String>> {
        Ok(
            if key == crate::settings::MARKET_DATA_AUTO_SYNC_LAST_ATTEMPT_KEY {
                self.marker.lock().unwrap().clone()
            } else if key == crate::settings::MARKET_DATA_BENCHMARK_IDS_KEY {
                self.benchmarks.lock().unwrap().clone()
            } else {
                None
            },
        )
    }
    async fn set_setting_value(&self, key: &str, value: &str) -> Result<()> {
        if self.fail_write.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(Error::Unexpected("Test write failure".into()));
        }
        if key == crate::settings::MARKET_DATA_BENCHMARK_IDS_KEY {
            *self.benchmarks.lock().unwrap() = Some(value.into());
        } else {
            *self.marker.lock().unwrap() = Some(value.into());
        }
        Ok(())
    }
}

#[tokio::test]
async fn daily_sync_concurrent_wakes_restart_and_failed_batch_do_not_repeat() {
    use std::sync::atomic::Ordering;
    let h = harness(scenario("NOM-TRADE-01").facts()).await;
    let assets =
        crate::assets::AssetService::new(h.sources.assets.clone(), h.quote_service.clone())
            .unwrap();
    let settings = TestSettings::default();
    h.quote_service
        .fail_daily_sync
        .store(true, Ordering::SeqCst);
    let (first, second) = tokio::join!(
        h.coordinator
            .update_if_due(&settings, &assets, &SilentObserver),
        h.coordinator
            .update_if_due(&settings, &assets, &SilentObserver),
    );
    assert_eq!(
        usize::from(first.unwrap().is_some()) + usize::from(second.unwrap().is_some()),
        1
    );
    assert_eq!(h.quote_service.daily_sync_calls.load(Ordering::SeqCst), 1);
    let restarted = harness(scenario("NOM-TRADE-01").facts()).await;
    assert!(restarted
        .coordinator
        .update_if_due(&settings, &assets, &SilentObserver)
        .await
        .unwrap()
        .is_none());
    assert_eq!(
        restarted
            .quote_service
            .daily_sync_calls
            .load(Ordering::SeqCst),
        0
    );
}

#[tokio::test]
async fn daily_sync_marker_write_failure_prevents_provider_calls() {
    use std::sync::atomic::Ordering;
    let h = harness(scenario("NOM-TRADE-01").facts()).await;
    let assets =
        crate::assets::AssetService::new(h.sources.assets.clone(), h.quote_service.clone())
            .unwrap();
    let settings = TestSettings::default();
    settings.fail_write.store(true, Ordering::SeqCst);
    assert!(h
        .coordinator
        .update_if_due(&settings, &assets, &SilentObserver)
        .await
        .is_err());
    assert_eq!(h.quote_service.daily_sync_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn benchmark_performance_uses_saved_quotes_without_provider_calls() {
    use crate::portfolio::performance::{PerformanceService, PerformanceServiceTrait};
    let facts = scenario("NOM-TRADE-01").facts();
    let h = harness(facts).await;
    let quotes = h.quote_service.all_quotes();
    let asset_id = quotes[0].asset_id.clone();
    let mut dates: Vec<_> = quotes
        .iter()
        .filter(|quote| quote.asset_id == asset_id)
        .map(|quote| quote.timestamp.date_naive())
        .collect();
    dates.sort();
    let service = PerformanceService::new(
        h.base_currency.clone(),
        h.timezone.clone(),
        h.sources.clone(),
        h.valuation_repo.clone(),
        h.lot_repo.clone(),
    );
    // The fixture quote service panics if the provider-fetch method is called.
    let result = service
        .calculate_performance_history(
            "symbol",
            &asset_id,
            dates.first().copied(),
            dates.last().copied(),
            None,
            None,
        )
        .await
        .unwrap();
    assert!(!result.series.is_empty());
    let missing = service
        .calculate_performance_history(
            "symbol",
            &asset_id,
            Some(dates[0] - chrono::Duration::days(365)),
            dates.last().copied(),
            None,
            None,
        )
        .await
        .unwrap();
    assert_eq!(missing.series, result.series);
    assert!(!missing.data_quality.warnings.is_empty());
    assert_eq!(missing.summary.quality, missing.data_quality.status);
    assert!(missing
        .data_quality
        .warnings
        .iter()
        .all(|warning| missing.summary.reasons.contains(warning)));
    // A holiday or an unfinished trading day must not erase saved returns.
    let trailing_gap = service
        .calculate_performance_history(
            "symbol",
            &asset_id,
            dates.first().copied(),
            Some(*dates.last().unwrap() + chrono::Duration::days(7)),
            None,
            None,
        )
        .await
        .unwrap();
    assert_eq!(trailing_gap.series, result.series);
    assert!(!trailing_gap.data_quality.warnings.is_empty());
    let empty = service
        .calculate_performance_history(
            "symbol",
            &asset_id,
            Some(dates[0] - chrono::Duration::days(365)),
            Some(dates[0] - chrono::Duration::days(30)),
            None,
            None,
        )
        .await
        .unwrap();
    assert!(empty.series.is_empty());
}

#[tokio::test]
async fn benchmark_registration_reuses_asset_and_persists_unique_ids() {
    let h = harness(scenario("NOM-TRADE-01").facts()).await;
    let assets =
        crate::assets::AssetService::new(h.sources.assets.clone(), h.quote_service.clone())
            .unwrap();
    let existing = crate::assets::AssetServiceTrait::get_assets(&assets)
        .unwrap()
        .remove(0);
    let settings = TestSettings::default();
    for _ in 0..2 {
        let registered = h
            .coordinator
            .register_benchmark(
                &settings,
                &assets,
                crate::assets::NewAsset {
                    id: Some(existing.id.clone()),
                    name: Some("Must not replace the existing name".into()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(registered.id, existing.id);
        assert_eq!(registered.name, existing.name);
    }
    let ids: Vec<String> = serde_json::from_str(
        &settings
            .get_setting_value(crate::settings::MARKET_DATA_BENCHMARK_IDS_KEY)
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(ids, vec![existing.id]);
}

mod initial_profile_enrichment {
    use super::*;
    use crate::assets::{
        Asset, AssetMetadata, AssetResolutionInput, AssetResolutionOutput, AssetServiceTrait,
        AssetSpec, EnsureAssetsResult, NewAsset, QuoteMode, UpdateAssetProfile,
    };
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::Mutex;

    /// Records dispatch, not provider persistence. Real enrichment state storage
    /// is exercised by the SQLite tests and the unchanged bond workflow E2E.
    #[derive(Default)]
    struct RecordingAssets {
        calls: Mutex<Vec<Vec<String>>>,
        fail_first: AtomicBool,
        active: AtomicUsize,
        peak: AtomicUsize,
        pause: Option<(Arc<tokio::sync::Notify>, Arc<tokio::sync::Notify>)>,
    }

    #[async_trait]
    impl AssetServiceTrait for RecordingAssets {
        fn get_assets(&self) -> Result<Vec<Asset>> {
            unimplemented!("enrichment must use the coordinator's targeted repository read")
        }
        fn get_asset_by_id(&self, _: &str) -> Result<Asset> {
            unimplemented!()
        }
        async fn delete_asset(&self, _: &str) -> Result<()> {
            unimplemented!()
        }
        async fn update_asset_profile(&self, _: &str, _: UpdateAssetProfile) -> Result<Asset> {
            unimplemented!()
        }
        async fn create_asset(&self, _: NewAsset) -> Result<Asset> {
            unimplemented!()
        }
        async fn get_or_create_minimal_asset(
            &self,
            _: &str,
            _: Option<String>,
            _: Option<AssetMetadata>,
            _: Option<String>,
        ) -> Result<Asset> {
            unimplemented!()
        }
        async fn update_quote_mode(&self, _: &str, _: &str) -> Result<Asset> {
            unimplemented!()
        }
        async fn get_assets_by_asset_ids(&self, _: &[String]) -> Result<Vec<Asset>> {
            unimplemented!()
        }
        async fn enrich_asset_profile(&self, _: &str) -> Result<Asset> {
            unimplemented!()
        }
        async fn enrich_assets(&self, ids: Vec<String>) -> Result<(usize, usize, usize)> {
            let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
            self.peak.fetch_max(active, Ordering::SeqCst);
            self.calls.lock().unwrap().push(ids.clone());
            if let Some((entered, release)) = &self.pause {
                entered.notify_one();
                release.notified().await;
            } else {
                // Without the coordinator lock, another creation batch can
                // enter while this one is awaiting its provider response.
                tokio::task::yield_now().await;
            }
            self.active.fetch_sub(1, Ordering::SeqCst);
            if self.fail_first.swap(false, Ordering::SeqCst) {
                Err(Error::Unexpected("isolated profile batch failure".into()))
            } else {
                Ok((ids.len(), 0, 0))
            }
        }
        async fn cleanup_legacy_metadata(&self, _: &str) -> Result<()> {
            unimplemented!()
        }
        async fn merge_unknown_asset(
            &self,
            _: &str,
            _: &str,
            _: &dyn crate::activities::ActivityRepositoryTrait,
        ) -> Result<u32> {
            unimplemented!()
        }
        async fn ensure_assets(
            &self,
            _: Vec<AssetSpec>,
            _: &dyn crate::activities::ActivityRepositoryTrait,
        ) -> Result<EnsureAssetsResult> {
            unimplemented!()
        }
        async fn resolve_import_asset_inputs(
            &self,
            _: Vec<AssetResolutionInput>,
        ) -> Result<Vec<AssetResolutionOutput>> {
            unimplemented!()
        }
    }

    #[derive(Debug, PartialEq, Eq)]
    enum Progress {
        Started(usize),
        Advanced(usize, usize),
        Completed(usize, usize, usize),
    }

    #[derive(Default)]
    struct RecordingObserver(Mutex<Vec<Progress>>);
    impl JobObserver for RecordingObserver {
        fn enrichment_started(&self, total: usize) {
            self.0.lock().unwrap().push(Progress::Started(total));
        }
        fn enrichment_progress(&self, completed: usize, total: usize) {
            self.0
                .lock()
                .unwrap()
                .push(Progress::Advanced(completed, total));
        }
        fn enrichment_completed(&self, enriched: usize, skipped: usize, failed: usize) {
            self.0
                .lock()
                .unwrap()
                .push(Progress::Completed(enriched, skipped, failed));
        }
    }

    fn facts_with_new_assets(count: usize) -> crate::test_support::scenario::ScenarioFacts {
        let mut facts = scenario("NOM-TRADE-01").facts();
        let template = facts.assets[0].clone();
        for index in 0..count {
            facts.assets.push(Asset {
                id: format!("new-{index}"),
                is_active: true,
                quote_mode: QuoteMode::Market,
                ..template.clone()
            });
        }
        facts
    }

    #[tokio::test]
    async fn creation_enrichment_is_targeted_deduplicated_and_does_not_fetch_quotes() {
        let mut facts = facts_with_new_assets(4);
        facts
            .assets
            .iter_mut()
            .find(|a| a.id == "new-2")
            .unwrap()
            .quote_mode = QuoteMode::Manual;
        facts
            .assets
            .iter_mut()
            .find(|a| a.id == "new-3")
            .unwrap()
            .is_active = false;
        let h = harness(facts).await;
        let assets = RecordingAssets::default();
        let observer = RecordingObserver::default();
        h.coordinator
            .enrich_created_assets(&[], &assets, &observer)
            .await
            .unwrap();
        assert!(assets.calls.lock().unwrap().is_empty());
        assert!(observer.0.lock().unwrap().is_empty());

        let ids = ["new-1", "new-0", "new-0", "new-2", "new-3", "missing", ""].map(String::from);
        h.coordinator
            .enrich_created_assets(&ids, &assets, &observer)
            .await
            .unwrap();
        assert_eq!(
            *assets.calls.lock().unwrap(),
            vec![vec!["new-0".to_string(), "new-1".to_string()]],
        );
        assert_eq!(
            *observer.0.lock().unwrap(),
            vec![
                Progress::Started(2),
                Progress::Advanced(2, 2),
                Progress::Completed(2, 0, 0)
            ],
        );
        // All other provider-fetch methods in this fixture panic if invoked.
        assert_eq!(h.quote_service.daily_sync_calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn creation_enrichment_isolates_failed_chunks_without_retrying() {
        let h = harness(facts_with_new_assets(7)).await;
        let assets = RecordingAssets::default();
        assets.fail_first.store(true, Ordering::SeqCst);
        let observer = RecordingObserver::default();
        let ids: Vec<_> = (0..7).map(|index| format!("new-{index}")).collect();
        h.coordinator
            .enrich_created_assets(&ids, &assets, &observer)
            .await
            .unwrap();
        assert_eq!(
            *assets.calls.lock().unwrap(),
            vec![ids[..5].to_vec(), ids[5..].to_vec()],
        );
        assert_eq!(
            *observer.0.lock().unwrap(),
            vec![
                Progress::Started(7),
                Progress::Advanced(5, 7),
                Progress::Advanced(7, 7),
                Progress::Completed(2, 0, 5),
            ],
        );
        let report = h
            .coordinator
            .update_all(MarketSyncMode::None, &SilentObserver)
            .await;
        assert!(report.failures.is_empty());
        assert_eq!(h.quote_service.daily_sync_calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn concurrent_creation_batches_share_the_existing_job_lock() {
        let h = harness(facts_with_new_assets(2)).await;
        let assets = RecordingAssets::default();
        let first = vec!["new-0".to_string()];
        let second = vec!["new-1".to_string()];
        let (first_result, second_result) = tokio::join!(
            h.coordinator
                .enrich_created_assets(&first, &assets, &SilentObserver),
            h.coordinator
                .enrich_created_assets(&second, &assets, &SilentObserver),
        );
        first_result.unwrap();
        second_result.unwrap();
        assert_eq!(assets.calls.lock().unwrap().len(), 2);
        assert_eq!(assets.peak.load(Ordering::SeqCst), 1);
        assert_eq!(h.quote_service.daily_sync_calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn creation_enrichment_does_not_change_schedule_and_serializes_with_daily() {
        let h = harness(facts_with_new_assets(1)).await;
        let entered = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let assets = RecordingAssets {
            pause: Some((entered.clone(), release.clone())),
            ..Default::default()
        };
        let settings = TestSettings::default();
        let ids = vec!["new-0".to_string()];
        let (created, daily) = tokio::join!(
            h.coordinator
                .enrich_created_assets(&ids, &assets, &SilentObserver),
            async {
                entered.notified().await;
                assert!(settings.marker.lock().unwrap().is_none());
                assert!(h.coordinator.job.try_lock().is_err());
                let daily = h
                    .coordinator
                    .update_if_due(&settings, &assets, &SilentObserver);
                tokio::pin!(daily);
                assert!(
                    tokio::time::timeout(std::time::Duration::from_millis(10), &mut daily)
                        .await
                        .is_err()
                );
                assert!(settings.marker.lock().unwrap().is_none());
                assert_eq!(h.quote_service.daily_sync_calls.load(Ordering::SeqCst), 0);
                release.notify_one();
                daily.await
            },
        );
        created.unwrap();
        assert!(daily.unwrap().is_some());
        assert!(settings.marker.lock().unwrap().is_some());
        assert_eq!(assets.calls.lock().unwrap().len(), 1);
        assert_eq!(h.quote_service.daily_sync_calls.load(Ordering::SeqCst), 1);
    }
}
