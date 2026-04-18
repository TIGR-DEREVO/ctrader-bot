# Plan: Headless API for cTrader Bot

## Context

The bot currently connects to cTrader, authenticates, subscribes to EUR/USD ticks, and logs them. Everything runs in a single sequential flow. To build a desktop GUI (future Tauri + React), we first need a headless API layer that exposes the bot's state and accepts commands. This plan adds an **Axum REST + WebSocket server** running alongside the bot event loop.

## Architecture

```
┌───────────────────────────────────────────────────┐
│                  ctrader-bot process               │
│                                                    │
│  ┌───────────┐    cmd_tx (mpsc)   ┌────────────┐  │
│  │ Axum API  │ ────────────────> │  Bot Loop   │  │
│  │ :3000     │ <──────────────── │  (owns conn)│  │
│  │ REST + WS │  broadcast chans  │             │  │
│  └─────┬─────┘                   └──────┬──────┘  │
│        │       reads/writes             │          │
│        └──────────┬─────────────────────┘          │
│                   v                                │
│        ┌─────────────────────┐                     │
│        │   Arc<AppState>     │                     │
│        │  quotes: DashMap    │                     │
│        │  positions: RwLock  │                     │
│        │  tick_tx: broadcast │                     │
│        │  cmd_tx: mpsc       │                     │
│        └─────────────────────┘                     │
└────────────────────────────────────────────────────┘
```

## Implementation Steps

### Step 1: Add dependencies to `bot/Cargo.toml`

New deps: `axum`, `tower-http` (cors), `dashmap`, `tokio` (add `sync` features)

### Step 2: Create shared state module — `bot/src/state.rs`

- `AppState` struct: quotes (DashMap), positions (RwLock), connection_status (AtomicU8)
- `Tick`, `Quote`, `Position`, `Order`, `AccountInfo` — JSON-serializable domain types
- `BotCommand` enum (PlaceOrder, CancelOrder, ClosePosition — each with oneshot response channel)
- `BotEvent` enum (ExecutionEvent, OrderError, ConnectionStatusChanged)
- Price conversion helpers: `proto_price_to_f64()`, `proto_volume_to_f64()`
- Broadcast channels: `tick_tx`, `event_tx`
- Command channel: `cmd_tx`

### Step 3: Refactor event loop into `bot/src/bot_loop.rs` with `tokio::select!`

Replace the current sequential `strategy.rs` event loop with a `select!`-based loop that handles:
- **Arm 1**: `conn.recv_raw()` — incoming cTrader messages (ticks, heartbeats, executions, errors)
- **Arm 2**: `cmd_rx.recv()` — commands from API (place order, cancel, etc.)
- **Arm 3**: `heartbeat_interval.tick()` — proactive heartbeat every 10s to prevent timeout

On tick: update `state.quotes`, broadcast on `tick_tx`
On execution: update `state.positions`, broadcast on `event_tx`
On command: construct protobuf request, send via `conn.send()`

### Step 4: Create API module — `bot/src/api/`

```
bot/src/api/
├── mod.rs          # build_router()
├── ws.rs           # WebSocket handler
└── routes/
    ├── mod.rs
    ├── status.rs   # GET /api/status
    ├── quotes.rs   # GET /api/quotes
    ├── positions.rs# GET /api/positions, POST /api/positions/:id/close
    └── orders.rs   # POST /api/orders, DELETE /api/orders/:id
```

**REST endpoints:**
| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/status` | Connection status, uptime |
| GET | `/api/quotes` | All latest bid/ask quotes |
| GET | `/api/positions` | Open positions with P&L |
| POST | `/api/orders` | Place order (→ BotCommand) |
| DELETE | `/api/orders/:id` | Cancel order |
| POST | `/api/positions/:id/close` | Close position |
| GET | `/ws` | WebSocket stream (ticks + events) |

**WebSocket**: On connect sends snapshot (quotes + positions), then streams ticks and events via `broadcast::Receiver`. Client can filter with `{"action":"subscribe","channels":["ticks"]}`.

### Step 5: Rewire `bot/src/main.rs`

```rust
#[tokio::main]
async fn main() -> Result<()> {
    // 1. Load config, init logging
    // 2. Connect + authenticate (existing code)
    // 3. Create AppState + channels
    // 4. tokio::spawn(bot_loop) — owns connection + cmd_rx
    // 5. tokio::spawn(axum_server) — serves on :3000
    // 6. select! { bot_handle, api_handle } — wait for either
}
```

### Step 6: Initial reconcile on startup

After auth, send `ProtoOAReconcileReq` to fetch open positions and pending orders, populate `AppState`. This ensures GET /api/positions returns data immediately.

## Files Modified
- `bot/Cargo.toml` — add axum, tower-http, dashmap
- `bot/src/main.rs` — rewrite to spawn bot_loop + API
- `bot/src/strategy.rs` → `bot/src/bot_loop.rs` — select!-based event loop

## Files Created
- `bot/src/state.rs` — AppState, domain types, channels, BotCommand/BotEvent
- `bot/src/api/mod.rs` — router
- `bot/src/api/ws.rs` — WebSocket handler
- `bot/src/api/routes/mod.rs`
- `bot/src/api/routes/status.rs`
- `bot/src/api/routes/quotes.rs`
- `bot/src/api/routes/positions.rs`
- `bot/src/api/routes/orders.rs`

## Verification

1. `cargo build` — compiles clean
2. `cargo run` — connects to cTrader, ticks flow, Axum starts on :3000
3. `curl localhost:3000/api/status` — returns `{"status":"connected"}`
4. `curl localhost:3000/api/quotes` — returns latest EUR/USD bid/ask
5. `websocat ws://localhost:3000/ws` — streams live tick JSON
6. Heartbeat keeps working (no disconnect after 30s)
