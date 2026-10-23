use super::*;
use diesel::connection::SimpleConnection;
use diesel::RunQueryDsl;

const VERSION: &str = "20261023000001";
fn older_database() -> SqliteConnection {
    let mut conn = SqliteConnection::establish(":memory:").unwrap();
    let mut migrations = conn.pending_migrations(MIGRATIONS).unwrap();
    migrations.sort_by_key(|m| m.name().version().to_string());
    for migration in migrations {
        if migration.name().version().to_string().as_str() >= VERSION {
            break;
        }
        conn.run_migration(&*migration).unwrap();
    }
    conn
}
#[derive(diesel::QueryableByName)]
struct TextRow {
    #[diesel(sql_type = diesel::sql_types::Text)]
    value: String,
}
fn scalar(conn: &mut SqliteConnection, sql: &str) -> String {
    diesel::sql_query(sql)
        .get_result::<TextRow>(conn)
        .unwrap()
        .value
}
fn seed(conn: &mut SqliteConnection, enabled: bool) {
    conn.batch_execute(&format!(
        "UPDATE market_data_providers SET enabled={} WHERE id='HTTP_API';",
        i32::from(enabled)
    ))
    .unwrap();
    conn.batch_execute(r#"
        INSERT INTO assets(id,kind,name,display_code,quote_mode,quote_ccy,instrument_type,instrument_symbol,provider_config)
        VALUES ('fund','INVESTMENT','My fund','FUND:005827','MARKET','CNY','EQUITY','FUND:005827',
                '{"preferred_provider":"HTTP_API","overrides":{"HTTP_API":{"type":"equity_symbol","symbol":"FUND:005827"}},"keep":42}'),
               ('manual','INVESTMENT','Manual','FUND:000001','MANUAL','CNY','EQUITY','FUND:000001',
                '{"preferred_provider":"HTTP_API"}'),
               ('other','INVESTMENT','Apple','AAPL','MARKET','USD','EQUITY','AAPL',
                '{"preferred_provider":"YAHOO","overrides":{"HTTP_API":{"type":"equity_symbol","symbol":"XNAS:AAPL"},"YAHOO":{"type":"equity_symbol","symbol":"AAPL"}}}');
        INSERT INTO quote_sync_state(asset_id,data_source,error_count,last_error,last_synced_at)
        VALUES ('fund','HTTP_API',15,'service down','2026-10-20T00:00:00Z'),('other','YAHOO',0,NULL,NULL);
        INSERT INTO accounts(id,name,currency) VALUES ('account','Migration test','CNY');
        INSERT INTO activities(id,account_id,asset_id,activity_type,activity_date,quantity,unit_price,currency,created_at,updated_at)
        VALUES ('buy','account','fund','BUY','2026-10-20','10','1.23','CNY','2026-10-20','2026-10-20');
        INSERT INTO quotes(id,asset_id,day,source,close,currency,created_at,timestamp)
        VALUES ('old','fund','2026-10-20','HTTP_API','1.23','CNY','2026-10-20T00:00:00Z','2026-10-20T00:00:00Z');
    "#).unwrap();
}
#[test]
fn native_migration_preserves_identity_prices_and_user_modes() {
    for enabled in [true, false] {
        let mut conn = older_database();
        seed(&mut conn, enabled);
        let order = scalar(&mut conn,"SELECT group_concat(id,',') AS value FROM (SELECT id FROM market_data_providers ORDER BY priority,id)");
        let identity=scalar(&mut conn,"SELECT json_group_array(json_array(id,name,display_code,quote_mode,quote_ccy,instrument_symbol)) AS value FROM assets ORDER BY id");
        let activity = scalar(&mut conn,"SELECT json_group_array(json_array(id,account_id,asset_id,quantity,unit_price)) AS value FROM activities");
        conn.run_pending_migrations(MIGRATIONS).unwrap();
        assert_eq!(activity, scalar(&mut conn,"SELECT json_group_array(json_array(id,account_id,asset_id,quantity,unit_price)) AS value FROM activities"));
        assert_eq!(scalar(&mut conn,"SELECT group_concat(id,',') AS value FROM (SELECT id FROM market_data_providers ORDER BY priority,id)"), order.replace("HTTP_API", "EASTMONEY,TENCENT,SINA"));
        assert_eq!(identity,scalar(&mut conn,"SELECT json_group_array(json_array(id,name,display_code,quote_mode,quote_ccy,instrument_symbol)) AS value FROM assets ORDER BY id"));
        assert_eq!(
            scalar(
                &mut conn,
                "SELECT provider_config AS value FROM assets WHERE id='fund'"
            ),
            r#"{"keep":42}"#
        );
        assert_eq!(
            scalar(
                &mut conn,
                "SELECT COALESCE(provider_config,'NULL') AS value FROM assets WHERE id='manual'"
            ),
            "NULL"
        );
        assert_eq!(scalar(&mut conn,"SELECT json_extract(provider_config,'$.preferred_provider') AS value FROM assets WHERE id='other'"),"YAHOO");
        assert_eq!(scalar(&mut conn,"SELECT CAST(error_count AS TEXT)||':'||data_source||':'||last_synced_at AS value FROM quote_sync_state WHERE asset_id='fund'"),"0::2026-10-20T00:00:00Z");
        assert_eq!(
            scalar(
                &mut conn,
                "SELECT source||':'||close AS value FROM quotes WHERE id='old'"
            ),
            "HTTP_API:1.23"
        );
        assert_eq!(scalar(&mut conn,"SELECT CAST(SUM(enabled) AS TEXT) AS value FROM market_data_providers WHERE id IN ('EASTMONEY','TENCENT','SINA')"),if enabled {"3"}else{"0"});
        assert_eq!(scalar(&mut conn,"SELECT group_concat(id,',') AS value FROM (SELECT id FROM market_data_providers WHERE id IN ('EASTMONEY','TENCENT','SINA') ORDER BY priority)"),"EASTMONEY,TENCENT,SINA");
        conn.batch_execute(
            "UPDATE market_data_providers SET enabled=0,priority=99 WHERE id='EASTMONEY'",
        )
        .unwrap();
        assert!(conn.run_pending_migrations(MIGRATIONS).unwrap().is_empty());
        assert_eq!(scalar(&mut conn,"SELECT CAST(priority AS TEXT) AS value FROM market_data_providers WHERE id='EASTMONEY'"),"99");
    }
}
#[test]
fn native_migration_is_atomic_on_failure() {
    let mut conn = older_database();
    seed(&mut conn, true);
    conn.batch_execute("CREATE TRIGGER fail_native_migration BEFORE DELETE ON market_data_providers WHEN OLD.id='HTTP_API' BEGIN SELECT RAISE(ABORT,'test failure'); END;").unwrap();
    assert!(conn.run_pending_migrations(MIGRATIONS).is_err());
    assert_eq!(scalar(&mut conn,"SELECT json_extract(provider_config,'$.preferred_provider') AS value FROM assets WHERE id='fund'"),"HTTP_API");
    assert_eq!(
        scalar(
            &mut conn,
            "SELECT data_source AS value FROM quote_sync_state WHERE asset_id='fund'"
        ),
        "HTTP_API"
    );
    assert_eq!(scalar(&mut conn,"SELECT CAST(COUNT(*) AS TEXT) AS value FROM market_data_providers WHERE id='EASTMONEY'"),"0");
    conn.batch_execute("DROP TRIGGER fail_native_migration")
        .unwrap();
    conn.run_pending_migrations(MIGRATIONS).unwrap();
}

#[test]
fn native_migration_without_http_settings_defaults_to_disabled() {
    let mut conn = older_database();
    conn.batch_execute("DELETE FROM market_data_providers WHERE id='HTTP_API'")
        .unwrap();
    conn.run_pending_migrations(MIGRATIONS).unwrap();
    assert_eq!(scalar(&mut conn,"SELECT CAST(SUM(enabled) AS TEXT) AS value FROM market_data_providers WHERE id IN ('EASTMONEY','TENCENT','SINA')"), "0");
}
