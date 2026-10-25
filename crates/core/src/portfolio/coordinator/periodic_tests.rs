use super::*;
use crate::settings::{Settings, SettingsUpdate};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::Duration;
use tokio::time::advance;

struct TestSettings {
    unavailable: AtomicBool,
    claims: AtomicU32,
    daily: AtomicBool,
    has_record: AtomicBool,
}

impl TestSettings {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            unavailable: AtomicBool::new(true),
            claims: AtomicU32::new(0),
            daily: AtomicBool::new(false),
            has_record: AtomicBool::new(false),
        })
    }
}

#[async_trait]
impl SettingsServiceTrait for TestSettings {
    async fn claim_market_data_sync_attempt(
        &self,
        _: chrono::DateTime<chrono::Utc>,
    ) -> Result<bool> {
        self.claims.fetch_add(1, Ordering::SeqCst);
        if self.unavailable.load(Ordering::SeqCst) {
            Err(Error::InvalidConfigValue("Persistence unavailable".into()))
        } else {
            Ok(false)
        }
    }
    fn get_settings(&self) -> Result<Settings> {
        Ok(Settings {
            market_data_sync_mode: if self.daily.load(Ordering::SeqCst) {
                crate::settings::MarketDataSyncMode::Daily
            } else {
                crate::settings::MarketDataSyncMode::Interval
            },
            ..Settings::default()
        })
    }
    async fn update_settings(&self, _: &SettingsUpdate) -> Result<()> {
        unreachable!()
    }
    fn get_base_currency(&self) -> Result<Option<String>> {
        unreachable!()
    }
    fn is_auto_update_check_enabled(&self) -> Result<bool> {
        unreachable!()
    }
    fn is_sync_enabled(&self) -> Result<bool> {
        unreachable!()
    }
    fn get_setting_value(&self, _: &str) -> Result<Option<String>> {
        Ok(self
            .has_record
            .load(Ordering::SeqCst)
            .then(|| "existing-record".to_string()))
    }
    async fn set_setting_value(&self, _: &str, _: &str) -> Result<()> {
        unreachable!()
    }
}

async fn elapse(seconds: u64) {
    advance(Duration::from_secs(seconds)).await;
    tokio::task::yield_now().await;
}

#[tokio::test(start_paused = true)]
async fn periodic_worker_preserves_startup_delay_checks_each_minute_and_stops_with_profile() {
    #[derive(Default)]
    struct Observer {
        starts: AtomicU32,
    }
    impl JobObserver for Observer {
        fn market_sync_started(&self) {
            self.starts.fetch_add(1, Ordering::SeqCst);
        }
        fn update_started(&self) {
            self.starts.fetch_add(1, Ordering::SeqCst);
        }
    }
    let observer = Arc::new(Observer::default());
    let h = harness(scenario("NOM-TRADE-01").facts()).await;
    let settings = TestSettings::new();
    let task = tokio::spawn(run_periodic_update(
        Arc::new(h.coordinator),
        observer.clone(),
        settings.clone(),
        Duration::from_secs(120),
    ));
    tokio::task::yield_now().await;
    elapse(119).await;
    assert_eq!(settings.claims.load(Ordering::SeqCst), 0);
    elapse(1).await;
    assert_eq!(settings.claims.load(Ordering::SeqCst), 1);
    elapse(60).await;
    assert_eq!(settings.claims.load(Ordering::SeqCst), 2);
    settings.unavailable.store(false, Ordering::SeqCst);
    elapse(60).await;
    assert_eq!(settings.claims.load(Ordering::SeqCst), 3);
    assert_eq!(
        observer.starts.load(Ordering::SeqCst),
        0,
        "A failed claim or non-due schedule cannot start coordinator/network work"
    );
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    elapse(24 * 3600).await;
    assert_eq!(settings.claims.load(Ordering::SeqCst), 3);
}

#[test]
fn startup_grace_only_applies_to_first_interval_and_observes_config_changes() {
    let settings = TestSettings::new();
    let grace = Duration::from_secs(120);
    assert!(!periodic_startup_ready(settings.as_ref(), Duration::ZERO, grace).unwrap());
    assert!(!periodic_startup_ready(settings.as_ref(), Duration::from_secs(60), grace).unwrap());
    settings.daily.store(true, Ordering::SeqCst);
    assert!(periodic_startup_ready(settings.as_ref(), Duration::from_secs(60), grace).unwrap());
    settings.daily.store(false, Ordering::SeqCst);
    settings.has_record.store(true, Ordering::SeqCst);
    assert!(periodic_startup_ready(settings.as_ref(), Duration::ZERO, grace).unwrap());
    settings.has_record.store(false, Ordering::SeqCst);
    assert!(periodic_startup_ready(settings.as_ref(), grace, grace).unwrap());
}
