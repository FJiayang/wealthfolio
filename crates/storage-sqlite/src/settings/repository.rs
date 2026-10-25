use async_trait::async_trait;
use diesel::prelude::*;
use std::sync::Arc;

use super::model::{AppPreferenceDB, AppSettingDB};
use crate::db::{get_connection, DbPool, WriteHandle};
use crate::errors::StorageError;
use crate::schema::app_settings::dsl::*;
use crate::schema::{accounts, assets};
use wealthfolio_core::assets::AssetKind;
use wealthfolio_core::errors::{Error, Result};
use wealthfolio_core::settings::{
    validate_provider_request_timezone, MarketDataAutoSyncRecord, MarketDataSyncMode,
    MarketDataSyncSchedule, Settings, SettingsRepositoryTrait, SettingsUpdate,
    CURRENCY_FORMATTING_REGIONS_KEY, DEFAULT_PROVIDER_REQUEST_TIMEZONE, GAIN_LOSS_COLOR_SCHEME_KEY,
    INSIGHTS_OVERVIEW_LAYOUT_KEY, MARKET_DATA_AUTO_SYNC_RECORD_KEY,
    MARKET_DATA_SYNC_INTERVAL_HOURS_KEY, MARKET_DATA_SYNC_MODE_KEY, MARKET_DATA_SYNC_TIMEZONE_KEY,
    MARKET_DATA_SYNC_TIME_KEY, PROVIDER_REQUEST_TIMEZONE_KEY,
};

pub struct SettingsRepository {
    pool: Arc<DbPool>,
    writer: WriteHandle,
}

impl SettingsRepository {
    pub fn new(pool: Arc<DbPool>, writer: WriteHandle) -> Self {
        SettingsRepository { pool, writer }
    }
}

fn read_settings(conn: &mut SqliteConnection) -> Result<Settings> {
    let all_settings: Vec<(String, String)> = app_settings
        .select((setting_key, setting_value))
        .load::<(String, String)>(conn)
        .map_err(StorageError::from)?;

    let mut settings = Settings::default(); // Use default implementation

    for (key, value) in all_settings {
        match key.as_str() {
            "theme" => settings.theme = value,
            "font" => settings.font = value,
            GAIN_LOSS_COLOR_SCHEME_KEY => settings.gain_loss_color_scheme = value,
            "language" => settings.language = value,
            "formatting_region" => settings.formatting_region = value,
            CURRENCY_FORMATTING_REGIONS_KEY => {
                settings.currency_formatting_regions =
                    serde_json::from_str(&value).unwrap_or_default();
            }
            "base_currency" => settings.base_currency = value,
            "timezone" => settings.timezone = value,
            PROVIDER_REQUEST_TIMEZONE_KEY => settings.provider_request_timezone = value,
            "onboarding_completed" => {
                settings.onboarding_completed = value.parse().unwrap_or(false);
            }
            "auto_update_check_enabled" => {
                settings.auto_update_check_enabled = value.parse().unwrap_or(true);
            }
            "menu_bar_visible" => {
                settings.menu_bar_visible = value.parse().unwrap_or(true);
            }
            "sync_enabled" => {
                settings.sync_enabled = value.parse().unwrap_or(true);
            }
            MARKET_DATA_SYNC_INTERVAL_HOURS_KEY => {
                settings.market_data_sync_interval_hours = value.parse().map_err(|_| {
                    Error::InvalidConfigValue("Invalid market data sync interval".to_string())
                })?;
            }
            MARKET_DATA_SYNC_MODE_KEY => {
                settings.market_data_sync_mode = match value.as_str() {
                    "interval" => MarketDataSyncMode::Interval,
                    "daily" => MarketDataSyncMode::Daily,
                    _ => {
                        return Err(Error::InvalidConfigValue(
                            "Invalid market data sync mode".to_string(),
                        ))
                    }
                };
            }
            MARKET_DATA_SYNC_TIME_KEY => settings.market_data_sync_time = Some(value),
            MARKET_DATA_SYNC_TIMEZONE_KEY => settings.market_data_sync_timezone = Some(value),
            "restore_reconnect_required" => {
                settings.restore_reconnect_required = value == "true";
            }
            "default_return_metric" => settings.default_return_metric = value,
            INSIGHTS_OVERVIEW_LAYOUT_KEY => {
                settings.insights_overview_layout = serde_json::from_str(&value).ok();
            }
            _ => {} // Ignore unknown settings
        }
    }

    Ok(settings)
}

fn read_auto_sync_record(conn: &mut SqliteConnection) -> Result<Option<MarketDataAutoSyncRecord>> {
    let stored: Option<String> = app_settings
        .filter(setting_key.eq(MARKET_DATA_AUTO_SYNC_RECORD_KEY))
        .select(setting_value)
        .first(conn)
        .optional()
        .map_err(StorageError::from)?;
    stored
        .map(|value| {
            serde_json::from_str(&value).map_err(|err| {
                Error::InvalidConfigValue(format!("Invalid automatic market sync state: {err}"))
            })
        })
        .transpose()
}

fn write_auto_sync_record(
    conn: &mut SqliteConnection,
    record: &MarketDataAutoSyncRecord,
) -> Result<()> {
    let value =
        serde_json::to_string(record).map_err(|err| Error::InvalidConfigValue(err.to_string()))?;
    diesel::replace_into(app_settings)
        .values(&AppSettingDB {
            setting_key: MARKET_DATA_AUTO_SYNC_RECORD_KEY.to_string(),
            setting_value: value,
        })
        .execute(conn)
        .map_err(StorageError::from)?;
    Ok(())
}

// Implement the trait for SettingsRepository
#[async_trait]
impl SettingsRepositoryTrait for SettingsRepository {
    async fn claim_market_data_sync_attempt(
        &self,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<bool> {
        self.writer
            .exec_tx(move |tx| {
                let schedule = MarketDataSyncSchedule::from_settings(&read_settings(tx.conn())?)?;
                let mut record = match read_auto_sync_record(tx.conn())? {
                    Some(record) => record,
                    None => {
                        let mut record =
                            MarketDataAutoSyncRecord::configured(schedule.clone(), now);
                        // Legacy interval setup keeps the existing delayed first run.
                        let due = matches!(schedule, MarketDataSyncSchedule::Interval { .. });
                        if due {
                            record.last_attempt_started_at = Some(now);
                        }
                        write_auto_sync_record(tx.conn(), &record)?;
                        return Ok(due);
                    }
                };
                if record.schedule != schedule {
                    record.schedule = schedule;
                    record.effective_since = now;
                    write_auto_sync_record(tx.conn(), &record)?;
                    return Ok(false);
                }
                if !record.is_due(now)? {
                    return Ok(false);
                }
                record.last_attempt_started_at = Some(now);
                write_auto_sync_record(tx.conn(), &record)?;
                Ok(true)
            })
            .await
    }

    fn get_settings(&self) -> Result<Settings> {
        let mut conn = get_connection(&self.pool)?;
        read_settings(&mut conn)
    }

    async fn update_settings(&self, new_settings: &SettingsUpdate) -> Result<()> {
        if let Some(ref timezone) = new_settings.provider_request_timezone {
            validate_provider_request_timezone(timezone)?;
        }
        let settings = new_settings.clone();
        self.writer
            .exec_tx(move |tx| {
                let previous_schedule =
                    MarketDataSyncSchedule::from_settings(&read_settings(tx.conn())?)?;
                if let Some(ref theme) = settings.theme {
                    diesel::replace_into(app_settings)
                        .values(&AppSettingDB {
                            setting_key: "theme".to_string(),
                            setting_value: theme.clone(),
                        })
                        .execute(tx.conn())
                        .map_err(StorageError::from)?;
                }

                if let Some(ref scheme) = settings.gain_loss_color_scheme {
                    diesel::replace_into(app_settings)
                        .values(&AppSettingDB {
                            setting_key: GAIN_LOSS_COLOR_SCHEME_KEY.to_string(),
                            setting_value: scheme.clone(),
                        })
                        .execute(tx.conn())
                        .map_err(StorageError::from)?;
                }

                if let Some(ref font) = settings.font {
                    diesel::replace_into(app_settings)
                        .values(&AppSettingDB {
                            setting_key: "font".to_string(),
                            setting_value: font.clone(),
                        })
                        .execute(tx.conn())
                        .map_err(StorageError::from)?;
                }

                if let Some(ref language) = settings.language {
                    diesel::replace_into(app_settings)
                        .values(&AppSettingDB {
                            setting_key: "language".to_string(),
                            setting_value: language.clone(),
                        })
                        .execute(tx.conn())
                        .map_err(StorageError::from)?;
                }

                if let Some(ref formatting_region) = settings.formatting_region {
                    diesel::replace_into(app_settings)
                        .values(&AppSettingDB {
                            setting_key: "formatting_region".to_string(),
                            setting_value: formatting_region.clone(),
                        })
                        .execute(tx.conn())
                        .map_err(StorageError::from)?;
                }

                if let Some(ref regions) = settings.currency_formatting_regions {
                    let regions: serde_json::Map<String, serde_json::Value> = regions
                        .iter()
                        .map(|(currency, region)| {
                            (currency.clone(), serde_json::Value::String(region.clone()))
                        })
                        .collect();
                    diesel::replace_into(app_settings)
                        .values(&AppSettingDB {
                            setting_key: CURRENCY_FORMATTING_REGIONS_KEY.to_string(),
                            setting_value: serde_json::Value::Object(regions).to_string(),
                        })
                        .execute(tx.conn())
                        .map_err(StorageError::from)?;
                }

                if let Some(ref base_currency) = settings.base_currency {
                    diesel::replace_into(app_settings)
                        .values(&AppSettingDB {
                            setting_key: "base_currency".to_string(),
                            setting_value: base_currency.clone(),
                        })
                        .execute(tx.conn())
                        .map_err(StorageError::from)?;
                }

                if let Some(ref timezone) = settings.provider_request_timezone {
                    diesel::replace_into(app_settings)
                        .values(&AppSettingDB {
                            setting_key: PROVIDER_REQUEST_TIMEZONE_KEY.to_string(),
                            setting_value: timezone.clone(),
                        })
                        .execute(tx.conn())
                        .map_err(StorageError::from)?;
                }

                if let Some(ref timezone) = settings.timezone {
                    diesel::replace_into(app_settings)
                        .values(&AppSettingDB {
                            setting_key: "timezone".to_string(),
                            setting_value: timezone.clone(),
                        })
                        .execute(tx.conn())
                        .map_err(StorageError::from)?;
                }

                if let Some(onboarding_completed) = settings.onboarding_completed {
                    diesel::replace_into(app_settings)
                        .values(&AppSettingDB {
                            setting_key: "onboarding_completed".to_string(),
                            setting_value: onboarding_completed.to_string(),
                        })
                        .execute(tx.conn())
                        .map_err(StorageError::from)?;
                }

                if let Some(auto_update_check_enabled) = settings.auto_update_check_enabled {
                    diesel::replace_into(app_settings)
                        .values(&AppSettingDB {
                            setting_key: "auto_update_check_enabled".to_string(),
                            setting_value: auto_update_check_enabled.to_string(),
                        })
                        .execute(tx.conn())
                        .map_err(StorageError::from)?;
                }

                if let Some(menu_bar_visible) = settings.menu_bar_visible {
                    diesel::replace_into(app_settings)
                        .values(&AppSettingDB {
                            setting_key: "menu_bar_visible".to_string(),
                            setting_value: menu_bar_visible.to_string(),
                        })
                        .execute(tx.conn())
                        .map_err(StorageError::from)?;
                }

                if let Some(sync_enabled) = settings.sync_enabled {
                    diesel::replace_into(app_settings)
                        .values(&AppSettingDB {
                            setting_key: "sync_enabled".to_string(),
                            setting_value: sync_enabled.to_string(),
                        })
                        .execute(tx.conn())
                        .map_err(StorageError::from)?;
                }

                if let Some(hours) = settings.market_data_sync_interval_hours {
                    diesel::replace_into(app_settings)
                        .values(&AppSettingDB {
                            setting_key: MARKET_DATA_SYNC_INTERVAL_HOURS_KEY.to_string(),
                            setting_value: hours.to_string(),
                        })
                        .execute(tx.conn())
                        .map_err(StorageError::from)?;
                }

                for (key, value) in [
                    (
                        MARKET_DATA_SYNC_MODE_KEY,
                        settings.market_data_sync_mode.map(|mode| match mode {
                            MarketDataSyncMode::Interval => "interval".to_string(),
                            MarketDataSyncMode::Daily => "daily".to_string(),
                        }),
                    ),
                    (
                        MARKET_DATA_SYNC_TIME_KEY,
                        settings.market_data_sync_time.clone(),
                    ),
                    (
                        MARKET_DATA_SYNC_TIMEZONE_KEY,
                        settings.market_data_sync_timezone.clone(),
                    ),
                ] {
                    if let Some(value) = value {
                        diesel::replace_into(app_settings)
                            .values(&AppSettingDB {
                                setting_key: key.to_string(),
                                setting_value: value,
                            })
                            .execute(tx.conn())
                            .map_err(StorageError::from)?;
                    }
                }
                let next_schedule =
                    MarketDataSyncSchedule::from_settings(&read_settings(tx.conn())?)?;
                if next_schedule != previous_schedule {
                    let previous_attempt = read_auto_sync_record(tx.conn())?
                        .and_then(|record| record.last_attempt_started_at);
                    let mut record =
                        MarketDataAutoSyncRecord::configured(next_schedule, chrono::Utc::now());
                    record.last_attempt_started_at = previous_attempt;
                    write_auto_sync_record(tx.conn(), &record)?;
                }

                if let Some(ref default_return_metric) = settings.default_return_metric {
                    diesel::replace_into(app_settings)
                        .values(&AppSettingDB {
                            setting_key: "default_return_metric".to_string(),
                            setting_value: default_return_metric.clone(),
                        })
                        .execute(tx.conn())
                        .map_err(StorageError::from)?;
                }

                if let Some(ref layout) = settings.insights_overview_layout {
                    let row = AppSettingDB {
                        setting_key: INSIGHTS_OVERVIEW_LAYOUT_KEY.to_string(),
                        setting_value: serde_json::Value::Object(layout.clone()).to_string(),
                    };
                    diesel::replace_into(app_settings)
                        .values(&row)
                        .execute(tx.conn())
                        .map_err(StorageError::from)?;
                    tx.update(&AppPreferenceDB(&row))?;
                }

                Ok(())
            })
            .await
    }

    fn get_setting(&self, setting_key_param: &str) -> Result<String> {
        let mut conn = get_connection(&self.pool)?;
        let result = app_settings
            .filter(setting_key.eq(setting_key_param))
            .select(setting_value)
            .first(&mut conn);

        match result {
            Ok(value) => Ok(value),
            Err(diesel::result::Error::NotFound) => {
                // Return default values for known settings
                let default_value = match setting_key_param {
                    "theme" => "light",
                    "font" => "font-mono",
                    "language" => "en",
                    "timezone" => "",
                    PROVIDER_REQUEST_TIMEZONE_KEY => DEFAULT_PROVIDER_REQUEST_TIMEZONE,
                    "onboarding_completed" => "false",
                    "auto_update_check_enabled" => "true",
                    "menu_bar_visible" => "true",
                    "sync_enabled" => "true",
                    "default_return_metric" => "twr",
                    _ => return Err(StorageError::from(diesel::result::Error::NotFound).into()),
                };
                Ok(default_value.to_string())
            }
            Err(e) => Err(StorageError::from(e).into()),
        }
    }

    async fn update_setting(
        &self,
        setting_key_param: &str,
        setting_value_param: &str,
    ) -> Result<()> {
        if setting_key_param == PROVIDER_REQUEST_TIMEZONE_KEY {
            validate_provider_request_timezone(setting_value_param)?;
        }
        let key = setting_key_param.to_string();
        let value = setting_value_param.to_string();

        self.writer
            .exec_tx(move |tx| {
                let row = AppSettingDB {
                    setting_key: key,
                    setting_value: value,
                };
                diesel::replace_into(app_settings)
                    .values(&row)
                    .execute(tx.conn())
                    .map_err(StorageError::from)?;
                tx.update(&AppPreferenceDB(&row))?;
                Ok(())
            })
            .await
    }

    fn get_distinct_currencies_excluding_base(&self, base_currency: &str) -> Result<Vec<String>> {
        let mut conn = get_connection(&self.pool)?;

        let currency_assets: Vec<String> = assets::table
            .filter(assets::kind.eq(AssetKind::Fx.as_db_str()))
            .filter(assets::quote_ccy.ne(base_currency))
            .select(assets::quote_ccy)
            .distinct()
            .load::<String>(&mut conn)
            .map_err(StorageError::from)?;

        let account_currencies: Vec<String> = accounts::table
            .filter(accounts::currency.ne(base_currency))
            .select(accounts::currency)
            .distinct()
            .load::<String>(&mut conn)
            .map_err(StorageError::from)?;

        let mut all_currencies: Vec<String> = Vec::new();
        all_currencies.extend(currency_assets);
        all_currencies.extend(account_currencies);
        all_currencies.sort();
        all_currencies.dedup();

        Ok(all_currencies)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{create_pool, run_migrations, write_actor::spawn_writer};
    use serde_json::json;

    async fn setup() -> (SettingsRepository, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("settings.db").to_string_lossy().to_string();
        run_migrations(&db_path).unwrap();
        let pool = create_pool(&db_path).unwrap();
        let writer = spawn_writer((*pool).clone()).unwrap();
        (SettingsRepository::new(pool, writer), dir)
    }

    #[tokio::test]
    async fn insights_layout_round_trips_and_survives_unrelated_updates() {
        let (repo, _dir) = setup().await;
        assert!(repo
            .get_settings()
            .unwrap()
            .insights_overview_layout
            .is_none());
        let layout = json!({
            "version": 1,
            "hiddenWidgets": ["regions"],
            "layouts": {"desktop": [{"i": "composition", "x": 0, "y": 0, "w": 9, "h": 6}]}
        });
        let update: SettingsUpdate = serde_json::from_value(json!({
            "insightsOverviewLayout": layout
        }))
        .unwrap();
        repo.update_settings(&update).await.unwrap();
        let theme_update = serde_json::from_value(json!({"theme": "dark"})).unwrap();
        repo.update_settings(&theme_update).await.unwrap();
        let loaded = repo.get_settings().unwrap();
        assert_eq!(loaded.theme, "dark");
        assert_eq!(loaded.insights_overview_layout.as_ref(), layout.as_object());
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(
                &repo.get_setting(INSIGHTS_OVERVIEW_LAYOUT_KEY).unwrap()
            )
            .unwrap(),
            layout
        );
        let reset = serde_json::from_value(json!({
            "insightsOverviewLayout": {"version": 1, "hiddenWidgets": [], "layouts": {}}
        }))
        .unwrap();
        repo.update_settings(&reset).await.unwrap();
        assert_eq!(
            repo.get_settings()
                .unwrap()
                .insights_overview_layout
                .unwrap()["hiddenWidgets"],
            json!([])
        );
    }

    #[tokio::test]
    async fn currency_formatting_regions_round_trip_without_syncing() {
        use crate::schema::sync_outbox;
        let (repo, _dir) = setup().await;
        assert!(repo
            .get_settings()
            .unwrap()
            .currency_formatting_regions
            .is_empty());
        let update: SettingsUpdate = serde_json::from_value(json!({
            "currencyFormattingRegions": {"USD": "US", "INR": "IN"}
        }))
        .unwrap();
        repo.update_settings(&update).await.unwrap();
        let theme_update = serde_json::from_value(json!({"theme": "dark"})).unwrap();
        repo.update_settings(&theme_update).await.unwrap();
        assert_eq!(
            Some(repo.get_settings().unwrap().currency_formatting_regions),
            update.currency_formatting_regions
        );

        let clear = serde_json::from_value(json!({"currencyFormattingRegions": {}})).unwrap();
        repo.update_settings(&clear).await.unwrap();
        assert!(repo
            .get_settings()
            .unwrap()
            .currency_formatting_regions
            .is_empty());

        let mut conn = get_connection(&repo.pool).unwrap();
        let outbox: i64 = sync_outbox::table.count().get_result(&mut conn).unwrap();
        assert_eq!(outbox, 0, "Formatting preferences stay on this device");
    }

    #[tokio::test]
    async fn malformed_stored_currency_formatting_regions_use_default() {
        let (repo, _dir) = setup().await;
        for invalid in ["not json", "null", "[]", r#"{"INR": 1}"#] {
            repo.update_setting(CURRENCY_FORMATTING_REGIONS_KEY, invalid)
                .await
                .unwrap();
            assert!(repo
                .get_settings()
                .unwrap()
                .currency_formatting_regions
                .is_empty());
        }
    }

    #[tokio::test]
    async fn malformed_stored_insights_layout_uses_default() {
        let (repo, _dir) = setup().await;
        for invalid in ["not json", "null", "[]", "42"] {
            repo.update_setting(INSIGHTS_OVERVIEW_LAYOUT_KEY, invalid)
                .await
                .unwrap();
            assert!(repo
                .get_settings()
                .unwrap()
                .insights_overview_layout
                .is_none());
        }
    }
    #[tokio::test]
    async fn insights_layout_sync_outbox_is_scoped_and_transactional() {
        use crate::schema::sync_outbox;
        let (repo, _dir) = setup().await;
        let update: SettingsUpdate = serde_json::from_value(json!({
            "theme": "dark", "insightsOverviewLayout": {"version": 6, "hiddenWidgets": ["regions"]}
        }))
        .unwrap();
        repo.update_settings(&update).await.unwrap();
        repo.update_setting("font", "font-sans").await.unwrap();
        let mut conn = get_connection(&repo.pool).unwrap();
        let events: Vec<(String, String, String)> = sync_outbox::table
            .select((
                sync_outbox::entity,
                sync_outbox::entity_id,
                sync_outbox::payload,
            ))
            .load(&mut conn)
            .unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "app_preference");
        assert_eq!(events[0].1, INSIGHTS_OVERVIEW_LAYOUT_KEY);
        let payload: serde_json::Value = serde_json::from_str(&events[0].2).unwrap();
        assert_eq!(payload["setting_key"], INSIGHTS_OVERVIEW_LAYOUT_KEY);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(payload["setting_value"].as_str().unwrap())
                .unwrap()["hiddenWidgets"],
            json!(["regions"])
        );
        let (receiver, _receiver_dir) = setup().await;
        let sync_receiver = crate::sync::app_sync::AppSyncRepository::new(
            receiver.pool.clone(),
            receiver.writer.clone(),
        );
        assert!(sync_receiver
            .apply_remote_event_lww(
                wealthfolio_core::sync::SyncEntity::AppPreference,
                events[0].1.clone(),
                wealthfolio_core::sync::SyncOperation::Update,
                "received-layout".to_string(),
                "2026-09-17T12:00:00Z".to_string(),
                1,
                payload,
            )
            .await
            .unwrap());
        assert_eq!(
            receiver.get_settings().unwrap().insights_overview_layout,
            update.insights_overview_layout
        );
        let mut receiver_conn = get_connection(&receiver.pool).unwrap();
        let receiver_outbox: i64 = sync_outbox::table
            .count()
            .get_result(&mut receiver_conn)
            .unwrap();
        assert_eq!(
            receiver_outbox, 0,
            "Receiving the actual outbound payload must not echo it"
        );
        repo.update_setting(INSIGHTS_OVERVIEW_LAYOUT_KEY, r#"{"version":6}"#)
            .await
            .unwrap();
        let count: i64 = sync_outbox::table.count().get_result(&mut conn).unwrap();
        assert_eq!(count, 2, "Generic layout writes also enqueue preferences");
        diesel::sql_query("CREATE TRIGGER fail_preference_outbox BEFORE INSERT ON sync_outbox BEGIN SELECT RAISE(ABORT, 'outbox unavailable'); END").execute(&mut conn).unwrap();
        let failing: SettingsUpdate = serde_json::from_value(
            json!({"theme": "light", "insightsOverviewLayout": {"version": 99}}),
        )
        .unwrap();
        assert!(repo.update_settings(&failing).await.is_err());
        assert_eq!(repo.get_settings().unwrap().theme, "dark");
        assert_eq!(
            repo.get_setting(INSIGHTS_OVERVIEW_LAYOUT_KEY).unwrap(),
            r#"{"version":6}"#
        );
    }
    fn record(repo: &SettingsRepository) -> MarketDataAutoSyncRecord {
        serde_json::from_str(&repo.get_setting(MARKET_DATA_AUTO_SYNC_RECORD_KEY).unwrap()).unwrap()
    }

    #[tokio::test]
    async fn automatic_interval_first_attempt_restart_and_failed_attempt_are_deduplicated() {
        let (repo, _dir) = setup().await;
        let now = chrono::Utc::now();
        assert!(repo.claim_market_data_sync_attempt(now).await.unwrap());
        assert!(!repo.claim_market_data_sync_attempt(now).await.unwrap());
        let restarted = SettingsRepository::new(repo.pool.clone(), repo.writer.clone());
        assert!(!restarted
            .claim_market_data_sync_attempt(now + chrono::Duration::hours(5))
            .await
            .unwrap());
        assert!(restarted
            .claim_market_data_sync_attempt(now + chrono::Duration::hours(6))
            .await
            .unwrap());
        // Recording an attempt is independent of its success/failure.
        assert!(!restarted
            .claim_market_data_sync_attempt(
                now + chrono::Duration::hours(6) + chrono::Duration::minutes(1)
            )
            .await
            .unwrap());
        assert!(restarted
            .claim_market_data_sync_attempt(now + chrono::Duration::days(4))
            .await
            .unwrap());
        assert!(!restarted
            .claim_market_data_sync_attempt(now + chrono::Duration::days(4))
            .await
            .unwrap());
    }

    #[tokio::test]
    async fn automatic_attempt_claims_are_serialized_and_persistence_failure_prevents_start() {
        let (repo, _dir) = setup().await;
        let now = chrono::Utc::now();
        let (a, b) = tokio::join!(
            repo.claim_market_data_sync_attempt(now),
            repo.claim_market_data_sync_attempt(now)
        );
        assert_ne!(a.unwrap(), b.unwrap());
        let mut conn = get_connection(&repo.pool).unwrap();
        diesel::sql_query("CREATE TRIGGER fail_auto_state BEFORE INSERT ON app_settings WHEN NEW.setting_key = 'market_data_auto_sync_record' BEGIN SELECT RAISE(ABORT, 'unavailable'); END").execute(&mut conn).unwrap();
        assert!(repo
            .claim_market_data_sync_attempt(now + chrono::Duration::hours(6))
            .await
            .is_err());
        assert_eq!(record(&repo).last_attempt_started_at, Some(now));
    }

    #[tokio::test]
    async fn daily_configuration_is_atomic_validated_and_local_only() {
        use crate::schema::sync_outbox;
        let (repo, _dir) = setup().await;
        for invalid in [
            json!({"theme": "dark", "marketDataSyncMode": "daily"}),
            json!({"marketDataSyncMode": "daily", "marketDataSyncTime": "25:00", "marketDataSyncTimezone": "UTC"}),
            json!({"marketDataSyncMode": "daily", "marketDataSyncTime": "18:00", "marketDataSyncTimezone": "Not/AZone"}),
        ] {
            assert!(repo
                .update_settings(&serde_json::from_value(invalid).unwrap())
                .await
                .is_err());
            assert_eq!(repo.get_settings().unwrap().theme, "light");
            assert_eq!(
                repo.get_settings().unwrap().market_data_sync_mode,
                MarketDataSyncMode::Interval
            );
        }
        let daily = serde_json::from_value(json!({"marketDataSyncMode":"daily", "marketDataSyncTime":"18:00", "marketDataSyncTimezone":"Asia/Shanghai"})).unwrap();
        repo.update_settings(&daily).await.unwrap();
        let configured = record(&repo);
        assert!(configured.last_attempt_started_at.is_none());
        assert!(!repo
            .claim_market_data_sync_attempt(configured.effective_since)
            .await
            .unwrap());
        let due = configured
            .schedule
            .next_after(configured.effective_since)
            .unwrap();
        assert!(repo.claim_market_data_sync_attempt(due).await.unwrap());
        assert!(!repo
            .claim_market_data_sync_attempt(due + chrono::Duration::minutes(1))
            .await
            .unwrap());
        let resumed = due + chrono::Duration::days(4) - chrono::Duration::hours(2);
        assert!(repo.claim_market_data_sync_attempt(resumed).await.unwrap());
        assert!(!repo
            .claim_market_data_sync_attempt(due + chrono::Duration::days(4))
            .await
            .unwrap());
        assert!(repo
            .claim_market_data_sync_attempt(due + chrono::Duration::days(5))
            .await
            .unwrap());
        let mut conn = get_connection(&repo.pool).unwrap();
        let outbox: i64 = sync_outbox::table.count().get_result(&mut conn).unwrap();
        assert_eq!(
            outbox, 0,
            "Automatic task state stays on this installation/profile"
        );
        let before = repo.get_setting(MARKET_DATA_AUTO_SYNC_RECORD_KEY).unwrap();
        let receiver =
            crate::sync::app_sync::AppSyncRepository::new(repo.pool.clone(), repo.writer.clone());
        assert!(!receiver
            .apply_remote_event_lww(
                wealthfolio_core::sync::SyncEntity::AppPreference,
                MARKET_DATA_AUTO_SYNC_RECORD_KEY.to_string(),
                wealthfolio_core::sync::SyncOperation::Update,
                "remote-auto-attempt".to_string(),
                "2026-10-25T00:00:00Z".to_string(),
                1,
                json!({"settingKey": MARKET_DATA_AUTO_SYNC_RECORD_KEY, "settingValue": "remote"}),
            )
            .await
            .unwrap());
        assert_eq!(
            repo.get_setting(MARKET_DATA_AUTO_SYNC_RECORD_KEY).unwrap(),
            before
        );
    }

    #[tokio::test]
    async fn mode_changes_preserve_inactive_configuration_and_only_future_schedule() {
        let (repo, _dir) = setup().await;
        repo.update_settings(&serde_json::from_value(json!({"marketDataSyncIntervalHours": 12, "marketDataSyncMode":"daily", "marketDataSyncTime":"18:00", "marketDataSyncTimezone":"Asia/Shanghai"})).unwrap()).await.unwrap();
        repo.update_settings(
            &serde_json::from_value(json!({"marketDataSyncMode":"interval"})).unwrap(),
        )
        .await
        .unwrap();
        let current = repo.get_settings().unwrap();
        assert_eq!(current.market_data_sync_interval_hours, 12);
        assert_eq!(current.market_data_sync_time.as_deref(), Some("18:00"));
        assert_eq!(
            current.market_data_sync_timezone.as_deref(),
            Some("Asia/Shanghai")
        );
        let changed = record(&repo);
        assert!(!repo
            .claim_market_data_sync_attempt(changed.effective_since)
            .await
            .unwrap());
        assert!(repo
            .claim_market_data_sync_attempt(changed.effective_since + chrono::Duration::hours(12))
            .await
            .unwrap());
        let before = repo.get_setting(MARKET_DATA_AUTO_SYNC_RECORD_KEY).unwrap();
        repo.update_settings(
            &serde_json::from_value(json!({"theme":"dark", "marketDataSyncTime":"20:00"})).unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(
            repo.get_setting(MARKET_DATA_AUTO_SYNC_RECORD_KEY).unwrap(),
            before,
            "Inactive settings and local/manual activity must not change the active plan"
        );
    }
}
