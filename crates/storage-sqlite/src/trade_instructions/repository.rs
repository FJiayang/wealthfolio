use super::model::InstructionDb;
use crate::{
    db::{get_connection, DbPool, WriteHandle},
    errors::StorageError,
    schema::trade_instructions::dsl::*,
};
use async_trait::async_trait;
use diesel::prelude::*;
use std::sync::Arc;
use wealthfolio_core::{trade_instructions::*, Error, Result};

pub struct TradeInstructionRepository {
    pool: Arc<DbPool>,
    writer: WriteHandle,
}
impl TradeInstructionRepository {
    pub fn new(pool: Arc<DbPool>, writer: WriteHandle) -> Self {
        Self { pool, writer }
    }
}
#[async_trait]
impl TradeInstructionRepositoryTrait for TradeInstructionRepository {
    fn get(&self, instruction_id: &str) -> Result<TradeInstruction> {
        let mut conn = get_connection(&self.pool)?;
        let row = trade_instructions
            .find(instruction_id)
            .select(InstructionDb::as_select())
            .first(&mut conn)
            .map_err(StorageError::from)?;
        TradeInstruction::try_from(row).map_err(Into::into)
    }
    fn get_by_request_id(&self, key: &str) -> Result<Option<TradeInstruction>> {
        let mut conn = get_connection(&self.pool)?;
        let row = trade_instructions
            .filter(request_id.eq(key))
            .select(InstructionDb::as_select())
            .first(&mut conn)
            .optional()
            .map_err(StorageError::from)?;
        row.map(TradeInstruction::try_from)
            .transpose()
            .map_err(Into::into)
    }
    fn list(&self, filter: InstructionFilter) -> Result<InstructionPage> {
        let mut conn = get_connection(&self.pool)?;
        let states = filter
            .statuses
            .unwrap_or_else(|| vec![InstructionStatus::Pending, InstructionStatus::InProgress]);
        let states: Vec<_> = states.iter().map(|s| s.as_str()).collect();
        let query = || {
            let mut q = trade_instructions.into_boxed();
            if let Some(ref goal) = filter.goal_id {
                q = q.filter(goal_id.eq(goal));
            }
            if !states.is_empty() {
                q = q.filter(status.eq_any(&states));
            }
            if let Some(ref account) = filter.account_id {
                q = q.filter(
                    account_id.eq(account).or(diesel::dsl::sql::<
                        diesel::sql_types::Nullable<diesel::sql_types::Text>,
                    >(
                        "json_extract(transfer, '$.toAccountId')"
                    )
                    .eq(account)),
                );
            }
            let directions: Vec<_> = filter
                .sides
                .clone()
                .unwrap_or_else(|| filter.side.into_iter().collect());
            if !directions.is_empty() {
                q = q.filter(side.eq_any(directions.into_iter().map(InstructionSide::as_str)));
            }
            if let Some(from) = filter.expected_completion_from {
                q = q.filter(expected_completion_at.ge(from.to_rfc3339()));
            }
            if let Some(before) = filter.expected_completion_before {
                q = q.filter(expected_completion_at.lt(before.to_rfc3339()));
            }
            q
        };
        let total = query()
            .count()
            .get_result(&mut conn)
            .map_err(StorageError::from)?;
        let sorted = match filter.sort {
            Some(InstructionSort::ExpectedCompletionAsc) => query().order((
                expected_completion_at.is_null().asc(),
                expected_completion_at.asc(),
                created_at.asc(),
                id.asc(),
            )),
            Some(InstructionSort::ExpectedCompletionDesc) => query().order((
                expected_completion_at.is_null().asc(),
                expected_completion_at.desc(),
                created_at.asc(),
                id.asc(),
            )),
            None => query().order((created_at.desc(), id.desc())),
        };
        let rows = sorted
            .limit(filter.limit.unwrap_or(50))
            .offset(filter.offset.unwrap_or(0))
            .select(InstructionDb::as_select())
            .load::<InstructionDb>(&mut conn)
            .map_err(StorageError::from)?;
        let items = rows
            .into_iter()
            .map(TradeInstruction::try_from)
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(InstructionPage { items, total })
    }
    async fn create(&self, instruction: TradeInstruction) -> Result<TradeInstruction> {
        let row = InstructionDb::try_from(instruction)?;
        self.writer
            .exec(move |conn| {
                diesel::insert_into(trade_instructions)
                    .values(&row)
                    .on_conflict(request_id)
                    .do_nothing()
                    .execute(conn)
                    .map_err(StorageError::from)?;
                let stored = trade_instructions
                    .filter(request_id.eq(&row.request_id))
                    .select(InstructionDb::as_select())
                    .first::<InstructionDb>(conn)
                    .map_err(StorageError::from)?;
                if stored.request_hash != row.request_hash {
                    return Err(Error::ConstraintViolation(
                        "requestId already belongs to a different instruction".into(),
                    ));
                }
                Ok(TradeInstruction::try_from(stored)?)
            })
            .await
    }
    async fn delete(&self, instruction_id: &str, expected_version: i32) -> Result<()> {
        let instruction_id = instruction_id.to_string();
        self.writer
            .exec(move |conn| {
                let changed = diesel::delete(
                    trade_instructions
                        .filter(id.eq(instruction_id))
                        .filter(version.eq(expected_version)),
                )
                .execute(conn)
                .map_err(StorageError::from)?;
                if changed != 1 {
                    return Err(Error::ConstraintViolation(
                        "Trade instruction changed. Refresh and try again.".into(),
                    ));
                }
                Ok(())
            })
            .await
    }
    async fn update(
        &self,
        instruction: TradeInstruction,
        expected_version: i32,
        expected_status: InstructionStatus,
    ) -> Result<TradeInstruction> {
        let row = InstructionDb::try_from(instruction)?;
        self.writer
            .exec(move |conn| {
                let changed = diesel::update(
                    trade_instructions
                        .filter(id.eq(&row.id))
                        .filter(version.eq(expected_version))
                        .filter(status.eq(expected_status.as_str())),
                )
                .set((
                    account_id.eq(&row.account_id),
                    account_name.eq(&row.account_name),
                    asset_id.eq(&row.asset_id),
                    asset_snapshot.eq(&row.asset_snapshot),
                    side.eq(&row.side),
                    transfer.eq(&row.transfer),
                    conversion.eq(&row.conversion),
                    quantity.eq(&row.quantity),
                    amount.eq(&row.amount),
                    amount_currency.eq(&row.amount_currency),
                    limit_price.eq(&row.limit_price),
                    notes.eq(&row.notes),
                    expected_completion_at.eq(&row.expected_completion_at),
                    status.eq(&row.status),
                    resolution_note.eq(&row.resolution_note),
                    updated_at.eq(&row.updated_at),
                    started_at.eq(&row.started_at),
                    closed_at.eq(&row.closed_at),
                    completed_at.eq(&row.completed_at),
                    version.eq(row.version),
                ))
                .execute(conn)
                .map_err(StorageError::from)?;
                if changed != 1 {
                    return Err(Error::ConstraintViolation(
                        "Trade instruction changed. Refresh and try again.".into(),
                    ));
                }
                Ok(TradeInstruction::try_from(row)?)
            })
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{create_pool, run_migrations, write_actor::spawn_writer};
    use serde_json::json;
    fn row() -> TradeInstruction {
        serde_json::from_value(json!({
            "id":"instruction-1", "goalId":"goal-1", "requestId":"request-1", "requestHash":"hash", "accountId":"account-1", "assetId":"asset-1", "accountName":"Account snapshot",
            "assetSnapshot":{"name":"Snapshot", "symbol":"ABC", "exchangeMic":"XNAS", "instrumentType":"EQUITY", "quoteCcy":"USD"},
            "side":"BUY", "quantity":"0.123456789123456789", "amount":null, "amountCurrency":null, "limitPrice":"12.3456789", "notes":"Optional notes",
            "expectedCompletionAt":null, "status":"PENDING", "resolutionNote":null, "source":"MCP", "createdAt":"2026-01-01T00:00:00Z", "updatedAt":"2026-01-01T00:00:00Z",
            "startedAt":null, "closedAt":null, "completedAt":null, "version":1
        })).unwrap()
    }
    #[tokio::test]
    async fn trade_instruction_persistence_idempotency_cas_and_no_outbox() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir
            .path()
            .join("instructions.db")
            .to_string_lossy()
            .to_string();
        run_migrations(&path).unwrap();
        let pool = create_pool(&path).unwrap();
        let writer = spawn_writer((*pool).clone()).unwrap();
        let repo = TradeInstructionRepository::new(pool.clone(), writer.clone());
        diesel::sql_query("INSERT INTO goals (id, title, target_amount) VALUES ('goal-1', 'Goal', 1000), ('goal-2', 'Other', 2000)")
            .execute(&mut get_connection(&pool).unwrap()).unwrap();
        let initial = row();
        let saved = repo.create(initial.clone()).await.unwrap();
        assert_eq!(saved.quantity, initial.quantity);
        let mut retry = initial.clone();
        retry.id = "other".into();
        assert_eq!(repo.create(retry).await.unwrap().id, saved.id);
        let mut changed = initial.clone();
        changed.request_hash = "different".into();
        assert!(repo.create(changed).await.is_err());
        let mut started = saved.clone();
        started.status = InstructionStatus::InProgress;
        started.version = 2;
        started.started_at = Some(chrono::Utc::now());
        repo.update(started.clone(), 1, InstructionStatus::Pending)
            .await
            .unwrap();
        assert!(repo
            .update(started, 1, InstructionStatus::Pending)
            .await
            .is_err());
        let reopened = TradeInstructionRepository::new(pool.clone(), writer);
        assert_eq!(
            reopened.get(&initial.id).unwrap().status,
            InstructionStatus::InProgress
        );
        assert_eq!(
            reopened.list(InstructionFilter::default()).unwrap().total,
            1
        );
        let mut conn = get_connection(&pool).unwrap();
        assert_eq!(
            crate::schema::sync_outbox::table
                .count()
                .get_result::<i64>(&mut conn)
                .unwrap(),
            0
        );
        assert_eq!(
            crate::schema::activities::table
                .count()
                .get_result::<i64>(&mut conn)
                .unwrap(),
            0
        );
        assert_eq!(
            crate::schema::quotes::table
                .count()
                .get_result::<i64>(&mut conn)
                .unwrap(),
            0
        );
        assert_eq!(
            reopened
                .list(InstructionFilter {
                    goal_id: Some("goal-2".into()),
                    ..Default::default()
                })
                .unwrap()
                .total,
            0
        );
        diesel::sql_query("DELETE FROM goals WHERE id = 'goal-1'")
            .execute(&mut conn)
            .unwrap();
        assert!(reopened.get(&initial.id).is_err());
    }
    #[tokio::test]
    async fn trade_instruction_filter_sort_and_paginate_before_loading() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("filter.db").to_string_lossy().to_string();
        run_migrations(&path).unwrap();
        let pool = create_pool(&path).unwrap();
        let writer = spawn_writer((*pool).clone()).unwrap();
        let repo = TradeInstructionRepository::new(pool.clone(), writer);
        diesel::sql_query("INSERT INTO goals (id, title, target_amount) VALUES ('goal-1', 'Goal', 1000), ('goal-2', 'Other', 2000)")
            .execute(&mut get_connection(&pool).unwrap()).unwrap();
        for (key, expected, direction, state, account, goal) in [
            (
                "a",
                Some("2026-10-01T00:00:00Z"),
                InstructionSide::Buy,
                InstructionStatus::Pending,
                "account-1",
                "goal-1",
            ),
            (
                "b",
                Some("2026-10-01T00:00:00Z"),
                InstructionSide::Buy,
                InstructionStatus::Pending,
                "account-1",
                "goal-1",
            ),
            (
                "c",
                Some("2026-10-02T00:00:00Z"),
                InstructionSide::Sell,
                InstructionStatus::Completed,
                "account-1",
                "goal-1",
            ),
            (
                "d",
                None,
                InstructionSide::Buy,
                InstructionStatus::Pending,
                "account-1",
                "goal-1",
            ),
            (
                "e",
                Some("2026-10-01T00:00:00Z"),
                InstructionSide::Buy,
                InstructionStatus::Pending,
                "account-2",
                "goal-1",
            ),
            (
                "f",
                Some("2026-10-01T00:00:00Z"),
                InstructionSide::Buy,
                InstructionStatus::Pending,
                "account-1",
                "goal-2",
            ),
        ] {
            let mut item = row();
            item.id = key.into();
            item.request_id = key.into();
            item.expected_completion_at = expected.map(|v| v.parse().unwrap());
            item.side = direction;
            item.status = state;
            if state == InstructionStatus::Completed {
                item.completed_at = item.expected_completion_at;
            }
            item.account_id = account.into();
            item.goal_id = goal.into();
            repo.create(item).await.unwrap();
        }
        let base = InstructionFilter {
            goal_id: Some("goal-1".into()),
            account_id: Some("account-1".into()),
            statuses: Some(vec![]),
            sort: Some(InstructionSort::ExpectedCompletionAsc),
            ..Default::default()
        };
        let ids = |page: InstructionPage| page.items.into_iter().map(|v| v.id).collect::<Vec<_>>();
        assert_eq!(ids(repo.list(base.clone()).unwrap()), ["a", "b", "c", "d"]);
        assert_eq!(
            ids(repo
                .list(InstructionFilter {
                    sort: Some(InstructionSort::ExpectedCompletionDesc),
                    ..base.clone()
                })
                .unwrap()),
            ["c", "a", "b", "d"]
        );
        let first = repo
            .list(InstructionFilter {
                limit: Some(2),
                ..base.clone()
            })
            .unwrap();
        assert_eq!(first.total, 4);
        assert_eq!(ids(first), ["a", "b"]);
        assert_eq!(
            ids(repo
                .list(InstructionFilter {
                    limit: Some(2),
                    offset: Some(2),
                    ..base.clone()
                })
                .unwrap()),
            ["c", "d"]
        );
        let filtered = repo
            .list(InstructionFilter {
                side: Some(InstructionSide::Buy),
                statuses: Some(vec![InstructionStatus::Pending]),
                expected_completion_from: Some("2026-09-30T20:00:00-04:00".parse().unwrap()),
                expected_completion_before: Some("2026-10-02T00:00:00Z".parse().unwrap()),
                ..base.clone()
            })
            .unwrap();
        assert_eq!(filtered.total, 2);
        assert_eq!(ids(filtered), ["a", "b"]);
        assert_eq!(
            ids(repo
                .list(InstructionFilter {
                    sort: None,
                    ..base.clone()
                })
                .unwrap()),
            ["d", "c", "b", "a"]
        );
        assert_eq!(
            ids(repo
                .list(InstructionFilter {
                    statuses: None,
                    ..base
                })
                .unwrap()),
            ["a", "b", "d"]
        );
        let original = repo.get("a").unwrap();
        let mut edited = original.clone();
        edited.account_id = "account-2".into();
        edited.account_name = "Changed account".into();
        edited.asset_id = Some("asset-2".into());
        edited.asset_snapshot.as_mut().unwrap().name = Some("Changed snapshot".into());
        edited.side = InstructionSide::Sell;
        edited.quantity = None;
        edited.amount = Some("25.123456789".parse().unwrap());
        edited.amount_currency = Some("USD".into());
        edited.notes = None;
        edited.limit_price = None;
        edited.expected_completion_at = None;
        edited.version = 2;
        repo.update(edited, 1, InstructionStatus::Pending)
            .await
            .unwrap();
        let persisted = repo.get("a").unwrap();
        assert_eq!(persisted.account_name, "Changed account");
        assert_eq!(persisted.asset_id.as_deref(), Some("asset-2"));
        assert_eq!(
            persisted.asset_snapshot.as_ref().unwrap().name.as_deref(),
            Some("Changed snapshot")
        );
        assert_eq!(persisted.amount.unwrap().to_string(), "25.123456789");
        assert!(persisted.quantity.is_none());
        assert!(persisted.notes.is_none());
        assert!(persisted.limit_price.is_none());
        assert!(persisted.expected_completion_at.is_none());
        assert_eq!(persisted.request_hash, original.request_hash);
        assert!(repo
            .update(original, 1, InstructionStatus::Pending)
            .await
            .is_err());
        assert!(repo.delete("a", 1).await.is_err());
        repo.delete("a", 2).await.unwrap();
        assert!(repo.get("a").is_err());
        assert!(repo.get_by_request_id("a").unwrap().is_none());
        repo.delete("c", 1).await.unwrap();
        let mut conn = get_connection(&pool).unwrap();
        assert_eq!(
            crate::schema::sync_outbox::table
                .count()
                .get_result::<i64>(&mut conn)
                .unwrap(),
            0
        );
        assert_eq!(
            crate::schema::activities::table
                .count()
                .get_result::<i64>(&mut conn)
                .unwrap(),
            0
        );
    }

    #[tokio::test]
    async fn trade_instruction_actions_roundtrip_filter_and_clear() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("actions.db").to_string_lossy().to_string();
        run_migrations(&path).unwrap();
        let pool = create_pool(&path).unwrap();
        let repo =
            TradeInstructionRepository::new(pool.clone(), spawn_writer((*pool).clone()).unwrap());
        let mut conn = get_connection(&pool).unwrap();
        diesel::sql_query(
            "INSERT INTO goals (id,title,target_amount) VALUES ('goal-1','Goal',1000)",
        )
        .execute(&mut conn)
        .unwrap();
        for (key, fields) in [
            (
                "deposit",
                json!({"side":"DEPOSIT","assetId":null,"assetSnapshot":null,"quantity":null,"amount":"1.123456789123456789","amountCurrency":"USD","limitPrice":null}),
            ),
            (
                "withdrawal",
                json!({"side":"WITHDRAWAL","assetId":null,"assetSnapshot":null,"quantity":null,"amount":"2","amountCurrency":"USD","limitPrice":null}),
            ),
            (
                "cash",
                json!({"side":"TRANSFER","assetId":null,"assetSnapshot":null,"quantity":null,"amount":"3","amountCurrency":"USD","limitPrice":null,"transfer":{"mode":"cash","isExternal":false,"direction":null,"toAccountId":"destination","toAccountName":"Destination snapshot","destinationCurrency":"EUR","destinationAmount":null,"unitPrice":null,"costCurrency":null}}),
            ),
            (
                "fx",
                json!({"side":"TRANSFER","accountId":"destination","assetId":null,"assetSnapshot":null,"quantity":null,"amount":"4","amountCurrency":"USD","limitPrice":null,"transfer":{"mode":"cash","isExternal":false,"direction":null,"toAccountId":"destination","toAccountName":"Destination snapshot","destinationCurrency":"EUR","destinationAmount":"3.123456789123456789","unitPrice":null,"costCurrency":null}}),
            ),
            (
                "securities",
                json!({"side":"TRANSFER","quantity":"5","limitPrice":null,"transfer":{"mode":"securities","isExternal":true,"direction":"in","toAccountId":null,"toAccountName":null,"destinationCurrency":null,"destinationAmount":null,"unitPrice":"6.123456789123456789","costCurrency":"USD"}}),
            ),
            (
                "conversion",
                json!({"side":"CONVERSION","quantity":"7","limitPrice":null,"conversion":{"toAssetId":"target","toAssetSnapshot":{"name":"Target snapshot","symbol":"DEF","exchangeMic":"XNAS","instrumentType":"EQUITY","quoteCcy":"USD"},"toQuantity":null}}),
            ),
        ] {
            let mut value = serde_json::to_value(row()).unwrap();
            value
                .as_object_mut()
                .unwrap()
                .extend(fields.as_object().unwrap().clone());
            value["id"] = json!(key);
            value["requestId"] = json!(key);
            let item: TradeInstruction = serde_json::from_value(value).unwrap();
            let saved = repo.create(item.clone()).await.unwrap();
            let loaded = repo.get(&saved.id).unwrap();
            assert_eq!(
                serde_json::to_value(&loaded).unwrap(),
                serde_json::to_value(&item).unwrap()
            );
            assert_eq!(repo.create(item).await.unwrap().id, saved.id);
        }
        let page = repo
            .list(InstructionFilter {
                account_id: Some("destination".into()),
                sides: Some(vec![InstructionSide::Transfer]),
                limit: Some(1),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(page.total, 2);
        assert_eq!(page.items.len(), 1);
        assert_eq!(
            repo.list(InstructionFilter {
                sides: Some(vec![InstructionSide::Deposit, InstructionSide::Conversion]),
                ..Default::default()
            })
            .unwrap()
            .total,
            2
        );
        let mut changed = repo.get("conversion").unwrap();
        changed.side = InstructionSide::Deposit;
        changed.asset_id = None;
        changed.asset_snapshot = None;
        changed.quantity = None;
        changed.amount = Some("10".parse().unwrap());
        changed.amount_currency = Some("USD".into());
        changed.conversion = None;
        changed.version += 1;
        repo.update(changed, 1, InstructionStatus::Pending)
            .await
            .unwrap();
        assert!(repo.get("conversion").unwrap().conversion.is_none());
        for statement in [
            "SELECT CAST(COUNT(*) AS TEXT) AS value FROM activities",
            "SELECT CAST(COUNT(*) AS TEXT) AS value FROM quotes",
            "SELECT CAST(COUNT(*) AS TEXT) AS value FROM sync_outbox",
        ] {
            #[derive(QueryableByName)]
            struct Count {
                #[diesel(sql_type=diesel::sql_types::Text)]
                value: String,
            }
            assert_eq!(
                diesel::sql_query(statement)
                    .get_result::<Count>(&mut conn)
                    .unwrap()
                    .value,
                "0"
            );
        }
        diesel::sql_query("DELETE FROM goals WHERE id='goal-1'")
            .execute(&mut conn)
            .unwrap();
        assert_eq!(repo.list(InstructionFilter::default()).unwrap().total, 0);
    }

    #[test]
    fn trade_instruction_migration_preserves_all_legacy_columns_and_hashes() {
        use diesel::connection::SimpleConnection;
        use diesel_migrations::MigrationHarness;
        const MIGRATIONS: diesel_migrations::EmbeddedMigrations =
            diesel_migrations::embed_migrations!();
        let mut conn = diesel::sqlite::SqliteConnection::establish(":memory:").unwrap();
        conn.batch_execute("PRAGMA foreign_keys=ON").unwrap();
        let mut migrations = conn.pending_migrations(MIGRATIONS).unwrap();
        migrations.sort_by_key(|m| m.name().version().to_string());
        for migration in migrations {
            if migration.name().version().to_string().as_str() >= "20261105000001" {
                break;
            }
            conn.run_migration(&*migration).unwrap();
        }
        conn.batch_execute(r#"
            INSERT INTO goals(id,title,target_amount) VALUES ('g','Goal',100);
            INSERT INTO trade_instructions(id,request_id,request_hash,account_id,asset_id,account_name,asset_snapshot,side,quantity,amount,amount_currency,limit_price,expected_completion_at,status,resolution_note,source,created_at,updated_at,started_at,closed_at,completed_at,version,goal_id,notes)
            VALUES ('legacy','request','original-hash','account','asset','Saved account','{"name":"Saved","symbol":"ABC","exchangeMic":"XNAS","instrumentType":"EQUITY","quoteCcy":"USD"}','BUY','1.123456789123456789',NULL,NULL,'12.34','2026-01-01T00:00:00Z','COMPLETED','Done','MCP','2025-01-01T00:00:00Z','2026-01-01T00:00:00Z','2025-12-31T00:00:00Z','2026-01-01T00:00:00Z','2025-12-31T23:00:00Z',4,'g','User notes');
        "#).unwrap();
        #[derive(QueryableByName)]
        struct JsonRow {
            #[diesel(sql_type=diesel::sql_types::Text)]
            value: String,
        }
        let columns = "id,request_id,request_hash,account_id,asset_id,account_name,asset_snapshot,side,quantity,amount,amount_currency,limit_price,expected_completion_at,status,resolution_note,source,created_at,updated_at,started_at,closed_at,completed_at,version,goal_id,notes";
        let pairs = columns
            .split(',')
            .map(|column| format!("'{column}',{column}"))
            .collect::<Vec<_>>()
            .join(",");
        let query = format!("SELECT json_object({pairs}) AS value FROM trade_instructions");
        let before = diesel::sql_query(&query)
            .get_result::<JsonRow>(&mut conn)
            .unwrap()
            .value;
        conn.run_pending_migrations(MIGRATIONS).unwrap();
        assert_eq!(
            diesel::sql_query(&query)
                .get_result::<JsonRow>(&mut conn)
                .unwrap()
                .value,
            before
        );
        let saved: TradeInstruction = trade_instructions
            .select(InstructionDb::as_select())
            .first::<InstructionDb>(&mut conn)
            .unwrap()
            .try_into()
            .unwrap();
        assert_eq!(saved.request_hash, "original-hash");
        assert!(saved.transfer.is_none() && saved.conversion.is_none());
        assert!(conn.run_pending_migrations(MIGRATIONS).unwrap().is_empty());
    }
}
