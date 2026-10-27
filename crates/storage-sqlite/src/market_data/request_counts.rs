//! Profile-local provider attempt events. Writes reuse the profile writer and
//! deliberately do not project sync/outbox changes.
use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, NaiveDateTime, SecondsFormat, Utc};
use diesel::{
    prelude::*,
    sql_types::{BigInt, Nullable, Text},
};
use wealthfolio_core::{
    errors::{Error, Result},
    quotes::request_counts::{
        parse_request_cursor, ProviderRequestRepository, RequestEvent, RequestQuery,
        REQUEST_RETENTION_DAYS,
    },
};

use wealthfolio_market_data::{RequestContext, RequestOutcome};

use crate::{
    db::{get_connection, DbPool, WriteHandle},
    errors::IntoCore,
};

const TRACKING_MIGRATION: &str = "20261024000001";

pub struct ProviderRequestSqliteRepository {
    pool: Arc<DbPool>,
    writer: WriteHandle,
}

impl ProviderRequestSqliteRepository {
    pub fn new(pool: Arc<DbPool>, writer: WriteHandle) -> Self {
        Self { pool, writer }
    }
}

#[derive(QueryableByName)]
struct EventRow {
    #[diesel(sql_type = Text)]
    provider_id: String,
    #[diesel(sql_type = Text)]
    occurred_at: String,
}

#[derive(QueryableByName)]
struct DiagnosticRow {
    #[diesel(sql_type = BigInt)]
    id: i64,
    #[diesel(sql_type = Text)]
    provider_id: String,
    #[diesel(sql_type = Text)]
    occurred_at: String,
    #[diesel(sql_type = Nullable<Text>)]
    context: Option<String>,
    #[diesel(sql_type = Nullable<Text>)]
    result: Option<String>,
}

impl DiagnosticRow {
    fn into_event(self) -> Result<RequestEvent> {
        Ok(RequestEvent {
            id: self.id,
            provider_id: self.provider_id,
            occurred_at: DateTime::parse_from_rfc3339(&self.occurred_at)
                .map_err(|_| Error::Repository("Invalid request timestamp".into()))?
                .with_timezone(&Utc),
            context: self
                .context
                .map(|value| serde_json::from_str(&value))
                .transpose()
                .map_err(|_| Error::Repository("Invalid request context".into()))?,
            result: self
                .result
                .map(|value| serde_json::from_str(&value))
                .transpose()
                .map_err(|_| Error::Repository("Invalid request result".into()))?,
        })
    }
}

#[derive(QueryableByName)]
struct InsertedId {
    #[diesel(sql_type = BigInt)]
    id: i64,
}

#[derive(QueryableByName)]
struct MarkerRow {
    #[diesel(sql_type = Text)]
    run_on: String,
}

fn timestamp(time: DateTime<Utc>) -> String {
    time.to_rfc3339_opts(SecondsFormat::Nanos, true)
}

#[async_trait]
impl ProviderRequestRepository for ProviderRequestSqliteRepository {
    async fn start_event(
        &self,
        provider: &str,
        at: DateTime<Utc>,
        context: &RequestContext,
    ) -> Result<Option<i64>> {
        let provider = provider.to_owned();
        let at = timestamp(at);
        let context = serde_json::to_string(context)
            .map_err(|_| Error::Unexpected("Invalid request context".into()))?;
        let cutoff = timestamp(Utc::now() - chrono::Duration::days(REQUEST_RETENTION_DAYS));
        self.writer.exec_tx(move |tx| {
            diesel::sql_query("DELETE FROM provider_request_events WHERE occurred_at < ?")
                .bind::<Text, _>(cutoff).execute(tx.conn()).into_core()?;
            diesel::sql_query("INSERT INTO provider_request_events(provider_id, occurred_at, context) VALUES (?, ?, ?)")
                .bind::<Text, _>(provider).bind::<Text, _>(at).bind::<Text, _>(context)
                .execute(tx.conn()).into_core()?;
            let row = diesel::sql_query("SELECT last_insert_rowid() AS id").get_result::<InsertedId>(tx.conn()).into_core()?;
            Ok(Some(row.id))
        }).await
    }

    async fn finish_event(&self, id: i64, outcome: &RequestOutcome) -> Result<()> {
        let result = serde_json::to_string(outcome)
            .map_err(|_| Error::Unexpected("Invalid request outcome".into()))?;
        self.writer
            .exec_tx(move |tx| {
                diesel::sql_query(
                    "UPDATE provider_request_events SET result = ? WHERE id = ? AND result IS NULL",
                )
                .bind::<Text, _>(result)
                .bind::<BigInt, _>(id)
                .execute(tx.conn())
                .into_core()?;
                Ok(())
            })
            .await
    }

    async fn prune(&self, before: DateTime<Utc>) -> Result<()> {
        let before = timestamp(before);
        self.writer
            .exec_tx(move |tx| {
                diesel::sql_query("DELETE FROM provider_request_events WHERE occurred_at < ?")
                    .bind::<Text, _>(before)
                    .execute(tx.conn())
                    .into_core()?;
                Ok(())
            })
            .await
    }

    fn diagnostics_since(&self) -> Result<DateTime<Utc>> {
        let mut conn = get_connection(&self.pool)?;
        let marker = diesel::sql_query("SELECT CAST(run_on AS TEXT) AS run_on FROM __diesel_schema_migrations WHERE version = '20261027000001'")
            .get_result::<MarkerRow>(&mut conn).into_core()?;
        NaiveDateTime::parse_from_str(&marker.run_on, "%Y-%m-%d %H:%M:%S%.f")
            .map(|time| time.and_utc())
            .map_err(|_| Error::Repository("Invalid diagnostics start".into()))
    }

    fn list_events(
        &self,
        query: &RequestQuery,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Vec<RequestEvent>> {
        let mut conn = get_connection(&self.pool)?;
        let cursor = query
            .cursor
            .as_deref()
            .map(parse_request_cursor)
            .transpose()?;
        let cursor_time = cursor.map(|(time, _)| timestamp(time));
        let cursor_id = cursor.map(|(_, id)| id).unwrap_or(i64::MAX);
        let rows = diesel::sql_query(
            "SELECT id, provider_id, occurred_at, context, result FROM provider_request_events
             WHERE occurred_at >= ? AND occurred_at < ?
             AND (? IS NULL OR provider_id = ?)
             AND (? IS NULL OR EXISTS (SELECT 1 FROM json_each(json_extract(context, '$.symbols')) WHERE value = ?))
             AND (? IS NULL OR COALESCE(json_extract(context, '$.recordKind'), 'http') = ?)
             AND (? IS NULL OR COALESCE(json_extract(result, '$.outcome'), 'unknown') = ?)
             AND (? IS NULL OR occurred_at < ? OR (occurred_at = ? AND id < ?))
             ORDER BY occurred_at DESC, id DESC LIMIT ?")
            .bind::<Text, _>(timestamp(start)).bind::<Text, _>(timestamp(end))
            .bind::<Nullable<Text>, _>(&query.provider_id).bind::<Nullable<Text>, _>(&query.provider_id)
            .bind::<Nullable<Text>, _>(&query.symbol).bind::<Nullable<Text>, _>(&query.symbol)
            .bind::<Nullable<Text>, _>(&query.record_kind).bind::<Nullable<Text>, _>(&query.record_kind)
            .bind::<Nullable<Text>, _>(&query.outcome).bind::<Nullable<Text>, _>(&query.outcome)
            .bind::<Nullable<Text>, _>(&cursor_time).bind::<Nullable<Text>, _>(&cursor_time).bind::<Nullable<Text>, _>(&cursor_time)
            .bind::<BigInt, _>(cursor_id).bind::<BigInt, _>(i64::from(query.limit.unwrap_or(50)) + 1)
            .load::<DiagnosticRow>(&mut conn).into_core()?;
        rows.into_iter().map(DiagnosticRow::into_event).collect()
    }

    fn visit_diagnostics(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        visitor: &mut dyn FnMut(RequestEvent) -> Result<()>,
    ) -> Result<DateTime<Utc>> {
        let mut conn = get_connection(&self.pool)?;
        let events = diesel::sql_query("SELECT id, provider_id, occurred_at, context, result FROM provider_request_events WHERE occurred_at >= ? AND occurred_at < ?")
            .bind::<Text, _>(timestamp(start)).bind::<Text, _>(timestamp(end));
        for row in events
            .load_iter::<DiagnosticRow, diesel::connection::DefaultLoadingMode>(&mut conn)
            .into_core()?
        {
            visitor(row.into_core()?.into_event()?)?;
        }
        // Keep the existing migration marker validation for historical statistics.
        self.visit_events(end, end, &mut |_, _| Ok(()))
    }

    async fn insert_event(&self, provider_id: &str, occurred_at: DateTime<Utc>) -> Result<()> {
        let provider_id = provider_id.to_owned();
        let occurred_at = timestamp(occurred_at);
        self.writer
            .exec_tx(move |tx| {
                diesel::sql_query(
                    "INSERT INTO provider_request_events(provider_id, occurred_at) VALUES (?, ?)",
                )
                .bind::<Text, _>(provider_id)
                .bind::<Text, _>(occurred_at)
                .execute(tx.conn())
                .into_core()?;
                Ok(())
            })
            .await
    }

    fn visit_events(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        visitor: &mut dyn FnMut(&str, DateTime<Utc>) -> Result<()>,
    ) -> Result<DateTime<Utc>> {
        let mut conn = get_connection(&self.pool)?;
        let marker = diesel::sql_query("SELECT CAST(run_on AS TEXT) AS run_on FROM __diesel_schema_migrations WHERE version = ?")
            .bind::<Text, _>(TRACKING_MIGRATION)
            .get_result::<MarkerRow>(&mut conn).optional().into_core()?
            .ok_or_else(|| Error::Repository("Provider request tracking start is unknown".into()))?;
        // Diesel's migration table uses SQLite CURRENT_TIMESTAMP (UTC).
        let tracking_since = NaiveDateTime::parse_from_str(&marker.run_on, "%Y-%m-%d %H:%M:%S%.f")
            .map(|time| time.and_utc())
            .map_err(|_| Error::Repository("Invalid provider request tracking start".into()))?;
        let query = diesel::sql_query("SELECT provider_id, occurred_at FROM provider_request_events WHERE occurred_at >= ? AND occurred_at < ?")
            .bind::<Text, _>(timestamp(start))
            .bind::<Text, _>(timestamp(end));
        let events = query
            .load_iter::<EventRow, diesel::connection::DefaultLoadingMode>(&mut conn)
            .into_core()?;
        for event in events {
            let event = event.into_core()?;
            let occurred_at = DateTime::parse_from_rfc3339(&event.occurred_at)
                .map_err(|_| Error::Repository("Invalid provider request event timestamp".into()))?
                .with_timezone(&Utc);
            visitor(&event.provider_id, occurred_at)?;
        }
        Ok(tracking_since)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{bootstrap, write_actor::spawn_writer, EncryptionPolicy, NoKeyProvider};
    use diesel::connection::SimpleConnection;

    fn utc(value: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(value)
            .unwrap()
            .with_timezone(&Utc)
    }
    fn open(path: &str) -> (Arc<DbPool>, WriteHandle) {
        let access = bootstrap(path, &NoKeyProvider, EncryptionPolicy::Plaintext).unwrap();
        access.run_migrations().unwrap();
        let pool = access.create_pool().unwrap();
        let writer = spawn_writer(pool.as_ref().clone()).unwrap();
        (pool, writer)
    }
    fn count(repository: &ProviderRequestSqliteRepository) -> (usize, DateTime<Utc>) {
        let mut count = 0;
        let marker = repository
            .visit_events(
                utc("2000-01-01T00:00:00Z"),
                utc("2099-01-01T00:00:00Z"),
                &mut |_, _| {
                    count += 1;
                    Ok(())
                },
            )
            .unwrap();
        (count, marker)
    }

    #[tokio::test]
    async fn diagnostics_migration_completion_filters_cursor_retention_and_counts() {
        use wealthfolio_core::quotes::request_counts::ProviderRequestService;
        let directory = tempfile::tempdir().unwrap();
        let (pool, writer) = open(directory.path().join("diagnostics.db").to_str().unwrap());
        let repository = Arc::new(ProviderRequestSqliteRepository::new(
            pool.clone(),
            writer.clone(),
        ));
        let at = Utc::now() - chrono::Duration::seconds(2);
        repository.insert_event("LEGACY", at).await.unwrap();
        {
            let mut connection = get_connection(&pool).unwrap();
            connection
                .batch_execute(include_str!(
                    "../../migrations/2026-10-27-000001_provider_request_diagnostics/down.sql"
                ))
                .unwrap();
            connection
                .batch_execute(include_str!(
                    "../../migrations/2026-10-27-000001_provider_request_diagnostics/up.sql"
                ))
                .unwrap();
        }
        let context = RequestContext::http(
            "history",
            vec!["ABC".into(), "XYZ".into()],
            serde_json::json!({"symbol":"ABC","from":"2026-10-01"}),
        );
        let first = repository
            .start_event("TEST", at, &context)
            .await
            .unwrap()
            .unwrap();
        let second = repository
            .start_event("TEST", at, &context)
            .await
            .unwrap()
            .unwrap();
        assert_ne!(first, second);
        let outcome = RequestOutcome::completed(
            std::time::Instant::now(),
            Some(("timeout", "Request timed out")),
            None,
        );
        repository.finish_event(first, &outcome).await.unwrap();
        repository
            .finish_event(
                first,
                &RequestOutcome::completed(std::time::Instant::now(), None, Some(200)),
            )
            .await
            .unwrap();
        let service = ProviderRequestService::new(repository.clone());
        let mut query = RequestQuery {
            provider_id: Some("TEST".into()),
            symbol: Some("XYZ".into()),
            limit: Some(1),
            ..Default::default()
        };
        let page = service.list_requests(query.clone()).await.unwrap();
        assert_eq!(page.rows.len(), 1);
        assert_eq!(page.rows[0].id, second);
        assert!(page.rows[0].result.is_none());
        query.cursor = page.next_cursor;
        let page = service.list_requests(query.clone()).await.unwrap();
        assert_eq!(page.rows[0].id, first);
        assert_eq!(page.rows[0].result.as_ref().unwrap().outcome, "failure");
        assert!(page.next_cursor.is_none());
        query.cursor = None;
        query.outcome = Some("failure".into());
        assert_eq!(
            service.list_requests(query.clone()).await.unwrap().rows[0].id,
            first
        );
        query.symbol = Some("AB".into());
        assert!(service.list_requests(query).await.unwrap().rows.is_empty());
        let counts = service.get_counts(7, "UTC").await.unwrap();
        let row = counts
            .rows
            .iter()
            .find(|row| row.provider_id == "TEST")
            .unwrap();
        assert_eq!(
            (row.request_count, row.failure_count, row.unknown_count),
            (2, 1, 1)
        );
        let legacy = service
            .list_requests(RequestQuery {
                provider_id: Some("LEGACY".into()),
                ..Default::default()
            })
            .await
            .unwrap();
        assert!(legacy.rows[0].context.is_none());
        assert!(legacy.rows[0].result.is_none());
        let mut sdk_context = context.clone();
        sdk_context.record_kind = "sdk".into();
        let sdk_id = repository
            .start_event("TEST", at, &sdk_context)
            .await
            .unwrap()
            .unwrap();
        repository
            .finish_event(
                sdk_id,
                &RequestOutcome::completed(std::time::Instant::now(), None, None),
            )
            .await
            .unwrap();
        let sdk = service
            .list_requests(RequestQuery {
                record_kind: Some("sdk".into()),
                outcome: Some("success".into()),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(sdk.rows.len(), 1);
        assert_eq!(sdk.rows[0].id, sdk_id);
        let counts = service.get_counts(7, "UTC").await.unwrap();
        let sdk = counts
            .rows
            .iter()
            .find(|row| row.record_kind == "sdk")
            .unwrap();
        assert_eq!((sdk.request_count, sdk.success_count), (1, 1));
        let cutoff = Utc::now() - chrono::Duration::days(30);
        repository
            .insert_event("OLD", cutoff - chrono::Duration::seconds(1))
            .await
            .unwrap();
        repository.insert_event("BOUNDARY", cutoff).await.unwrap();
        repository.prune(cutoff).await.unwrap();
        let remaining = repository
            .list_events(
                &RequestQuery::default(),
                cutoff - chrono::Duration::days(1),
                Utc::now(),
            )
            .unwrap();
        assert!(!remaining.iter().any(|row| row.provider_id == "OLD"));
        assert!(remaining.iter().any(|row| row.provider_id == "BOUNDARY"));
        writer.shutdown().await;
        assert!(repository.finish_event(first, &outcome).await.is_err());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn diagnostic_completion_preserves_concurrent_request_identity() {
        let directory = tempfile::tempdir().unwrap();
        let (pool, writer) = open(directory.path().join("concurrent.db").to_str().unwrap());
        let repository = Arc::new(ProviderRequestSqliteRepository::new(pool, writer.clone()));
        let at = Utc::now() - chrono::Duration::seconds(1);
        let mut tasks = Vec::new();
        for index in 0..16_u64 {
            let repository = repository.clone();
            tasks.push(tokio::spawn(async move {
                let context = RequestContext::http(
                    "quote",
                    vec![index.to_string()],
                    serde_json::json!({"index":index}),
                );
                let id = repository
                    .start_event("TEST", at, &context)
                    .await
                    .unwrap()
                    .unwrap();
                let outcome = RequestOutcome {
                    duration_ms: index,
                    outcome: "success".into(),
                    http_status: Some(200),
                    error_category: None,
                    error_message: None,
                };
                repository.finish_event(id, &outcome).await.unwrap();
            }));
        }
        for task in tasks {
            task.await.unwrap();
        }
        let rows = repository
            .list_events(&RequestQuery::default(), at, Utc::now())
            .unwrap();
        assert_eq!(rows.len(), 16);
        for row in rows {
            let symbol = &row.context.unwrap().symbols[0];
            assert_eq!(
                symbol.parse::<u64>().unwrap(),
                row.result.unwrap().duration_ms
            );
        }
        writer.shutdown().await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn request_counts_atomic_concurrent_restart_and_profile_isolation() {
        let directory = tempfile::tempdir().unwrap();
        let first = directory.path().join("first.db");
        let second = directory.path().join("second.db");
        let (pool, writer) = open(first.to_str().unwrap());
        let repository = Arc::new(ProviderRequestSqliteRepository::new(
            pool.clone(),
            writer.clone(),
        ));
        let marker = count(&repository).1;
        let mut tasks = Vec::new();
        for _ in 0..50 {
            let repository = repository.clone();
            tasks.push(tokio::spawn(async move {
                repository
                    .insert_event("CUSTOM_SCRAPER:test", utc("2026-10-24T00:00:00.123456789Z"))
                    .await
                    .unwrap();
            }));
        }
        for task in tasks {
            task.await.unwrap();
        }
        assert_eq!(count(&repository).0, 50);
        let (other_pool, other_writer) = open(second.to_str().unwrap());
        let other = ProviderRequestSqliteRepository::new(other_pool, other_writer.clone());
        assert_eq!(count(&other).0, 0);
        other_writer.shutdown().await;
        writer.shutdown().await;
        drop(repository);
        drop(pool);
        let (pool, writer) = open(first.to_str().unwrap());
        let repository = ProviderRequestSqliteRepository::new(pool.clone(), writer.clone());
        assert_eq!(count(&repository), (50, marker));
        let mut connection = get_connection(&pool).unwrap();
        let event = diesel::sql_query(
            "SELECT provider_id, occurred_at FROM provider_request_events LIMIT 1",
        )
        .get_result::<EventRow>(&mut connection)
        .unwrap();
        assert_eq!(event.occurred_at, "2026-10-24T00:00:00.123456789Z");
        assert_eq!(event.provider_id, "CUSTOM_SCRAPER:test");
        writer.shutdown().await;
        assert!(repository.insert_event("YAHOO", Utc::now()).await.is_err());
    }

    #[tokio::test]
    async fn request_counts_utc_range_is_half_open_at_nanosecond_precision() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("precision.db");
        let (pool, writer) = open(path.to_str().unwrap());
        let repository = ProviderRequestSqliteRepository::new(pool, writer.clone());
        for time in [
            "2026-10-23T15:59:59.999999999Z",
            "2026-10-23T16:00:00.000000000Z",
            "2026-10-23T16:00:00.000000001Z",
            "2026-10-24T16:00:00.000000000Z",
        ] {
            repository.insert_event("YAHOO", utc(time)).await.unwrap();
        }
        let mut times = Vec::new();
        repository
            .visit_events(
                utc("2026-10-23T16:00:00Z"),
                utc("2026-10-24T16:00:00Z"),
                &mut |_, time| {
                    times.push(time);
                    Ok(())
                },
            )
            .unwrap();
        times.sort();
        assert_eq!(
            times,
            vec![
                utc("2026-10-23T16:00:00Z"),
                utc("2026-10-23T16:00:00.000000001Z")
            ]
        );
        writer.shutdown().await;
    }

    #[tokio::test]
    async fn request_counts_missing_marker_and_storage_failures_are_errors() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("test.db");
        let (pool, writer) = open(path.to_str().unwrap());
        let repository = ProviderRequestSqliteRepository::new(pool.clone(), writer.clone());
        let mut connection = get_connection(&pool).unwrap();
        connection
            .batch_execute("DELETE FROM __diesel_schema_migrations WHERE version='20261024000001'")
            .unwrap();
        assert!(repository
            .visit_events(utc("2000-01-01T00:00:00Z"), Utc::now(), &mut |_, _| Ok(()))
            .is_err());
        connection
            .batch_execute("DROP TABLE provider_request_events")
            .unwrap();
        assert!(repository.insert_event("YAHOO", Utc::now()).await.is_err());
        writer.shutdown().await;
    }
}
