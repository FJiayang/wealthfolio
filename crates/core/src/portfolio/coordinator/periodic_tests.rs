use super::*;
use crate::settings::{Settings, SettingsUpdate};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::Duration;
use tokio::time::{advance, Instant};

struct TestSettings {
    hours: AtomicU32,
    unavailable: AtomicBool,
    reads: AtomicU32,
}

impl TestSettings {
    fn new(hours: u32) -> Arc<Self> {
        Arc::new(Self {
            hours: AtomicU32::new(hours),
            unavailable: AtomicBool::new(false),
            reads: AtomicU32::new(0),
        })
    }
}

#[async_trait]
impl SettingsServiceTrait for TestSettings {
    fn get_settings(&self) -> Result<Settings> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        if self.unavailable.load(Ordering::SeqCst) {
            return Err(Error::InvalidConfigValue("Settings unavailable".into()));
        }
        Ok(Settings {
            market_data_sync_interval_hours: self.hours.load(Ordering::SeqCst),
            ..Settings::default()
        })
    }
    async fn update_settings(&self, _: &SettingsUpdate) -> Result<()> {
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
    fn get_setting_value(&self, _: &str) -> Result<Option<String>> {
        unimplemented!()
    }
    async fn set_setting_value(&self, _: &str, _: &str) -> Result<()> {
        unimplemented!()
    }
}

async fn wait_task(
    settings: Arc<TestSettings>,
    last_completed: Option<Instant>,
) -> tokio::task::JoinHandle<()> {
    let task = tokio::spawn(async move {
        wait_for_periodic_update(settings.as_ref(), last_completed).await;
    });
    tokio::task::yield_now().await;
    task
}

async fn elapse(seconds: u64) {
    advance(Duration::from_secs(seconds)).await;
    tokio::task::yield_now().await;
}

#[tokio::test(start_paused = true)]
async fn periodic_first_run_is_ready_but_later_runs_wait_from_completion() {
    let settings = TestSettings::new(6);
    wait_task(settings.clone(), None).await.await.unwrap();
    let completed = Instant::now();
    // Time spent in a job does not count toward the following interval.
    elapse(300).await;
    let task = wait_task(settings, Some(completed + Duration::from_secs(300))).await;
    elapse(6 * 3600 - 60).await;
    assert!(!task.is_finished());
    elapse(60).await;
    task.await.unwrap();
}

#[tokio::test(start_paused = true)]
async fn periodic_interval_can_be_extended_then_shortened_without_restart() {
    let settings = TestSettings::new(1);
    let task = wait_task(settings.clone(), Some(Instant::now())).await;
    settings.hours.store(24, Ordering::SeqCst);
    elapse(2 * 3600).await;
    assert!(!task.is_finished());
    settings.hours.store(1, Ordering::SeqCst);
    elapse(59).await;
    assert!(!task.is_finished());
    elapse(1).await;
    task.await.unwrap();
}

#[tokio::test(start_paused = true)]
async fn periodic_read_failure_prevents_sync_until_settings_recover() {
    let settings = TestSettings::new(1);
    settings.unavailable.store(true, Ordering::SeqCst);
    let task = wait_task(settings.clone(), None).await;
    elapse(3600).await;
    assert!(!task.is_finished());
    settings.unavailable.store(false, Ordering::SeqCst);
    elapse(60).await;
    task.await.unwrap();

    // Every completed attempt, including a failed job, starts a full wait.
    let task = wait_task(settings, Some(Instant::now())).await;
    elapse(60).await;
    assert!(!task.is_finished());
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    elapse(24 * 3600).await;
}

#[tokio::test(start_paused = true)]
async fn periodic_worker_preserves_startup_delay_and_stops_with_its_profile() {
    struct Observer;
    impl JobObserver for Observer {}
    let h = harness(scenario("NOM-TRADE-01").facts()).await;
    let settings = TestSettings::new(6);
    settings.unavailable.store(true, Ordering::SeqCst);
    let task = tokio::spawn(run_periodic_update(
        Arc::new(h.coordinator),
        Arc::new(Observer),
        settings.clone(),
        Duration::from_secs(120),
    ));
    tokio::task::yield_now().await;
    elapse(119).await;
    assert_eq!(settings.reads.load(Ordering::SeqCst), 0);
    elapse(1).await;
    assert_eq!(settings.reads.load(Ordering::SeqCst), 1);
    elapse(60).await;
    assert_eq!(settings.reads.load(Ordering::SeqCst), 2);
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    elapse(24 * 3600).await;
    assert_eq!(settings.reads.load(Ordering::SeqCst), 2);
}
