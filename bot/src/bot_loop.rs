// bot_loop.rs — основной event loop бота.
//
// Один select!:
//   1) входящие сообщения с cTrader
//   2) команды от API
//   3) heartbeat по таймеру

use crate::connection::CTraderConnection;
use crate::state::{
    f64_to_proto_volume, order_from_proto, parse_order_type, parse_side, position_from_proto,
    proto, proto_price_to_f64, AppState, BotCommand, BotEvent, PlaceOrderArgs, PlaceOrderResult,
    Position, Quote, Tick, STATUS_CONNECTED, STATUS_DISCONNECTED,
};
use anyhow::Result;
use prost::Message;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

const SUBSCRIBE_SYMBOL_ID: i64 = 1311; // ETHEREUM

fn wrap(payload_type: u32, inner: &impl Message) -> proto::ProtoMessage {
    proto::ProtoMessage {
        payload_type,
        payload: Some(inner.encode_to_vec()),
        client_msg_id: None,
    }
}

/// Первоначальная синхронизация состояния: позиции и pending ордера.
pub async fn initial_reconcile(
    conn: &mut CTraderConnection,
    state: &Arc<AppState>,
) -> Result<()> {
    debug!("Запрос ProtoOAReconcileReq");

    let req = proto::ProtoOaReconcileReq {
        payload_type: Some(proto::ProtoOaPayloadType::ProtoOaReconcileReq as i32),
        ctid_trader_account_id: state.account_id,
        return_protection_orders: Some(false),
    };
    let envelope = wrap(proto::ProtoOaPayloadType::ProtoOaReconcileReq as u32, &req);
    conn.send(&envelope).await?;

    // Читаем сообщения, пока не придёт ProtoOaReconcileRes (может прийти heartbeat/spot до него)
    loop {
        let raw = conn.recv_raw().await?;
        let msg = proto::ProtoMessage::decode(raw.as_slice())?;
        if msg.payload_type == proto::ProtoOaPayloadType::ProtoOaReconcileRes as u32 {
            if let Some(ref payload) = msg.payload {
                let res = proto::ProtoOaReconcileRes::decode(payload.as_slice())?;
                let positions: Vec<Position> =
                    res.position.iter().map(position_from_proto).collect();
                let orders: Vec<_> = res.order.iter().map(order_from_proto).collect();
                info!(
                    "Reconcile: {} позиций, {} ордеров",
                    positions.len(),
                    orders.len()
                );
                *state.positions.write().await = positions;
                *state.orders.write().await = orders;
            }
            return Ok(());
        }
        // Промежуточные сообщения просто обрабатываем как обычно
        handle_incoming(&msg, state, conn).await?;
    }
}

pub async fn subscribe_to_spots(
    conn: &mut CTraderConnection,
    state: &Arc<AppState>,
) -> Result<()> {
    let req = proto::ProtoOaSubscribeSpotsReq {
        payload_type: Some(proto::ProtoOaPayloadType::ProtoOaSubscribeSpotsReq as i32),
        ctid_trader_account_id: state.account_id,
        symbol_id: vec![SUBSCRIBE_SYMBOL_ID],
        subscribe_to_spot_timestamp: Some(true),
    };
    let envelope = wrap(
        proto::ProtoOaPayloadType::ProtoOaSubscribeSpotsReq as u32,
        &req,
    );
    conn.send(&envelope).await?;
    info!("Подписка на spot events отправлена (symbol_id={})", SUBSCRIBE_SYMBOL_ID);
    Ok(())
}

/// Главный event loop. Владеет соединением, получает команды через cmd_rx.
pub async fn run(
    mut conn: CTraderConnection,
    state: Arc<AppState>,
    mut cmd_rx: mpsc::Receiver<BotCommand>,
) -> Result<()> {
    state.set_status(STATUS_CONNECTED);
    let _ = state.event_tx.send(BotEvent::ConnectionStatusChanged {
        status: "connected".into(),
    });

    let mut heartbeat = tokio::time::interval(Duration::from_secs(10));

    loop {
        tokio::select! {
            // входящее сообщение
            raw = conn.recv_raw() => {
                let raw = match raw {
                    Ok(r) => r,
                    Err(e) => {
                        warn!("Соединение оборвано: {}", e);
                        state.set_status(STATUS_DISCONNECTED);
                        let _ = state.event_tx.send(BotEvent::ConnectionStatusChanged {
                            status: "disconnected".into(),
                        });
                        return Err(e);
                    }
                };
                let msg = proto::ProtoMessage::decode(raw.as_slice())?;
                handle_incoming(&msg, &state, &mut conn).await?;
            }

            // команда от API
            Some(cmd) = cmd_rx.recv() => {
                handle_command(cmd, &state, &mut conn).await;
            }

            // heartbeat
            _ = heartbeat.tick() => {
                let pong = proto::ProtoHeartbeatEvent {
                    payload_type: Some(proto::ProtoPayloadType::HeartbeatEvent as i32),
                };
                let envelope = wrap(proto::ProtoPayloadType::HeartbeatEvent as u32, &pong);
                if let Err(e) = conn.send(&envelope).await {
                    warn!("Не удалось отправить heartbeat: {}", e);
                }
            }
        }
    }
}

async fn handle_incoming(
    msg: &proto::ProtoMessage,
    state: &Arc<AppState>,
    conn: &mut CTraderConnection,
) -> Result<()> {
    let pt = msg.payload_type;

    if pt == proto::ProtoPayloadType::HeartbeatEvent as u32 {
        // cервер пингует — отвечаем
        let pong = proto::ProtoHeartbeatEvent {
            payload_type: Some(proto::ProtoPayloadType::HeartbeatEvent as i32),
        };
        let envelope = wrap(proto::ProtoPayloadType::HeartbeatEvent as u32, &pong);
        conn.send(&envelope).await?;
    } else if pt == proto::ProtoOaPayloadType::ProtoOaSpotEvent as u32 {
        if let Some(payload) = msg.payload.as_ref() {
            let spot = proto::ProtoOaSpotEvent::decode(payload.as_slice())?;
            let bid = spot.bid.map(proto_price_to_f64);
            let ask = spot.ask.map(proto_price_to_f64);
            let quote = Quote {
                symbol_id: spot.symbol_id,
                bid,
                ask,
                timestamp_ms: spot.timestamp,
            };
            state.quotes.insert(spot.symbol_id, quote);
            let _ = state.tick_tx.send(Tick {
                symbol_id: spot.symbol_id,
                bid,
                ask,
                timestamp_ms: spot.timestamp,
            });
        }
    } else if pt == proto::ProtoOaPayloadType::ProtoOaExecutionEvent as u32 {
        if let Some(payload) = msg.payload.as_ref() {
            let exec = proto::ProtoOaExecutionEvent::decode(payload.as_slice())?;
            apply_execution(&exec, state).await;
            let _ = state.event_tx.send(BotEvent::Execution {
                execution_type: format!("{:?}", exec.execution_type),
                position_id: exec.position.as_ref().map(|p| p.position_id),
                order_id: exec.order.as_ref().map(|o| o.order_id),
            });
        }
    } else if pt == proto::ProtoOaPayloadType::ProtoOaOrderErrorEvent as u32 {
        if let Some(payload) = msg.payload.as_ref() {
            let err = proto::ProtoOaOrderErrorEvent::decode(payload.as_slice())?;
            warn!(
                "OrderError: {} — {}",
                err.error_code,
                err.description.clone().unwrap_or_default()
            );
            let _ = state.event_tx.send(BotEvent::OrderError {
                error_code: err.error_code,
                description: err.description.unwrap_or_default(),
                order_id: err.order_id,
                position_id: err.position_id,
            });
        }
    } else if pt == proto::ProtoOaPayloadType::ProtoOaErrorRes as u32 {
        if let Some(payload) = msg.payload.as_ref() {
            let err = proto::ProtoOaErrorRes::decode(payload.as_slice())?;
            warn!(
                "OA error: {:?} — {}",
                err.error_code,
                err.description.clone().unwrap_or_default()
            );
        }
    } else {
        debug!("Неизвестный payload_type={}", pt);
    }

    Ok(())
}

async fn apply_execution(exec: &proto::ProtoOaExecutionEvent, state: &Arc<AppState>) {
    // Обновляем позицию/ордер в state по факту execution event.
    if let Some(pos) = exec.position.as_ref() {
        let updated = position_from_proto(pos);
        let mut positions = state.positions.write().await;
        if let Some(existing) = positions.iter_mut().find(|p| p.position_id == updated.position_id)
        {
            *existing = updated;
        } else {
            positions.push(updated);
        }
        // Если позиция закрыта — удаляем.
        if pos.position_status == proto::ProtoOaPositionStatus::PositionStatusClosed as i32 {
            positions.retain(|p| p.position_id != pos.position_id);
        }
    }

    if let Some(order) = exec.order.as_ref() {
        let updated = order_from_proto(order);
        let mut orders = state.orders.write().await;
        if let Some(existing) = orders.iter_mut().find(|o| o.order_id == updated.order_id) {
            *existing = updated;
        } else {
            orders.push(updated);
        }
        // Убираем исполненные/отменённые/просроченные ордера.
        let st = order.order_status;
        if st == proto::ProtoOaOrderStatus::OrderStatusFilled as i32
            || st == proto::ProtoOaOrderStatus::OrderStatusCancelled as i32
            || st == proto::ProtoOaOrderStatus::OrderStatusExpired as i32
            || st == proto::ProtoOaOrderStatus::OrderStatusRejected as i32
        {
            orders.retain(|o| o.order_id != order.order_id);
        }
    }
}

async fn handle_command(
    cmd: BotCommand,
    state: &Arc<AppState>,
    conn: &mut CTraderConnection,
) {
    match cmd {
        BotCommand::PlaceOrder { args, resp } => {
            let result = place_order(args, state, conn).await;
            let _ = resp.send(result);
        }
        BotCommand::CancelOrder { order_id, resp } => {
            let req = proto::ProtoOaCancelOrderReq {
                payload_type: Some(proto::ProtoOaPayloadType::ProtoOaCancelOrderReq as i32),
                ctid_trader_account_id: state.account_id,
                order_id,
            };
            let envelope = wrap(
                proto::ProtoOaPayloadType::ProtoOaCancelOrderReq as u32,
                &req,
            );
            let out = conn.send(&envelope).await.map_err(|e| e.to_string());
            let _ = resp.send(out);
        }
        BotCommand::ClosePosition {
            position_id,
            volume,
            resp,
        } => {
            // Если volume не задан — закрываем всю позицию.
            let vol_proto = match volume {
                Some(v) => f64_to_proto_volume(v),
                None => {
                    let positions = state.positions.read().await;
                    match positions.iter().find(|p| p.position_id == position_id) {
                        Some(p) => f64_to_proto_volume(p.volume),
                        None => {
                            let _ = resp.send(Err("position not found".into()));
                            return;
                        }
                    }
                }
            };
            let req = proto::ProtoOaClosePositionReq {
                payload_type: Some(proto::ProtoOaPayloadType::ProtoOaClosePositionReq as i32),
                ctid_trader_account_id: state.account_id,
                position_id,
                volume: vol_proto,
            };
            let envelope = wrap(
                proto::ProtoOaPayloadType::ProtoOaClosePositionReq as u32,
                &req,
            );
            let out = conn.send(&envelope).await.map_err(|e| e.to_string());
            let _ = resp.send(out);
        }
    }
}

async fn place_order(
    args: PlaceOrderArgs,
    state: &Arc<AppState>,
    conn: &mut CTraderConnection,
) -> Result<PlaceOrderResult, String> {
    let side = parse_side(&args.side).ok_or_else(|| format!("invalid side: {}", args.side))?;
    let order_type = parse_order_type(&args.order_type)
        .ok_or_else(|| format!("invalid order_type: {}", args.order_type))?;

    let req = proto::ProtoOaNewOrderReq {
        payload_type: Some(proto::ProtoOaPayloadType::ProtoOaNewOrderReq as i32),
        ctid_trader_account_id: state.account_id,
        symbol_id: args.symbol_id,
        order_type: order_type as i32,
        trade_side: side as i32,
        volume: f64_to_proto_volume(args.volume),
        limit_price: args.limit_price,
        stop_price: args.stop_price,
        time_in_force: None,
        expiration_timestamp: None,
        stop_loss: args.stop_loss,
        take_profit: args.take_profit,
        comment: None,
        base_slippage_price: None,
        slippage_in_points: None,
        label: None,
        position_id: None,
        client_order_id: None,
        relative_stop_loss: None,
        relative_take_profit: None,
        guaranteed_stop_loss: None,
        trailing_stop_loss: None,
        stop_trigger_method: None,
    };
    let envelope = wrap(proto::ProtoOaPayloadType::ProtoOaNewOrderReq as u32, &req);
    conn.send(&envelope).await.map_err(|e| e.to_string())?;

    Ok(PlaceOrderResult {
        accepted: true,
        message: "order sent to server".into(),
    })
}
