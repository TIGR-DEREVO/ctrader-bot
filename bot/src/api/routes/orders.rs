use crate::state::{AppState, BotCommand, Order, PlaceOrderArgs};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use std::sync::Arc;
use tokio::sync::oneshot;

pub async fn list_orders(State(state): State<Arc<AppState>>) -> Json<Vec<Order>> {
    let orders = state.orders.read().await.clone();
    Json(orders)
}

pub async fn place_order(
    State(state): State<Arc<AppState>>,
    Json(args): Json<PlaceOrderArgs>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let (tx, rx) = oneshot::channel();
    state
        .cmd_tx
        .send(BotCommand::PlaceOrder { args, resp: tx })
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    match rx.await {
        Ok(Ok(result)) => Ok(Json(serde_json::json!({
            "accepted": result.accepted,
            "message": result.message,
        }))),
        Ok(Err(e)) => Err((StatusCode::BAD_REQUEST, e)),
        Err(_) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            "bot loop dropped response".into(),
        )),
    }
}

pub async fn cancel_order(
    State(state): State<Arc<AppState>>,
    Path(order_id): Path<i64>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let (tx, rx) = oneshot::channel();
    state
        .cmd_tx
        .send(BotCommand::CancelOrder { order_id, resp: tx })
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
