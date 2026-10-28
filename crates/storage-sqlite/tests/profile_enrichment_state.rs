//! Profile completion must be recorded even before the first quote sync.
#![allow(clippy::unwrap_used, clippy::panic, reason = "test code")]

use std::sync::Arc;

use chrono::{DateTime, NaiveDate, Utc};
use diesel::prelude::*;
use wealthfolio_core::assets::{
    AssetKind, AssetRepositoryTrait, AssetService, AssetServiceTrait, InstrumentType, NewAsset,
    QuoteMode,
};
use wealthfolio_core::quotes::{QuoteService, QuoteServiceTrait, QuoteSyncState, SyncStateStore};
use wealthfolio_core::secrets::SecretStore;
use wealthfolio_core::Result;
use wealthfolio_storage_sqlite::{
    activities::ActivityRepository,
    assets::AssetRepository,
    db,
    market_data::{MarketDataRepository, QuoteSyncStateRepository},
    schema::quotes,
};

struct NoSecrets;

impl SecretStore for NoSecrets {
    fn get_secret(&self, _: &str) -> Result<Option<String>> {
        panic!("Local asset deletion must not access credentials")
    }
    fn set_secret(&self, _: &str, _: &str) -> Result<()> {
        panic!("Local asset deletion must not access credentials")
    }
    fn delete_secret(&self, _: &str) -> Result<()> {
        panic!("Local asset deletion must not access credentials")
    }
}

struct TestDb {
    _directory: tempfile::TempDir,
    assets: Arc<AssetRepository>,
    states: Arc<QuoteSyncStateRepository>,
    pool: Arc<db::DbPool>,
    writer: db::WriteHandle,
}

impl TestDb {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let access = db::DbAccess::plaintext(directory.path().join("app.db").to_str().unwrap());
        access.prepare().unwrap();
        access.run_migrations().unwrap();
        let pool = access.create_pool().unwrap();
        let (writer, _task) = db::write_actor::spawn_writer_with_sync_state(
            (*pool).clone(),
            Arc::new(|| {}),
            Arc::default(),
        )
        .unwrap();
        Self {
            _directory: directory,
            assets: Arc::new(AssetRepository::new(pool.clone(), writer.clone())),
            states: Arc::new(QuoteSyncStateRepository::new(pool.clone(), writer.clone())),
            pool,
            writer,
        }
    }

    async fn services(&self) -> (AssetService, Arc<dyn QuoteServiceTrait>) {
        diesel::sql_query("UPDATE market_data_providers SET enabled=0")
            .execute(&mut db::get_connection(&self.pool).unwrap())
            .unwrap();
        let market_data = Arc::new(MarketDataRepository::new(
            self.pool.clone(),
            self.writer.clone(),
        ));
        let quotes = QuoteService::new(
            market_data.clone(),
            self.states.clone(),
            market_data,
            self.assets.clone(),
            Arc::new(ActivityRepository::new(
                self.pool.clone(),
                self.writer.clone(),
            )),
            Arc::new(NoSecrets),
        )
        .await
        .unwrap();
        let quotes: Arc<dyn QuoteServiceTrait> = Arc::new(quotes);
        (
            AssetService::new(self.assets.clone(), quotes.clone()).unwrap(),
            quotes,
        )
    }

    async fn create_asset(&self, id: &str) {
        self.assets
            .create(NewAsset {
                id: Some(id.into()),
                name: Some("New asset".into()),
                kind: AssetKind::Investment,
                instrument_type: Some(InstrumentType::Equity),
                instrument_symbol: Some(id.into()),
                is_active: true,
                quote_mode: QuoteMode::Market,
                quote_ccy: "USD".into(),
                ..Default::default()
            })
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn profile_completion_creates_state_before_first_quote_sync() {
    let db = TestDb::new();
    db.create_asset("profile-first").await;
    assert!(db
        .states
        .get_by_asset_id("profile-first")
        .unwrap()
        .is_none());

    db.states
        .mark_profile_enriched("profile-first")
        .await
        .unwrap();

    let state = db.states.get_by_asset_id("profile-first").unwrap().unwrap();
    let defaults = QuoteSyncState::new("profile-first".into(), String::new());
    assert!(!state.needs_profile_enrichment());
    assert!(state.profile_enriched_at.is_some());
    assert_eq!(state.asset_id, defaults.asset_id);
    assert_eq!(state.data_source, defaults.data_source);
    assert_eq!(state.sync_priority, defaults.sync_priority);
    assert_eq!(state.last_synced_at, None);
    assert_eq!(state.position_closed_date, None);
    assert_eq!(state.error_count, 0);
    assert_eq!(state.last_error, None);
    assert!(db
        .states
        .get_assets_needing_profile_enrichment()
        .unwrap()
        .is_empty());

    // A repeated completion updates the same row, not a second record.
    db.states
        .mark_profile_enriched("profile-first")
        .await
        .unwrap();
    assert_eq!(db.states.get_all().unwrap().len(), 1);
}

#[tokio::test]
async fn profile_completion_preserves_existing_quote_sync_state() {
    let db = TestDb::new();
    db.create_asset("existing-state").await;
    let original_time = DateTime::parse_from_rfc3339("2020-01-02T03:04:05Z")
        .unwrap()
        .with_timezone(&Utc);
    let mut original = QuoteSyncState::new("existing-state".into(), "CUSTOM_PROVIDER".into());
    original.position_closed_date = NaiveDate::from_ymd_opt(2021, 2, 3);
    original.is_active = false;
    original.last_synced_at = Some(original_time);
    original.sync_priority = 37;
    original.error_count = 4;
    original.last_error = Some("Preserve quote failure".into());
    original.profile_enriched_at = Some(original_time);
    original.created_at = original_time;
    original.updated_at = original_time;
    db.states.upsert(&original).await.unwrap();

    db.states
        .mark_profile_enriched("existing-state")
        .await
        .unwrap();

    let updated = db
        .states
        .get_by_asset_id("existing-state")
        .unwrap()
        .unwrap();
    assert!(updated.profile_enriched_at.unwrap() > original_time);
    assert!(updated.updated_at > original_time);
    let mut expected = serde_json::to_value(original).unwrap();
    let mut actual = serde_json::to_value(updated).unwrap();
    for field in ["profileEnrichedAt", "updatedAt"] {
        expected.as_object_mut().unwrap().remove(field);
        actual.as_object_mut().unwrap().remove(field);
    }
    assert_eq!(
        actual, expected,
        "profile completion must not reset quote state"
    );
}

#[tokio::test]
async fn profile_completion_rejects_missing_asset_without_an_orphan_state() {
    let db = TestDb::new();

    assert!(db
        .states
        .mark_profile_enriched("missing-asset")
        .await
        .is_err());
    assert!(db
        .states
        .get_by_asset_id("missing-asset")
        .unwrap()
        .is_none());
    assert!(db.assets.get_by_id("missing-asset").is_err());
}

#[tokio::test]
async fn asset_deletion_removes_state_recreated_after_earlier_cleanup() {
    let db = TestDb::new();
    db.create_asset("delete-race").await;
    db.states
        .mark_profile_enriched("delete-race")
        .await
        .unwrap();

    // Reproduce the former two-transaction service deletion window: initial
    // cleanup, then an in-flight enrichment completion, then asset deletion.
    db.states.delete("delete-race").await.unwrap();
    db.states
        .mark_profile_enriched("delete-race")
        .await
        .unwrap();
    assert!(db.states.get_by_asset_id("delete-race").unwrap().is_some());
    db.assets.delete("delete-race").await.unwrap();

    assert!(db.assets.get_by_id("delete-race").is_err());
    assert!(db.states.get_by_asset_id("delete-race").unwrap().is_none());
    assert!(db
        .states
        .mark_profile_enriched("delete-race")
        .await
        .is_err());
    assert!(db.states.get_by_asset_id("delete-race").unwrap().is_none());
}

#[tokio::test]
async fn rejected_asset_deletion_preserves_asset_quotes_and_profile_state() {
    let db = TestDb::new();
    db.create_asset("held-asset").await;
    db.states.mark_profile_enriched("held-asset").await.unwrap();
    let before_asset = serde_json::to_value(db.assets.get_by_id("held-asset").unwrap()).unwrap();
    let before_state =
        serde_json::to_value(db.states.get_by_asset_id("held-asset").unwrap().unwrap()).unwrap();
    let mut conn = db::get_connection(&db.pool).unwrap();
    diesel::sql_query("INSERT INTO accounts (id,name,currency) VALUES ('account','Account','USD')")
        .execute(&mut conn)
        .unwrap();
    diesel::sql_query(
        "INSERT INTO activities (id,account_id,asset_id,activity_type,activity_date,quantity,unit_price,currency,created_at,updated_at) \
         VALUES ('buy','account','held-asset','BUY','2026-01-02T00:00:00Z','1','10','USD',CURRENT_TIMESTAMP,CURRENT_TIMESTAMP)",
    )
    .execute(&mut conn)
    .unwrap();
    diesel::sql_query(
        "INSERT INTO quotes (id,asset_id,day,source,close,currency,created_at,timestamp) \
         VALUES ('saved-quote','held-asset','2026-01-02','MANUAL','10','USD',CURRENT_TIMESTAMP,'2026-01-02T00:00:00Z')",
    )
    .execute(&mut conn)
    .unwrap();
    drop(conn);

    // Exercise the real service too: it must not clear state before the
    // repository can reject deletion because this asset has an activity.
    let (service, _) = db.services().await;
    assert!(service.delete_asset("held-asset").await.is_err());
    assert_eq!(
        serde_json::to_value(db.assets.get_by_id("held-asset").unwrap()).unwrap(),
        before_asset,
    );
    assert_eq!(
        serde_json::to_value(db.states.get_by_asset_id("held-asset").unwrap().unwrap()).unwrap(),
        before_state,
    );
    let saved_quotes = quotes::table
        .filter(quotes::asset_id.eq("held-asset"))
        .select((quotes::id, quotes::close, quotes::source))
        .load::<(String, String, String)>(&mut db::get_connection(&db.pool).unwrap())
        .unwrap();
    assert_eq!(
        saved_quotes,
        vec![("saved-quote".into(), "10".into(), "MANUAL".into())]
    );
}

#[tokio::test]
async fn profile_completion_survives_sync_refresh_and_skips_repeat_enrichment() {
    let db = TestDb::new();
    db.create_asset("completed-profile").await;
    let (assets, quotes) = db.services().await;
    // This is the completion write performed after successful profile fetching.
    // Provider response handling is covered by the core service and bond E2E;
    // this test exercises real SQLite state and real sync-state refresh.
    quotes
        .mark_profile_enriched("completed-profile")
        .await
        .unwrap();
    quotes.refresh_sync_state().await.unwrap();

    assert!(!quotes
        .get_assets_needing_profile_enrichment()
        .unwrap()
        .iter()
        .any(|state| state.asset_id == "completed-profile"));
    // All providers are disabled. Skipping instead of failing proves the saved
    // completion is observed by the real enrichment service before fetching.
    assert_eq!(
        assets
            .enrich_assets(vec!["completed-profile".into()])
            .await
            .unwrap(),
        (0, 1, 0),
    );
}

#[tokio::test]
async fn failed_initial_enrichment_remains_pending_after_sync_refresh() {
    let db = TestDb::new();
    db.create_asset("failed-profile").await;
    let (assets, quotes) = db.services().await;
    // No enabled provider can supply a profile: this runs the actual failure
    // path without network or a fake success marker.
    assert_eq!(
        assets
            .enrich_assets(vec!["failed-profile".into()])
            .await
            .unwrap(),
        (0, 0, 1),
    );
    assert!(quotes.get_sync_state("failed-profile").unwrap().is_none());
    quotes.refresh_sync_state().await.unwrap();
    let state = quotes.get_sync_state("failed-profile").unwrap().unwrap();
    assert!(state.needs_profile_enrichment());
    assert!(quotes
        .get_assets_needing_profile_enrichment()
        .unwrap()
        .iter()
        .any(|state| state.asset_id == "failed-profile"));
}
