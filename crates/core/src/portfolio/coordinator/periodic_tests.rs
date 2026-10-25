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
    assert!(missing.series.is_empty());
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
