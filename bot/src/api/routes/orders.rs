use std::sync::Arc;
use std::time::Duration;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use tokio::sync::oneshot;

use crate::api::dto;
use crate::state::{AppState, BotCommand};

/// How long `place_order` waits for the `ExecutionEvent` to land in
/// `state.orders` before giving up and returning a pending ack. Broker
/// latency on market orders is typically 20–80 ms; this cap is a safety
/// net that almost never triggers in practice.
const EXEC_SETTLE_TIMEOUT: Duration = Duration::from_millis(1_200);

/// Convenience constructor for a JSON error envelope matching the UI's
/// `ApiError` parser (`{ error: { code, message } }`).
fn api_err(
    status: StatusCode,
    code: impl Into<String>,
    message: impl Into<String>,
) -> (StatusCode, Json<dto::ApiErrorBody>) {
    (status, Json(dto::ApiErrorBody::new(code, message)))
}

pub async fn list_orders(State(state): State<Arc<AppState>>) -> Json<Vec<dto::Order>> {
    let orders = state.orders.read().await;
    let out: Vec<dto::Order> = orders
        .iter()
        .map(|o| dto::Order::from_state(o, &state.symbols))
        .collect();
    Json(out)
}

pub async fn place_order(
    State(state): State<Arc<AppState>>,
    Json(req): Json<dto::PlaceOrderRequest>,
) -> Result<Json<dto::Order>, (StatusCode, Json<dto::ApiErrorBody>)> {
    let side_label = req.side.as_internal();
    let symbol_label = req.symbol.clone();
    let args = req.into_internal(&state.symbols).map_err(|e| {
        metrics::counter!(
            "ctrader_bot_orders_rejected_total",
            "reason" => "bad_request",
            "side" => side_label,
            "symbol" => symbol_label.clone(),
        )
        .increment(1);
        api_err(StatusCode::BAD_REQUEST, "bad_request", e)
    })?;

    // Snapshot the order count before dispatch so we can distinguish "the
    // order we just placed" from pre-existing orders. `orders.last()` on
    // its own is unsafe: a buy → sell sequence would echo the buy back on
    // the sell's response.
    let baseline_count = state.orders.read().await.len();

    let (tx, rx) = oneshot::channel();
    state
        .cmd_tx
        .send(BotCommand::PlaceOrder { args, resp: tx })
        .await
        .map_err(|e| {
            api_err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "bot_channel_closed",
                e.to_string(),
            )
        })?;

    match rx.await {
        Ok(Ok(result)) => {
            if !result.accepted {
                metrics::counter!(
                    "ctrader_bot_orders_rejected_total",
                    "reason" => "broker_rejected",
                    "side" => side_label,
                    "symbol" => symbol_label.clone(),
                )
                .increment(1);
                return Err(api_err(
                    StatusCode::BAD_REQUEST,
                    "broker_rejected",
                    result.message,
                ));
            }
            metrics::counter!(
                "ctrader_bot_orders_placed_total",
                "side" => side_label,
                "symbol" => symbol_label.clone(),
            )
            .increment(1);
            // Poll briefly for the concrete Order to appear after the
            // ExecutionEvent. 99% of market orders settle in well under a
            // second; the loop exits early the moment we see one.
            if let Some(order_dto) = wait_for_new_order(&state, baseline_count).await {
                return Ok(Json(order_dto));
            }
            // Broker accepted but we didn't see the ExecutionEvent within
            // the budget. Unusual — WS `execution` frame will still land
            // later. Surface as a structured 202 so the UI can distinguish
            // "pending broker confirmation" from actual failures.
            Err(api_err(
                StatusCode::ACCEPTED,
                "pending_execution",
                "order accepted by broker; waiting on execution event (will arrive via WS)",
            ))
        }
        Ok(Err(e)) => {
            metrics::counter!(
                "ctrader_bot_orders_rejected_total",
                "reason" => "bot_error",
                "side" => side_label,
                "symbol" => symbol_label,
            )
            .increment(1);
            Err(api_err(StatusCode::BAD_REQUEST, "bot_error", e))
        }
        Err(_) => Err(api_err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "bot_channel_dropped",
            "bot loop dropped response",
        )),
    }
}

/// Wait up to `EXEC_SETTLE_TIMEOUT` for `state.orders` to grow past the
/// baseline snapshot, signalling our order's ExecutionEvent landed.
/// Returns the newest order as a DTO, or `None` on timeout.
async fn wait_for_new_order(
    state: &Arc<AppState>,
    baseline_count: usize,
) -> Option<dto::Order> {
    let poll_interval = Duration::from_millis(20);
    let deadline = tokio::time::Instant::now() + EXEC_SETTLE_TIMEOUT;
    loop {
        {
            let orders = state.orders.read().await;
            if orders.len() > baseline_count {
                if let Some(o) = orders.last() {
                    return Some(dto::Order::from_state(o, &state.symbols));
                }
            }
        }
        if tokio::time::Instant::now() >= deadline {
            return None;
        }
        tokio::time::sleep(poll_interval).await;
    }
}

pub async fn cancel_order(
    State(state): State<Arc<AppState>>,
    Path(order_id_str): Path<String>,
) -> Result<Json<dto::Order>, (StatusCode, Json<dto::ApiErrorBody>)> {
    let order_id: i64 = order_id_str.parse().map_err(|_| {
        api_err(StatusCode::BAD_REQUEST, "invalid_order_id", "invalid order id")
    })?;

    let (tx, rx) = oneshot::channel();
    state
        .cmd_tx
        .send(BotCommand::CancelOrder { order_id, resp: tx })
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
            let orders = state.orders.read().await;
            let found = orders.iter().find(|o| o.order_id == order_id);
            match found {
                Some(o) => Ok(Json(dto::Order::from_state(o, &state.symbols))),
                None => Err(api_err(
                    StatusCode::NOT_FOUND,
                    "order_not_found",
                    format!("order {order_id} not found after cancel request"),
                )),
            }
        }
        Ok(Err(e)) => Err(api_err(StatusCode::BAD_REQUEST, "cancel_rejected", e)),
        Err(_) => Err(api_err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "bot_channel_dropped",
            "bot loop dropped response",
        )),
    }
}
