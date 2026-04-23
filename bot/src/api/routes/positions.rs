use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use tokio::sync::oneshot;

use crate::api::dto;
use crate::state::{AppState, BotCommand};

/// Convenience constructor for a JSON error envelope matching the UI's
/// `ApiError` parser (`{ error: { code, message } }`).
fn api_err(
    status: StatusCode,
    code: impl Into<String>,
    message: impl Into<String>,
) -> (StatusCode, Json<dto::ApiErrorBody>) {
    (status, Json(dto::ApiErrorBody::new(code, message)))
}

pub async fn list_positions(State(state): State<Arc<AppState>>) -> Json<Vec<dto::Position>> {
    let positions = state.positions.read().await;
    let out: Vec<dto::Position> = positions
        .iter()
        .map(|p| dto::Position::from_state(p, &state.symbols))
        .collect();
    Json(out)
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CloseBody {
    pub volume: Option<f64>,
}

pub async fn close_position(
    State(state): State<Arc<AppState>>,
    Path(position_id_str): Path<String>,
    body: Option<Json<CloseBody>>,
) -> Result<Json<dto::Position>, (StatusCode, Json<dto::ApiErrorBody>)> {
    let position_id: i64 = position_id_str.parse().map_err(|_| {
        api_err(
            StatusCode::BAD_REQUEST,
            "invalid_position_id",
            "invalid position id",
        )
    })?;

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
        .map_err(|e| {
            api_err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "bot_channel_closed",
                e.to_string(),
            )
        })?;

    match rx.await {
        Ok(Ok(())) => {
            // Return the current snapshot of the position (if still open) so the
            // UI mutation hook can confirm acceptance. The bot broadcasts the
            // actual `position_closed` via WS once the broker confirms.
            let positions = state.positions.read().await;
            let found = positions.iter().find(|p| p.position_id == position_id);
            match found {
                Some(p) => Ok(Json(dto::Position::from_state(p, &state.symbols))),
                None => Err(api_err(
                    StatusCode::NOT_FOUND,
                    "position_not_found",
                    format!("position {position_id} not found after close request"),
                )),
            }
        }
        Ok(Err(e)) => Err(api_err(StatusCode::BAD_REQUEST, "close_rejected", e)),
        Err(_) => Err(api_err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "bot_channel_dropped",
            "bot loop dropped response",
        )),
    }
}
