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
                q = q.filter(account_id.eq(account));
            }
            q
        };
        let total = query()
            .count()
            .get_result(&mut conn)
            .map_err(StorageError::from)?;
        let rows = query()
            .order((created_at.desc(), id.desc()))
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
}
