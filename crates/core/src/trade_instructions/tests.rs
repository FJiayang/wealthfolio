use super::*;
use crate::activities::activities_service_tests::tests::{
    create_test_account, create_test_asset, MockAccountService, MockAssetService,
};
use crate::{errors::DatabaseError, Error, Result};
use async_trait::async_trait;
use chrono::{Duration, Utc};
use serde_json::json;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

#[derive(Default)]
struct MemoryRepository(Mutex<HashMap<String, TradeInstruction>>);
#[async_trait]
impl TradeInstructionRepositoryTrait for MemoryRepository {
    fn get(&self, id: &str) -> Result<TradeInstruction> {
        self.0
            .lock()
            .unwrap()
            .get(id)
            .cloned()
            .ok_or_else(|| DatabaseError::NotFound(id.into()).into())
    }
    fn get_by_request_id(&self, key: &str) -> Result<Option<TradeInstruction>> {
        Ok(self
            .0
            .lock()
            .unwrap()
            .values()
            .find(|v| v.request_id == key)
            .cloned())
    }
    fn list(&self, _: InstructionFilter) -> Result<InstructionPage> {
        unimplemented!()
    }
    async fn delete(&self, id: &str, version: i32) -> Result<()> {
        let mut rows = self.0.lock().unwrap();
        if rows.get(id).is_none_or(|row| row.version != version) {
            return Err(Error::ConstraintViolation("stale".into()));
        }
        rows.remove(id);
        Ok(())
    }
    async fn create(&self, row: TradeInstruction) -> Result<TradeInstruction> {
        self.0.lock().unwrap().insert(row.id.clone(), row.clone());
        Ok(row)
    }
    async fn update(
        &self,
        row: TradeInstruction,
        version: i32,
        status: InstructionStatus,
    ) -> Result<TradeInstruction> {
        let mut rows = self.0.lock().unwrap();
        let existing = rows.get(&row.id).unwrap();
        if existing.version != version || existing.status != status {
            return Err(Error::ConstraintViolation("stale".into()));
        }
        rows.insert(row.id.clone(), row.clone());
        Ok(row)
    }
}
fn setup() -> (TradeInstructionService, Arc<MemoryRepository>) {
    setup_with_accounts(vec![])
}
fn setup_with_accounts(
    extra: Vec<crate::accounts::Account>,
) -> (TradeInstructionService, Arc<MemoryRepository>) {
    let accounts = Arc::new(MockAccountService::new());
    accounts.add_account(create_test_account("a", "USD"));
    accounts.add_account(create_test_account("b", "USD"));
    for account in extra {
        accounts.add_account(account);
    }
    let assets = MockAssetService::new();
    let mut asset = create_test_asset("s", "USD");
    asset.is_active = true;
    assets.add_asset(asset);
    let mut other = create_test_asset("t", "USD");
    other.is_active = true;
    assets.add_asset(other);
    let repo = Arc::new(MemoryRepository::default());
    let goal = serde_json::from_value(json!({"id":"g", "goalType":"custom_save_up", "title":"Goal", "statusLifecycle":"active", "statusHealth":"not_applicable", "priority":0, "createdAt":"", "updatedAt":""})).unwrap();
    let goals = Arc::new(crate::goals::GoalService::new(
        Arc::new(crate::goals::MockGoalRepository::new(vec![goal])),
        accounts.clone(),
    ));
    (
        TradeInstructionService::new(repo.clone(), accounts, Arc::new(assets), goals),
        repo,
    )
}
fn input() -> NewTradeInstruction {
    serde_json::from_value(json!({"goalId":"g","requestId":uuid::Uuid::new_v4().to_string(),"accountId":"a","asset":{"id":"s"},"side":"BUY","quantity":"1.2300","limitPrice":"0.0100","expectedCompletionAt":"2020-01-01T09:00:00+09:00"})).unwrap()
}
#[tokio::test]
async fn trade_instruction_completion_retry_correction_and_conflict() {
    let (service, _) = setup();
    let row = service
        .create(input(), InstructionSource::Mcp)
        .await
        .unwrap();
    assert_eq!(row.quantity.unwrap().to_string(), "1.23");
    assert_eq!(
        row.expected_completion_at.unwrap().to_rfc3339(),
        "2020-01-01T00:00:00+00:00"
    );
    let done = service
        .complete(
            &row.id,
            CompleteInstruction {
                expected_version: 1,
                resolution_note: Some("Partial execution; remainder abandoned".into()),
                completed_at: None,
            },
        )
        .await
        .unwrap();
    let again = service
        .complete(
            &row.id,
            CompleteInstruction {
                expected_version: 1,
                resolution_note: Some("Partial execution; remainder abandoned".into()),
                completed_at: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(done.completed_at, again.completed_at);
    assert_eq!(done.version, again.version);
    let actual = Utc::now() - Duration::days(2);
    let corrected = service
        .correct_completion_time(
            &row.id,
            CorrectCompletionTime {
                expected_version: done.version,
                completed_at: actual,
            },
        )
        .await
        .unwrap();
    assert_eq!(corrected.completed_at, Some(actual));
    assert_eq!(corrected.closed_at, done.closed_at);
    assert!(service
        .correct_completion_time(
            &row.id,
            CorrectCompletionTime {
                expected_version: done.version,
                completed_at: actual
            }
        )
        .await
        .is_err());
    assert!(service.start(&row.id, corrected.version).await.is_err());
    assert!(service
        .cancel(
            &row.id,
            CancelInstruction {
                expected_version: corrected.version,
                resolution_note: "cancel".into()
            }
        )
        .await
        .is_err());
    assert!(service
        .correct_completion_time(
            &row.id,
            CorrectCompletionTime {
                expected_version: corrected.version,
                completed_at: Utc::now() + Duration::days(1)
            }
        )
        .await
        .is_err());
}
#[tokio::test]
async fn trade_instruction_idempotency_and_validation_precede_asset_access() {
    let (service, repo) = setup();
    let request = input();
    let first = service
        .create(request.clone(), InstructionSource::Ui)
        .await
        .unwrap();
    let mut same = request.clone();
    same.quantity = Some("1.23".parse().unwrap());
    assert_eq!(
        service
            .create(same, InstructionSource::Ui)
            .await
            .unwrap()
            .id,
        first.id
    );
    let mut different = request;
    different.side = InstructionSide::Sell;
    assert!(service
        .create(different, InstructionSource::Ui)
        .await
        .is_err());
    let mut invalid = input();
    invalid.asset.as_mut().unwrap().id = Some("missing".into());
    invalid.amount = Some("10".parse().unwrap());
    assert!(matches!(
        service.create(invalid, InstructionSource::Ui).await,
        Err(Error::Validation(_))
    ));
    assert_eq!(repo.0.lock().unwrap().len(), 1);
}
#[tokio::test]
async fn trade_instruction_start_cancel_and_missing_references() {
    let (service, repo) = setup();
    let row = service
        .create(input(), InstructionSource::Ui)
        .await
        .unwrap();
    let started = service.start(&row.id, row.version).await.unwrap();
    assert!(started.started_at.is_some());
    let cancelled = service
        .cancel(
            &row.id,
            CancelInstruction {
                expected_version: started.version,
                resolution_note: "No longer needed".into(),
            },
        )
        .await
        .unwrap();
    assert_eq!(cancelled.status, InstructionStatus::Cancelled);
    assert!(cancelled.completed_at.is_none());
    let mut request = input();
    request.quantity = None;
    request.amount = Some("10.01".parse().unwrap());
    request.amount_currency = Some("usd".into());
    let row = service
        .create(request, InstructionSource::Ui)
        .await
        .unwrap();
    {
        let mut rows = repo.0.lock().unwrap();
        let stored = rows.get_mut(&row.id).unwrap();
        stored.account_id = "deleted".into();
        stored.asset_id = Some("deleted".into());
    }
    assert!(service.start(&row.id, row.version).await.is_err());
    assert!(service
        .complete(
            &row.id,
            CompleteInstruction {
                expected_version: row.version,
                resolution_note: Some("Already executed".into()),
                completed_at: None
            }
        )
        .await
        .is_ok());
}

#[tokio::test]
async fn trade_instruction_requires_existing_goal_and_preserves_optional_notes() {
    let (service, repo) = setup();
    let mut request = input();
    request.goal_id = "missing-goal".into();
    request.asset.as_mut().unwrap().id = Some("missing-asset".into());
    let error = service
        .create(request, InstructionSource::Mcp)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("missing-goal"));
    assert!(repo.0.lock().unwrap().is_empty());
    let mut request = input();
    request.goal_id = " ".into();
    assert!(matches!(
        service.create(request, InstructionSource::Ui).await,
        Err(Error::Validation(_))
    ));
    let mut request = input();
    request.notes = Some(" Optional reason\nExecution detail ".into());
    let row = service
        .create(request, InstructionSource::Mcp)
        .await
        .unwrap();
    assert_eq!(row.goal_id, "g");
    assert_eq!(
        row.notes.as_deref(),
        Some("Optional reason\nExecution detail")
    );
    let row = service
        .create(input(), InstructionSource::Ui)
        .await
        .unwrap();
    assert!(row.notes.is_none());
    let mut raw = serde_json::to_value(input()).unwrap();
    raw.as_object_mut().unwrap().remove("goalId");
    assert!(serde_json::from_value::<NewTradeInstruction>(raw).is_err());
}

#[test]
fn trade_instruction_list_rejects_invalid_completion_range() {
    let (service, _) = setup();
    let now = Utc::now();
    for before in [now, now - Duration::seconds(1)] {
        assert!(service
            .list(InstructionFilter {
                expected_completion_from: Some(now),
                expected_completion_before: Some(before),
                ..Default::default()
            })
            .is_err());
    }
    assert!(
        serde_json::from_value::<InstructionFilter>(json!({"expectedCompletionFrom": "bad"}))
            .is_err()
    );
}

fn edit_input(version: i32) -> UpdateTradeInstruction {
    serde_json::from_value(json!({"expectedVersion":version,"accountId":"a","asset":{"id":"s"},"side":"SELL","amount":"12.3400","amountCurrency":"usd","notes":"Updated"})).unwrap()
}

#[tokio::test]
async fn trade_instruction_edit_preserves_identity_and_rejects_stale_or_started_rows() {
    let (service, _) = setup();
    let original_input = input();
    let original = service
        .create(original_input.clone(), InstructionSource::Mcp)
        .await
        .unwrap();
    let edited = service.edit(&original.id, edit_input(1)).await.unwrap();
    assert_eq!(edited.version, 2);
    assert_eq!(edited.request_id, original.request_id);
    assert_eq!(edited.request_hash, original.request_hash);
    assert_eq!(edited.created_at, original.created_at);
    assert_eq!(edited.source, original.source);
    assert_eq!(edited.goal_id, original.goal_id);
    assert_eq!(
        serde_json::to_value(&edited.asset_snapshot).unwrap(),
        serde_json::to_value(&original.asset_snapshot).unwrap()
    );
    assert_eq!(edited.status, InstructionStatus::Pending);
    assert_eq!(edited.side, InstructionSide::Sell);
    assert_eq!(edited.quantity, None);
    assert_eq!(edited.amount.unwrap().to_string(), "12.34");
    assert_eq!(edited.amount_currency.as_deref(), Some("USD"));
    assert!(edited.limit_price.is_none());
    assert!(edited.expected_completion_at.is_none());
    assert_eq!(
        service
            .create(original_input, InstructionSource::Mcp)
            .await
            .unwrap()
            .version,
        2
    );
    assert!(service.edit(&original.id, edit_input(1)).await.is_err());
    let mut changed = edit_input(2);
    changed.account_id = "b".into();
    changed.asset.as_mut().unwrap().id = Some("t".into());
    changed.notes = Some(" ".into());
    let changed = service.edit(&original.id, changed).await.unwrap();
    assert_eq!(changed.account_id, "b");
    assert_eq!(changed.asset_id.as_deref(), Some("t"));
    assert!(changed.notes.is_none());
    let started = service.start(&original.id, 3).await.unwrap();
    assert!(service
        .edit(&original.id, edit_input(started.version))
        .await
        .is_err());
}

#[tokio::test]
async fn trade_instruction_completion_accepts_empty_notes_but_cancellation_requires_one() {
    for value in [
        json!({"expectedVersion":1}),
        json!({"expectedVersion":1,"resolutionNote":null}),
        json!({"expectedVersion":1,"resolutionNote":""}),
        json!({"expectedVersion":1,"resolutionNote":"  "}),
    ] {
        let (service, _) = setup();
        let row = service
            .create(input(), InstructionSource::Ui)
            .await
            .unwrap();
        let before = Utc::now();
        let done = service
            .complete(&row.id, serde_json::from_value(value).unwrap())
            .await
            .unwrap();
        assert_eq!(done.resolution_note, None);
        assert!(done.completed_at.unwrap() >= before);
        assert!(done.completed_at.unwrap() <= Utc::now());
        let retried = service
            .complete(
                &row.id,
                CompleteInstruction {
                    expected_version: 1,
                    resolution_note: Some("  ".into()),
                    completed_at: None,
                },
            )
            .await
            .unwrap();
        assert_eq!(retried.completed_at, done.completed_at);
        assert_eq!(retried.version, done.version);
    }
    let (service, _) = setup();
    let row = service
        .create(input(), InstructionSource::Ui)
        .await
        .unwrap();
    assert!(service
        .complete(
            &row.id,
            CompleteInstruction {
                expected_version: 1,
                resolution_note: Some("x".repeat(4001)),
                completed_at: None
            }
        )
        .await
        .is_err());
    assert!(service
        .complete(
            &row.id,
            CompleteInstruction {
                expected_version: 1,
                resolution_note: None,
                completed_at: Some(Utc::now() + Duration::days(1))
            }
        )
        .await
        .is_err());
    assert!(service
        .cancel(
            &row.id,
            CancelInstruction {
                expected_version: 1,
                resolution_note: " ".into()
            }
        )
        .await
        .is_err());
}

#[tokio::test]
async fn trade_instruction_delete_all_statuses_checks_version_and_removes_creation_key() {
    for state in [
        InstructionStatus::Pending,
        InstructionStatus::InProgress,
        InstructionStatus::Completed,
        InstructionStatus::Cancelled,
    ] {
        let (service, repo) = setup();
        let mut row = service
            .create(input(), InstructionSource::Ui)
            .await
            .unwrap();
        row = match state {
            InstructionStatus::Pending => row,
            InstructionStatus::InProgress => service.start(&row.id, row.version).await.unwrap(),
            InstructionStatus::Completed => service
                .complete(
                    &row.id,
                    CompleteInstruction {
                        expected_version: row.version,
                        resolution_note: None,
                        completed_at: None,
                    },
                )
                .await
                .unwrap(),
            InstructionStatus::Cancelled => service
                .cancel(
                    &row.id,
                    CancelInstruction {
                        expected_version: row.version,
                        resolution_note: "Abandoned".into(),
                    },
                )
                .await
                .unwrap(),
        };
        assert!(service.delete(&row.id, row.version - 1).await.is_err());
        assert!(service.get(&row.id).is_ok());
        service.delete(&row.id, row.version).await.unwrap();
        assert!(service.get(&row.id).is_err());
        assert!(repo.get_by_request_id(&row.request_id).unwrap().is_none());
    }
}

fn operation(mut fields: serde_json::Value) -> NewTradeInstruction {
    fields["goalId"] = json!("g");
    fields["accountId"] = fields.get("accountId").cloned().unwrap_or(json!("a"));
    fields["requestId"] = json!(uuid::Uuid::new_v4().to_string());
    serde_json::from_value(fields).unwrap()
}

#[tokio::test]
async fn trade_instruction_actions_preserve_unknown_values_and_complete_without_bookkeeping() {
    for fields in [
        json!({"side":"DEPOSIT","amount":"123.123456789123456789","amountCurrency":"usd"}),
        json!({"side":"WITHDRAWAL","amount":"12","amountCurrency":"USD"}),
        json!({"side":"TRANSFER","amount":"12","amountCurrency":"USD","transfer":{"mode":"cash","isExternal":false,"toAccountId":"b","destinationCurrency":"eur"}}),
        json!({"side":"TRANSFER","amount":"12","amountCurrency":"USD","transfer":{"mode":"cash","isExternal":false,"toAccountId":"a","destinationCurrency":"EUR","destinationAmount":"10.123456789123456789"}}),
        json!({"side":"TRANSFER","amount":"12","amountCurrency":"USD","transfer":{"mode":"cash","isExternal":false,"toAccountId":"b","destinationCurrency":"USD"}}),
        json!({"side":"TRANSFER","amount":"12","amountCurrency":"USD","transfer":{"mode":"cash","isExternal":true,"direction":"in"}}),
        json!({"side":"TRANSFER","amount":"12","amountCurrency":"USD","transfer":{"mode":"cash","isExternal":true,"direction":"out"}}),
        json!({"side":"TRANSFER","asset":{"id":"s"},"quantity":"1000000.123456789","transfer":{"mode":"securities","isExternal":false,"toAccountId":"b"}}),
        json!({"side":"TRANSFER","asset":{"id":"s"},"quantity":"1","transfer":{"mode":"securities","isExternal":true,"direction":"out"}}),
        json!({"side":"TRANSFER","asset":{"id":"s"},"quantity":"1","transfer":{"mode":"securities","isExternal":true,"direction":"in"}}),
        json!({"side":"TRANSFER","asset":{"id":"s"},"quantity":"1","transfer":{"mode":"securities","isExternal":true,"direction":"in","unitPrice":"9.123456789123456789","costCurrency":"usd"}}),
        json!({"side":"CONVERSION","asset":{"id":"s"},"quantity":"1","conversion":{"toAsset":{"id":"t"}}}),
        json!({"side":"CONVERSION","asset":{"id":"s"},"quantity":"1","conversion":{"toAsset":{"id":"t"},"toQuantity":"2.123456789123456789"}}),
    ] {
        let (service, _) = setup();
        let request = operation(fields);
        let first = service
            .create(request.clone(), InstructionSource::Ui)
            .await
            .unwrap();
        assert_eq!(
            service
                .create(request, InstructionSource::Ui)
                .await
                .unwrap()
                .id,
            first.id
        );
        if let Some(transfer) = &first.transfer {
            if !transfer.is_external && transfer.mode == InstructionTransferMode::Cash {
                assert!(transfer.to_account_name.is_some());
                if transfer.destination_currency.as_deref() == Some("USD") {
                    assert_eq!(transfer.destination_amount, first.amount);
                }
            }
        }
        let started = service.start(&first.id, 1).await.unwrap();
        let done = service
            .complete(
                &first.id,
                CompleteInstruction {
                    expected_version: started.version,
                    resolution_note: None,
                    completed_at: None,
                },
            )
            .await
            .unwrap();
        assert_eq!(done.status, InstructionStatus::Completed);
        assert_eq!(
            serde_json::to_value(&first.transfer).unwrap(),
            serde_json::to_value(&done.transfer).unwrap()
        );
        assert_eq!(
            serde_json::to_value(&first.conversion).unwrap(),
            serde_json::to_value(&done.conversion).unwrap()
        );
    }
}

#[tokio::test]
async fn trade_instruction_rejects_invalid_operation_fields() {
    for fields in [
        json!({"side":"DEPOSIT","quantity":"1"}),
        json!({"side":"WITHDRAWAL","amount":"1","amountCurrency":"USD","asset":{"id":"s"}}),
        json!({"side":"DEPOSIT","amount":"1","amountCurrency":"USD","limitPrice":"1"}),
        json!({"side":"DEPOSIT","amount":"1","amountCurrency":"USD","conversion":{"toAsset":{"id":"t"}}}),
        json!({"side":"TRANSFER","amount":"1","amountCurrency":"USD"}),
        json!({"side":"TRANSFER","amount":"1","amountCurrency":"USD","transfer":{"mode":"cash","isExternal":true}}),
        json!({"side":"TRANSFER","amount":"1","amountCurrency":"USD","transfer":{"mode":"cash","isExternal":true,"direction":"in","toAccountId":"b"}}),
        json!({"side":"TRANSFER","amount":"1","amountCurrency":"USD","transfer":{"mode":"cash","isExternal":false,"toAccountId":"a","destinationCurrency":"USD"}}),
        json!({"side":"TRANSFER","amount":"1","amountCurrency":"USD","transfer":{"mode":"cash","isExternal":false,"toAccountId":"b","destinationCurrency":"USD","destinationAmount":"2"}}),
        json!({"side":"TRANSFER","amount":"1","amountCurrency":"USD","transfer":{"mode":"cash","isExternal":false,"toAccountId":"b","destinationCurrency":"EUR","destinationAmount":"0"}}),
        json!({"side":"TRANSFER","amount":"1","amountCurrency":"USD","transfer":{"mode":"cash","isExternal":false,"toAccountId":"b","destinationCurrency":"EUR","unitPrice":"1","costCurrency":"USD"}}),
        json!({"side":"TRANSFER","quantity":"1","asset":{"id":"s"},"transfer":{"mode":"securities","isExternal":false,"toAccountId":"a"}}),
        json!({"side":"TRANSFER","quantity":"1","asset":{"id":"s"},"transfer":{"mode":"securities","isExternal":true,"direction":"in","unitPrice":"1"}}),
        json!({"side":"TRANSFER","quantity":"1","asset":{"id":"s"},"transfer":{"mode":"securities","isExternal":true,"direction":"out","unitPrice":"1","costCurrency":"USD"}}),
        json!({"side":"CONVERSION","quantity":"1","asset":{"id":"s"},"conversion":{"toAsset":{"id":"s"}}}),
        json!({"side":"CONVERSION","quantity":"1","asset":{"id":"s"},"conversion":{"toAsset":{"id":"t"},"toQuantity":"-1"}}),
    ] {
        let (service, repo) = setup();
        assert!(
            matches!(
                service
                    .create(operation(fields.clone()), InstructionSource::Ui)
                    .await,
                Err(Error::Validation(_))
            ),
            "{fields}"
        );
        assert!(repo.0.lock().unwrap().is_empty());
    }
}

#[tokio::test]
async fn trade_instruction_transfer_accounts_follow_activity_restrictions() {
    let mut card = create_test_account("card", "USD");
    card.account_type = "CREDIT_CARD".into();
    let mut holdings = create_test_account("holdings", "USD");
    holdings.tracking_mode = crate::accounts::TrackingMode::Holdings;
    let mut inactive = create_test_account("inactive", "USD");
    inactive.is_active = false;
    let mut archived = create_test_account("archived", "USD");
    archived.is_archived = true;
    let (service, repo) = setup_with_accounts(vec![card, holdings, inactive, archived]);
    let cash = |from: &str, to: &str| {
        operation(
            json!({"side":"TRANSFER","accountId":from,"amount":"1","amountCurrency":"USD","transfer":{"mode":"cash","isExternal":false,"toAccountId":to,"destinationCurrency":"USD"}}),
        )
    };
    for (from, to) in [("a", "card"), ("card", "a")] {
        service
            .create(cash(from, to), InstructionSource::Ui)
            .await
            .unwrap();
    }
    for (from, to) in [
        ("a", "holdings"),
        ("holdings", "a"),
        ("a", "inactive"),
        ("archived", "a"),
    ] {
        assert!(service
            .create(cash(from, to), InstructionSource::Ui)
            .await
            .is_err());
    }
    assert!(service
        .create(
            operation(
                json!({"side":"DEPOSIT","accountId":"card","amount":"1","amountCurrency":"USD"})
            ),
            InstructionSource::Ui
        )
        .await
        .is_err());
    assert!(service.create(operation(json!({"side":"TRANSFER","accountId":"a","asset":{"id":"s"},"quantity":"1","transfer":{"mode":"securities","isExternal":false,"toAccountId":"card"}})), InstructionSource::Ui).await.is_err());
    let row = service
        .create(cash("a", "b"), InstructionSource::Ui)
        .await
        .unwrap();
    repo.0
        .lock()
        .unwrap()
        .get_mut(&row.id)
        .unwrap()
        .transfer
        .as_mut()
        .unwrap()
        .to_account_id = Some("inactive".into());
    assert!(service.start(&row.id, row.version).await.is_err());
    let row = service.create(operation(json!({"side":"CONVERSION","asset":{"id":"s"},"quantity":"1","conversion":{"toAsset":{"id":"t"}}})), InstructionSource::Ui).await.unwrap();
    repo.0
        .lock()
        .unwrap()
        .get_mut(&row.id)
        .unwrap()
        .conversion
        .as_mut()
        .unwrap()
        .to_asset_id = "missing".into();
    assert!(service.start(&row.id, row.version).await.is_err());
}

#[tokio::test]
async fn trade_instruction_type_changes_clear_details_and_keep_creation_identity() {
    let (service, _) = setup();
    let original = service.create(operation(json!({"side":"CONVERSION","asset":{"id":"s"},"quantity":"1","conversion":{"toAsset":{"id":"t"}}})), InstructionSource::Ui).await.unwrap();
    let edited = service.edit(&original.id, serde_json::from_value(json!({"expectedVersion":1,"side":"DEPOSIT","accountId":"a","amount":"10","amountCurrency":"USD"})).unwrap()).await.unwrap();
    assert!(edited.asset_id.is_none());
    assert!(edited.asset_snapshot.is_none());
    assert!(edited.conversion.is_none());
    assert!(edited.transfer.is_none());
    assert_eq!(edited.request_hash, original.request_hash);
    assert_eq!(edited.request_id, original.request_id);
}

#[tokio::test]
async fn trade_instruction_legacy_hash_and_retry_survive_new_optional_fields() {
    #[derive(serde::Serialize)]
    #[serde(rename_all = "camelCase")]
    struct LegacyInput {
        goal_id: String,
        request_id: String,
        account_id: String,
        asset: crate::activities::AssetResolutionInput,
        side: InstructionSide,
        #[serde(with = "rust_decimal::serde::str_option")]
        quantity: Option<rust_decimal::Decimal>,
        #[serde(with = "rust_decimal::serde::str_option")]
        amount: Option<rust_decimal::Decimal>,
        amount_currency: Option<String>,
        #[serde(with = "rust_decimal::serde::str_option")]
        limit_price: Option<rust_decimal::Decimal>,
        notes: Option<String>,
        expected_completion_at: Option<chrono::DateTime<Utc>>,
    }
    use sha2::{Digest, Sha256};
    let request = input();
    let legacy = LegacyInput {
        goal_id: request.goal_id.clone(),
        request_id: request.request_id.clone(),
        account_id: request.account_id.clone(),
        asset: request.asset.clone().unwrap(),
        side: request.side,
        quantity: Some("1.23".parse().unwrap()),
        amount: None,
        amount_currency: None,
        limit_price: Some("0.01".parse().unwrap()),
        notes: None,
        expected_completion_at: request.expected_completion_at,
    };
    let hash = hex::encode(Sha256::digest(serde_json::to_vec(&legacy).unwrap()));
    let (service, repo) = setup();
    let first = service
        .create(request.clone(), InstructionSource::Ui)
        .await
        .unwrap();
    assert_eq!(first.request_hash, hash);
    repo.0
        .lock()
        .unwrap()
        .get_mut(&first.id)
        .unwrap()
        .request_hash = hash;
    assert_eq!(
        service
            .create(request, InstructionSource::Ui)
            .await
            .unwrap()
            .id,
        first.id
    );
    assert!(service
        .list(InstructionFilter {
            side: Some(InstructionSide::Buy),
            sides: Some(vec![]),
            ..Default::default()
        })
        .is_err());
}
