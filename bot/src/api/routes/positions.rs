use crate::state::{AppState, BotCommand, Position};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use std::sync::Arc;
use tokio::sync::oneshot;

pub async fn list_positions(State(state): State<Arc<AppState>>) -> Json<Vec<Position>> {
    let positions = state.positions.read().await.clone();
    Json(positions)
}

#[derive(Deserialize, Default)]
pub struct CloseBody {
    pub volume: Option<f64>,
}

pub async fn close_position(
    State(state): State<Arc<AppState>>,
    Path(position_id): Path<i64>,
    body: Option<Json<CloseBody>>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let volume = body.and_then(|b| b.0.volume);
    let (tx, rx) = oneshot::channel();
    state
        .cmd_tx
        .send(BotCommand::ClosePosition {
            position_id,
            volume,
            resp: tx,
        })
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    match rx.await {
        Ok(Ok(())) => Ok(Json(serde_json::json!({ "accepted": true }))),
        Ok(Err(e)) => Err((StatusCode::BAD_REQUEST, e)),
        Err(_) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            "bot loop dropped response".into(),
        )),
    }
}
