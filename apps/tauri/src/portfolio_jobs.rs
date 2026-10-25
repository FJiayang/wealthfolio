//! Tauri side of portfolio jobs: the coordinator does the work, this module
//! turns its callbacks into the window events the frontend listens to.

use std::sync::Arc;

use log::error;
use tauri::async_runtime::JoinHandle;
use tauri::AppHandle;
use wealthfolio_core::health::HealthServiceTrait;
use wealthfolio_core::portfolio::coordinator::{
    run_periodic_update, AccountFailure, JobObserver, PortfolioJobReport, PortfolioJobRequest,
    RetryPolicy,
};
use wealthfolio_core::quotes::SyncResult;

use crate::context::ServiceContext;
use crate::events::{
    MarketSyncResult, PortfolioRequestPayload, MARKET_SYNC_COMPLETE, MARKET_SYNC_ERROR,
    MARKET_SYNC_START, PORTFOLIO_UPDATE_COMPLETE, PORTFOLIO_UPDATE_ERROR, PORTFOLIO_UPDATE_START,
};

pub struct TauriJobObserver {
    app_handle: AppHandle,
    context: Arc<ServiceContext>,
}

impl TauriJobObserver {
    pub fn new(app_handle: AppHandle, context: Arc<ServiceContext>) -> Self {
        Self {
            app_handle,
            context,
        }
    }

    fn emit<P: serde::Serialize>(&self, event: &str, payload: &P) {
        if let Err(e) =
            crate::events::emit_for_profile(&self.app_handle, &self.context, event, payload)
        {
            error!("Failed to emit {} event: {}", event, e);
        }
    }

    fn clear_health_cache(&self) {
        let health_service = self.context.health_service();
        tauri::async_runtime::spawn(async move {
            health_service.clear_cache().await;
        });
    }
}

impl JobObserver for TauriJobObserver {
    fn enrichment_started(&self, total: usize) {
        self.emit(
            crate::events::ASSET_ENRICHMENT_START,
            &serde_json::json!({ "total": total }),
        );
    }
    fn enrichment_progress(&self, completed: usize, total: usize) {
        self.emit(
            crate::events::ASSET_ENRICHMENT_PROGRESS,
            &serde_json::json!({ "completed": completed, "total": total }),
        );
    }
    fn enrichment_completed(&self, enriched: usize, skipped: usize, failed: usize) {
        self.emit(
            crate::events::ASSET_ENRICHMENT_COMPLETE,
            &serde_json::json!({ "enriched": enriched, "skipped": skipped, "failed": failed }),
        );
    }

    fn market_sync_started(&self) {
        self.emit(MARKET_SYNC_START, &());
    }

    fn market_sync_completed(&self, result: &SyncResult) {
        self.clear_health_cache();
        self.emit(
            MARKET_SYNC_COMPLETE,
            &MarketSyncResult {
                failed_syncs: result.failures.clone(),
                skipped_reasons: result
                    .skipped_reasons
                    .iter()
                    .map(|(asset_id, reason)| (asset_id.clone(), reason.to_string()))
                    .collect(),
                show_skipped_reasons: false,
            },
        );
    }

    fn market_sync_failed(&self, message: &str) {
        self.emit(MARKET_SYNC_ERROR, &message.to_string());
    }

    fn update_started(&self) {
        self.emit(PORTFOLIO_UPDATE_START, &());
    }

    fn update_failed(&self, failure: &AccountFailure) {
        if failure.code == "INVALID_SNAPSHOT_DATE" {
            self.clear_health_cache();
        }
        if failure.account_id.is_empty() {
            self.emit(PORTFOLIO_UPDATE_ERROR, &failure.message);
        } else {
            self.emit(PORTFOLIO_UPDATE_ERROR, failure);
        }
    }

    fn update_completed(&self) {
        // Freshness verdicts and stale-projection issues are cached.
        self.clear_health_cache();
        self.emit(PORTFOLIO_UPDATE_COMPLETE, &());
    }
}

/// One portfolio job from a frontend/planner payload.
pub async fn run_portfolio_request(
    app_handle: &AppHandle,
    context: &Arc<ServiceContext>,
    payload: PortfolioRequestPayload,
) -> Option<PortfolioJobReport> {
    let request = PortfolioJobRequest {
        account_ids: payload.account_ids,
        market_sync: payload.market_sync_mode,
        force_full: payload.force_full,
    };
    let observer = TauriJobObserver::new(app_handle.clone(), Arc::clone(context));
    match context
        .portfolio_coordinator()
        .run_job_with_retry(request, &observer, RetryPolicy::default())
        .await
    {
        Ok(report) => Some(report),
        Err(err) => {
            error!("Portfolio job failed: {}", err);
            None
        }
    }
}

/// Check the daily market-data schedule while this profile is active.
pub fn spawn_periodic_update(
    app_handle: AppHandle,
    context: Arc<ServiceContext>,
) -> JoinHandle<()> {
    let observer: Arc<dyn JobObserver> =
        Arc::new(TauriJobObserver::new(app_handle, Arc::clone(&context)));
    tauri::async_runtime::spawn(run_periodic_update(
        context.portfolio_coordinator(),
        observer,
        context.settings_service(),
        context.asset_service(),
    ))
}

/// Startup and resume use the same persisted daily admission as the worker.
pub async fn check_market_data_update(
    handle: AppHandle,
    context: Arc<ServiceContext>,
) -> Result<(), String> {
    let observer = TauriJobObserver::new(handle, context.clone());
    let coordinator = context.portfolio_coordinator();
    let report = coordinator
        .update_if_due(
            context.settings_service().as_ref(),
            context.asset_service().as_ref(),
            &observer,
        )
        .await
        .map_err(|error| {
            observer.market_sync_failed(&error.to_string());
            error.to_string()
        })?;
    if report.is_none() {
        coordinator
            .update_all(wealthfolio_core::quotes::MarketSyncMode::None, &observer)
            .await;
    }
    Ok(())
}

pub fn request_scheduled_update(handle: AppHandle, context: Arc<ServiceContext>) {
    if !context.is_active() {
        return;
    }
    let task_context = context.clone();
    context.portfolio_tasks.spawn(false, async move {
        check_market_data_update(handle, task_context).await.is_ok()
    });
}
