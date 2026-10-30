//! Settings updates on real SQLite: a rejected update, including one with an
//! invalid base currency, changes neither the base currency nor its FX pairs,
//! and one that fails to save keeps the old base currency.
#![allow(clippy::unwrap_used, clippy::panic, reason = "test code")]
mod common;

use std::sync::Arc;

use diesel::prelude::*;
use serde_json::json;
use wealthfolio_core::events::MockDomainEventSink;
use wealthfolio_core::fx::FxService;
use wealthfolio_core::settings::{SettingsService, SettingsServiceTrait, SettingsUpdate};
use wealthfolio_storage_sqlite::{
    db, fx::FxRepository, schema::assets, settings::SettingsRepository, DbPool,
};

struct Fixture {
    settings: SettingsService,
    pool: Arc<DbPool>,
    events: MockDomainEventSink,
    _dir: common::TestDir,
}

async fn fixture() -> Fixture {
    let dir = common::TestDir::new();
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
    let events = MockDomainEventSink::new();
    let fx = FxService::new(Arc::new(FxRepository::new(pool.clone(), writer.clone())))
        .with_event_sink(Arc::new(events.clone()));
    let settings = SettingsService::new(
        Arc::new(SettingsRepository::new(pool.clone(), writer)),
        Arc::new(fx),
    );
    // A CAD account needs an FX pair to every new base currency.
    diesel::sql_query("INSERT INTO accounts (id, name, currency) VALUES ('cad', 'CAD', 'CAD')")
        .execute(&mut db::get_connection(&pool).unwrap())
        .unwrap();
    settings
        .update_settings(&update(json!({"baseCurrency": "USD", "timezone": "UTC"})))
        .await
        .unwrap();
    events.clear();
    Fixture {
        settings,
        pool,
        events,
        _dir: dir,
    }
}

fn update(value: serde_json::Value) -> SettingsUpdate {
    serde_json::from_value(value).unwrap()
}

fn fx_pairs(pool: &DbPool) -> Vec<Option<String>> {
    assets::table
        .filter(assets::kind.eq("FX"))
        .select(assets::instrument_key)
        .order(assets::instrument_key)
        .load(&mut db::get_connection(pool).unwrap())
        .unwrap()
}

fn assert_unchanged(fixture: &Fixture) {
    let settings = fixture.settings.get_settings().unwrap();
    assert_eq!(settings.base_currency, "USD");
    assert_eq!(settings.timezone, "UTC");
}

#[tokio::test]
async fn rejected_update_keeps_the_base_currency_and_registers_no_fx_pair() {
    let fixture = fixture().await;
    assert_eq!(fx_pairs(&fixture.pool), [Some("FX:CAD/USD".to_string())]);

    let rejected = update(json!({"baseCurrency": "EUR", "timezone": "Mars/Olympus"}));
    assert!(fixture.settings.update_settings(&rejected).await.is_err());

    assert_unchanged(&fixture);
    assert_eq!(fx_pairs(&fixture.pool), [Some("FX:CAD/USD".to_string())]);
    assert!(fixture.events.events().is_empty());

    let accepted = update(json!({"baseCurrency": "EUR", "timezone": "Europe/Paris"}));
    fixture.settings.update_settings(&accepted).await.unwrap();
    let settings = fixture.settings.get_settings().unwrap();
    assert_eq!(settings.base_currency, "EUR");
    assert_eq!(settings.timezone, "Europe/Paris");
    assert_eq!(
        fx_pairs(&fixture.pool),
        [
            Some("FX:CAD/EUR".to_string()),
            Some("FX:CAD/USD".to_string()),
            Some("FX:USD/EUR".to_string())
        ]
    );
}

#[tokio::test]
async fn failed_save_keeps_the_base_currency() {
    let fixture = fixture().await;
    diesel::sql_query(
        "CREATE TRIGGER fail_timezone BEFORE INSERT ON app_settings \
         WHEN NEW.setting_key = 'timezone' BEGIN SELECT RAISE(ABORT, 'disk full'); END",
    )
    .execute(&mut db::get_connection(&fixture.pool).unwrap())
    .unwrap();

    let failing = update(json!({"baseCurrency": "EUR", "timezone": "Europe/Paris"}));
    assert!(fixture.settings.update_settings(&failing).await.is_err());

    assert_unchanged(&fixture);
}

#[tokio::test]
async fn market_data_sync_interval_defaults_round_trips_and_survives_other_updates() {
    let fixture = fixture().await;
    assert_eq!(
        fixture
            .settings
            .get_settings()
            .unwrap()
            .market_data_sync_interval_hours,
        6
    );
    for hours in [1, 6, 12, 24] {
        fixture
            .settings
            .update_settings(&update(json!({"marketDataSyncIntervalHours": hours})))
            .await
            .unwrap();
        assert_eq!(
            fixture
                .settings
                .get_settings()
                .unwrap()
                .market_data_sync_interval_hours,
            hours
        );
    }
    fixture
        .settings
        .update_settings(&update(json!({"theme": "dark"})))
        .await
        .unwrap();
    assert_eq!(
        fixture
            .settings
            .get_settings()
            .unwrap()
            .market_data_sync_interval_hours,
        24
    );
    assert!(fixture.events.events().is_empty());
}

#[tokio::test]
async fn invalid_market_data_sync_interval_rejects_the_whole_update() {
    let fixture = fixture().await;
    for hours in [0, 2, 5, 25] {
        let rejected = update(json!({"marketDataSyncIntervalHours": hours, "baseCurrency": "EUR"}));
        assert!(fixture.settings.update_settings(&rejected).await.is_err());
        assert_unchanged(&fixture);
        assert_eq!(
            fixture
                .settings
                .get_settings()
                .unwrap()
                .market_data_sync_interval_hours,
            6
        );
        assert_eq!(fx_pairs(&fixture.pool), [Some("FX:CAD/USD".to_string())]);
    }
    assert!(fixture.events.events().is_empty());
}

#[tokio::test]
async fn invalid_persisted_market_data_sync_interval_is_reported() {
    use wealthfolio_core::settings::MARKET_DATA_SYNC_INTERVAL_HOURS_KEY;
    let fixture = fixture().await;
    for value in ["garbage", "0", "2", "4294967296"] {
        fixture
            .settings
            .set_setting_value(MARKET_DATA_SYNC_INTERVAL_HOURS_KEY, value)
            .await
            .unwrap();
        assert!(fixture.settings.get_settings().is_err());
    }
}

#[tokio::test]
async fn gain_loss_color_scheme_is_local_and_rejects_invalid_updates_atomically() {
    use wealthfolio_storage_sqlite::schema::sync_outbox;
    let fixture = fixture().await;
    assert_eq!(
        fixture
            .settings
            .get_settings()
            .unwrap()
            .gain_loss_color_scheme,
        "green-up-red-down"
    );
    let outbox_count = || {
        sync_outbox::table
            .count()
            .get_result::<i64>(&mut db::get_connection(&fixture.pool).unwrap())
            .unwrap()
    };
    let before = outbox_count();
    let pairs_before = fx_pairs(&fixture.pool);
    for scheme in ["red-up-green-down", "green-up-red-down"] {
        fixture
            .settings
            .update_settings(&update(json!({"gainLossColorScheme": scheme})))
            .await
            .unwrap();
        fixture
            .settings
            .update_settings(&update(json!({"theme": "dark"})))
            .await
            .unwrap();
        assert_eq!(
            fixture
                .settings
                .get_settings()
                .unwrap()
                .gain_loss_color_scheme,
            scheme
        );
    }
    assert!(fixture
        .settings
        .update_settings(&update(json!({
            "gainLossColorScheme": "invalid", "baseCurrency": "EUR"
        })))
        .await
        .is_err());
    assert_unchanged(&fixture);
    assert_eq!(fx_pairs(&fixture.pool), pairs_before);
    assert!(fixture.events.events().is_empty());
    assert_eq!(outbox_count(), before);
}

#[test]
fn old_settings_payload_uses_default_gain_loss_colors() {
    use wealthfolio_core::settings::Settings;
    let mut payload = serde_json::to_value(Settings::default()).unwrap();
    payload
        .as_object_mut()
        .unwrap()
        .remove("gainLossColorScheme");
    let loaded: Settings = serde_json::from_value(payload).unwrap();
    assert_eq!(loaded.gain_loss_color_scheme, "green-up-red-down");
}

#[tokio::test]
async fn invalid_base_currency_is_rejected_before_any_fx_pair_is_registered() {
    let fixture = fixture().await;

    for invalid in ["", "usd", "GBp", "GBX", "USDT", "NOT A CURRENCY"] {
        let rejected = update(json!({"baseCurrency": invalid}));
        assert!(
            fixture.settings.update_settings(&rejected).await.is_err(),
            "{invalid:?} was accepted"
        );
    }

    assert_unchanged(&fixture);
    assert_eq!(fx_pairs(&fixture.pool), [Some("FX:CAD/USD".to_string())]);
    assert!(fixture.events.events().is_empty());
}

#[tokio::test]
async fn saved_invalid_base_currency_is_rejected_when_sent_again() {
    let fixture = fixture().await;
    diesel::sql_query(
        "UPDATE app_settings SET setting_value = 'usd' WHERE setting_key = 'base_currency'",
    )
    .execute(&mut db::get_connection(&fixture.pool).unwrap())
    .unwrap();

    let resent = update(json!({"baseCurrency": "usd", "timezone": "Europe/Paris"}));
    assert!(fixture.settings.update_settings(&resent).await.is_err());

    let settings = fixture.settings.get_settings().unwrap();
    assert_eq!(settings.base_currency, "usd");
    assert_eq!(settings.timezone, "UTC");
}

#[tokio::test]
async fn provider_request_timezone_defaults_and_stays_independent_of_display_timezone() {
    let fixture = fixture().await;
    assert_eq!(
        fixture
            .settings
            .get_settings()
            .unwrap()
            .provider_request_timezone,
        "Asia/Shanghai"
    );
    for timezone in ["America/New_York", "UTC", "Asia/Shanghai"] {
        fixture
            .settings
            .update_settings(&update(json!({"providerRequestTimezone": timezone})))
            .await
            .unwrap();
        let settings = fixture.settings.get_settings().unwrap();
        assert_eq!(settings.provider_request_timezone, timezone);
        assert_eq!(settings.timezone, "UTC");
        assert_eq!(
            fixture
                .settings
                .get_setting_value("provider_request_timezone")
                .unwrap()
                .as_deref(),
            Some(timezone)
        );
    }
    fixture
        .settings
        .update_settings(&update(json!({"timezone": "Europe/Paris"})))
        .await
        .unwrap();
    assert_eq!(
        fixture
            .settings
            .get_settings()
            .unwrap()
            .provider_request_timezone,
        "Asia/Shanghai"
    );
}

#[tokio::test]
async fn invalid_provider_request_timezone_rejects_the_entire_update() {
    let fixture = fixture().await;
    for timezone in ["", "UTC+8", "Mars/Olympus"] {
        assert!(fixture
            .settings
            .update_settings(&update(json!({
                "providerRequestTimezone": timezone,
                "baseCurrency": "EUR",
                "timezone": "Europe/Paris"
            })))
            .await
            .is_err());
        assert_unchanged(&fixture);
        assert_eq!(
            fixture
                .settings
                .get_settings()
                .unwrap()
                .provider_request_timezone,
            "Asia/Shanghai"
        );
        assert_eq!(fx_pairs(&fixture.pool), [Some("FX:CAD/USD".to_string())]);
        assert!(fixture.events.events().is_empty());
        assert!(fixture
            .settings
            .set_setting_value("provider_request_timezone", timezone)
            .await
            .is_err());
    }
}

#[tokio::test]
async fn daily_schedule_defaults_persist_and_validate_atomically() {
    let fixture = fixture().await;
    let config = fixture.settings.get_settings().unwrap();
    assert_eq!(config.market_data_sync_time, "22:00");
    assert_eq!(config.market_data_sync_timezone, "Asia/Shanghai");
    fixture
        .settings
        .update_settings(&update(
            json!({"marketDataSyncTime": "21:35", "marketDataSyncTimezone": "Asia/Tokyo"}),
        ))
        .await
        .unwrap();
    let config = fixture.settings.get_settings().unwrap();
    assert_eq!(config.market_data_sync_time, "21:35");
    assert_eq!(config.market_data_sync_timezone, "Asia/Tokyo");
    for time in ["24:00", "22:60", "7:00", "22:00:00", "bad"] {
        assert!(fixture
            .settings
            .update_settings(&update(
                json!({"marketDataSyncTime": time, "baseCurrency": "EUR"})
            ))
            .await
            .is_err());
        assert_eq!(
            fixture.settings.get_settings().unwrap().base_currency,
            "USD"
        );
    }
    assert!(fixture
        .settings
        .update_settings(&update(json!({"marketDataSyncTimezone": "Mars/Olympus"})))
        .await
        .is_err());
    let marker = wealthfolio_core::settings::MARKET_DATA_AUTO_SYNC_LAST_ATTEMPT_KEY;
    fixture
        .settings
        .set_setting_value(marker, "2026-10-25T14:00:00Z")
        .await
        .unwrap();
    fixture
        .settings
        .update_settings(&update(json!({"marketDataSyncTime": "23:00"})))
        .await
        .unwrap();
    assert_eq!(
        fixture
            .settings
            .get_setting_value(marker)
            .unwrap()
            .as_deref(),
        Some("2026-10-25T14:00:00Z")
    );
    assert!(fixture.events.events().is_empty());
}
