// ws.rs — WebSocket endpoint.
//
// На подключении: snapshot (quotes + positions + status).
// Затем — поток из tick_tx и event_tx через broadcast::Receiver.

use crate::state::{AppState, BotEvent, Tick};
use axum::{
    extract::{
        ws::{Message, WebSocket},
        State, WebSocketUpgrade,
    },
    response::IntoResponse,
};
use serde::Serialize;
use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::debug;

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum WsOut<'a> {
    Snapshot {
        status: &'a str,
        quotes: Vec<crate::state::Quote>,
        positions: Vec<crate::state::Position>,
        orders: Vec<crate::state::Order>,
    },
    Tick(Tick),
    Event(BotEvent),
}

pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(mut socket: WebSocket, state: Arc<AppState>) {
    // Snapshot
    let snapshot = {
        let positions = state.positions.read().await.clone();
        let orders = state.orders.read().await.clone();
        let quotes: Vec<_> = state.quotes.iter().map(|e| e.value().clone()).collect();
        WsOut::Snapshot {
            status: crate::state::status_str(state.status()),
            quotes,
            positions,
            orders,
        }
    };
    if let Ok(text) = serde_json::to_string(&snapshot) {
        if socket.send(Message::Text(text)).await.is_err() {
            return;
        }
    }

    let mut tick_rx = state.tick_tx.subscribe();
    let mut event_rx = state.event_tx.subscribe();

    loop {
        tokio::select! {
            tick = tick_rx.recv() => {
                match tick {
                    Ok(t) => {
                        if let Ok(text) = serde_json::to_string(&WsOut::Tick(t)) {
                            if socket.send(Message::Text(text)).await.is_err() {
                                break;
                            }
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        debug!("WS отстал на {} тиков", n);
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            event = event_rx.recv() => {
                match event {
                    Ok(ev) => {
                        if let Ok(text) = serde_json::to_string(&WsOut::Event(ev)) {
                            if socket.send(Message::Text(text)).await.is_err() {
                                break;
                            }
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => {}
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            incoming = socket.recv() => {
                match incoming {
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Err(_)) => break,
                    _ => {} // ignoring text/ping/pong from client for now
                }
            }
        }
    }
}
