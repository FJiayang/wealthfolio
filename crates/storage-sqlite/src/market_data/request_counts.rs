//! Profile-local provider attempt events. Writes reuse the profile writer and
//! deliberately do not project sync/outbox changes.
use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, NaiveDateTime, SecondsFormat, Utc};
use diesel::{prelude::*, sql_types::Text};
use wealthfolio_core::{
    errors::{Error, Result},
    quotes::request_counts::ProviderRequestRepository,
};

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
struct MarkerRow {
    #[diesel(sql_type = Text)]
    run_on: String,
}

fn timestamp(time: DateTime<Utc>) -> String {
    time.to_rfc3339_opts(SecondsFormat::Nanos, true)
}

#[async_trait]
impl ProviderRequestRepository for ProviderRequestSqliteRepository {
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
