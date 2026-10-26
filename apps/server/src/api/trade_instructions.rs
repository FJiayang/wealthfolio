use crate::{error::ApiResult, main_lib::AppState};
use axum::{
    extract::Path,
    routing::{get as get_route, post},
    Extension, Json, Router,
};
use std::sync::Arc;
use wealthfolio_core::trade_instructions::*;

async fn create(
    Extension(state): Extension<Arc<AppState>>,
    Json(input): Json<NewTradeInstruction>,
) -> ApiResult<Json<TradeInstruction>> {
    Ok(Json(
        state
            .trade_instruction_service
            .create(input, InstructionSource::Ui)
            .await?,
    ))
}

async fn list(
    Extension(state): Extension<Arc<AppState>>,
    Json(filter): Json<InstructionFilter>,
) -> ApiResult<Json<InstructionPage>> {
    Ok(Json(state.trade_instruction_service.list(filter)?))
}

async fn get(
    Extension(state): Extension<Arc<AppState>>,
    Path(id): Path<String>,
) -> ApiResult<Json<TradeInstruction>> {
    Ok(Json(state.trade_instruction_service.get(&id)?))
}

async fn start(
    Extension(state): Extension<Arc<AppState>>,
    Path(id): Path<String>,
    Json(input): Json<InstructionVersion>,
) -> ApiResult<Json<TradeInstruction>> {
    Ok(Json(
        state
            .trade_instruction_service
            .start(&id, input.expected_version)
            .await?,
    ))
}

async fn complete(
    Extension(state): Extension<Arc<AppState>>,
    Path(id): Path<String>,
    Json(input): Json<CompleteInstruction>,
) -> ApiResult<Json<TradeInstruction>> {
    Ok(Json(
        state.trade_instruction_service.complete(&id, input).await?,
    ))
}

async fn cancel(
    Extension(state): Extension<Arc<AppState>>,
    Path(id): Path<String>,
    Json(input): Json<CancelInstruction>,
) -> ApiResult<Json<TradeInstruction>> {
    Ok(Json(
        state.trade_instruction_service.cancel(&id, input).await?,
    ))
}

async fn correct_completion_time(
    Extension(state): Extension<Arc<AppState>>,
    Path(id): Path<String>,
    Json(input): Json<CorrectCompletionTime>,
) -> ApiResult<Json<TradeInstruction>> {
    Ok(Json(
        state
            .trade_instruction_service
            .correct_completion_time(&id, input)
            .await?,
    ))
}

pub fn router<S: Clone + Send + Sync + 'static>() -> Router<S> {
    Router::new()
        .route("/trade-instructions", post(create))
        .route("/trade-instructions/list", post(list))
        .route("/trade-instructions/{id}", get_route(get))
        .route("/trade-instructions/{id}/start", post(start))
        .route("/trade-instructions/{id}/complete", post(complete))
        .route("/trade-instructions/{id}/cancel", post(cancel))
        .route(
            "/trade-instructions/{id}/completion-time",
            post(correct_completion_time),
        )
}
