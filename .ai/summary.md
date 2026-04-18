# cTrader Algo Trading Bot — Project Summary

## Overview

Building an algorithmic trading bot in **Rust** connected to **FxPro** via **cTrader Open API** (Protobuf/TCP).

---

## Broker: FxPro

- Regulated by FCA (UK), CySEC, FSCA
- Headquartered in London, servers in EU
- Minimum deposit: **$100** (recommended $1,000 for comfortable trading)
- cTrader account commission: **$35 per $1M traded** (≈ $3.5 per lot per side)
- Spread on EUR/USD (cTrader): from **0.2 pips**
- Leverage (UK/FCA retail): **1:30**

---

## API Options (evaluated)

| API | Protocol | Min Deposit | Notes |
|---|---|---|---|
| **cTrader Open API** | Protobuf/TCP | $100 | ✅ Best for this project |
| FIX API 4.4 | FIX | $5,000+ | Institutional, requires broker approval |
| MetaTrader 5 | Python lib | $100 | Requires Windows/Wine, not recommended |
| MetaTrader 4 | MQL4 / ZeroMQ | $100 | No native external API, avoid |

**Decision: cTrader Open API** — no terminal required, native async, works on Linux, accessible from $100.

---

## Language Choice

**Rust** — chosen for maximum performance.

| Language | Pros | Cons |
|---|---|---|
| **Rust** ✅ | No GC pauses, predictable latency, memory safe | Steeper learning curve |
| C# | Native cTrader language (cAlgo), fast dev | GC pauses under load |
| Python | Best for ML/backtesting | Too slow for execution engine |
| C++ | Maximum HFT speed | Unsafe, complex |

---

## Architecture

```
┌─────────────────────────────────────┐
│         Rust Core (tokio async)     │
│                                     │
│  TCP/TLS ──▶ Auth ──▶ Strategy     │
│  connector    layer    engine       │
│                  │                  │
│          Order Manager              │
│         (risk checks)               │
└──────────────┬──────────────────────┘
               │ Protobuf/TCP
       cTrader Open API
       demo.ctraderapi.com:5035
               │
┌──────────────▼──────────────────────┐
│  Python sidecar (future / optional) │
│  ML models, backtesting, research   │
│  Connected via PyO3                 │
└─────────────────────────────────────┘
               │
┌──────────────▼──────────────────────┐
│  Observability                      │
│  TimescaleDB ← trades & metrics     │
│  Grafana + Alertmanager             │
└─────────────────────────────────────┘
```

---

## Tech Stack

| Component | Crate |
|---|---|
| Async runtime | `tokio` |
| Protobuf | `prost` + `prost-build` |
| TLS (pure Rust, no OpenSSL) | `tokio-rustls` + `rustls` |
| Logging & tracing | `tracing` + `tracing-subscriber` |
| Config (.env) | `dotenvy` |
| Error handling | `anyhow` + `thiserror` |
| Byte buffers | `bytes` |
| Future: backtesting | `polars` (faster than pandas) |
| Future: Rust+Python bridge | `PyO3` + `maturin` |

---

## Project Structure

```
ctrader-bot/
├── Cargo.toml          # dependencies
├── build.rs            # compiles .proto → Rust structs
├── .env                # credentials (never commit!)
├── .env.example        # template
├── proto/              # Spotware .proto files (download separately)
│   ├── OpenApiCommonMessages.proto
│   ├── OpenApiMessages.proto
│   └── OpenApiModelMessages.proto
├── src/
│   ├── main.rs         # entry point, config loading
│   ├── connection.rs   # TCP/TLS connector, frame read/write
│   ├── auth.rs         # two-step authentication
│   └── strategy.rs     # trading logic (stub → implement here)
└── README.md
```

---

## cTrader Open API Protocol

Messages are framed as:
```
┌──────────────┬─────────────────────┐
│  4 bytes     │  N bytes            │
│  body length │  Protobuf message   │
└──────────────┴─────────────────────┘
```

### Authentication Flow (2 steps)

```
Client                        cTrader API
  │                               │
  │── ProtoOAApplicationAuthReq ──▶│  (clientId + clientSecret)
  │◀─ ProtoOAApplicationAuthRes ──│
  │                               │
  │── ProtoOAAccountAuthReq ──────▶│  (accessToken + accountId)
  │◀─ ProtoOAAccountAuthRes ──────│
  │                               │
  │   ✅ Ready to trade           │
```

### Key Message Types

| Message | Direction | Purpose |
|---|---|---|
| `ProtoOAApplicationAuthReq/Res` | → / ← | App-level auth |
| `ProtoOAAccountAuthReq/Res` | → / ← | Account-level auth |
| `ProtoOASubscribeSpotsReq` | → | Subscribe to live ticks |
| `ProtoOASpotEvent` | ← | Tick update (bid/ask) |
| `ProtoOANewOrderReq` | → | Place order |
| `ProtoOAExecutionEvent` | ← | Order filled/rejected |
| `ProtoHeartbeatEvent` | ↔ | Keepalive every 10s |

⚠️ **Heartbeat is mandatory** — server disconnects if no activity for ~30s.

---

## Environment Variables (.env)

```bash
CTRADER_CLIENT_ID=your_client_id
CTRADER_CLIENT_SECRET=your_client_secret
CTRADER_ACCESS_TOKEN=your_access_token   # from OAuth flow
CTRADER_ACCOUNT_ID=10634940             # your demo account
CTRADER_HOST=demo.ctraderapi.com        # use live.ctraderapi.com for real money
CTRADER_PORT=5035
RUST_LOG=ctrader_bot=debug
```

---

## Account Setup Status

| Step | Status |
|---|---|
| FxPro account | ✅ Existing |
| cTrader Demo account created | ✅ Account #10634940 |
| Demo balance | ✅ £10,000 virtual |
| Application registered on open.ctrader.com | ✅ App "monkey" (ID 23571) |
| Application status | ⏳ "Submitted" → waiting KYC (~3 days) |
| clientId + clientSecret | ✅ In .env |
| accessToken | ⏳ Waiting for "Active" status |
| Rust project skeleton | ✅ Created |

---

## Third-party Libraries Evaluated

### `raul-gherman/ctrader-openapi-rust`
- ⚠️ **Not recommended for production**
- Only 3 stars, 5 commits, 3 files total
- Uses unknown custom crates (`quack-builder`, `quacky`) instead of standard `prost`
- No CI, no tests, no examples
- Useful only as a reference for proto struct names

### `geminik23/ctrader-fix`
- ✅ **Quality library**, 35+ implemented features
- Full `MarketClient` + `TradeClient` (orders, positions, heartbeat)
- ❌ **FIX API only** — requires $5,000+ deposit + broker approval
- Revisit when/if scaling to FIX API

---

## Next 3 Days (while waiting for KYC)

### Day 1 — Build the project
```bash
# Download .proto files from Spotware
cd ctrader-bot/proto
curl -O https://raw.githubusercontent.com/spotware/openapi-proto-messages/master/OpenApiCommonMessages.proto
curl -O https://raw.githubusercontent.com/spotware/openapi-proto-messages/master/OpenApiMessages.proto
curl -O https://raw.githubusercontent.com/spotware/openapi-proto-messages/master/OpenApiModelMessages.proto

# Build (resolves compile errors before token is ready)
mkdir -p ../src/proto && cd ..
cargo build
```

### Day 2 — Read the docs
- [help.ctrader.com/open-api](https://help.ctrader.com/open-api) — full API reference
- Focus on: `ProtoOASubscribeSpotsReq`, `ProtoOASpotEvent`, `ProtoHeartbeatEvent`

### Day 3 — Implement `handle_message()`
The message dispatcher — routes incoming server messages to handlers:
```rust
match payload_type {
    ProtoOASpotEvent        => on_tick(bid, ask),
    ProtoHeartbeatEvent     => send_heartbeat(),
    ProtoOAExecutionEvent   => on_order_filled(),
    ProtoOAErrorRes         => handle_error(),
}
```

---

## Roadmap

```
Phase 1 — Connect & Listen (now)
  └── TCP/TLS connection
  └── Authentication
  └── Subscribe to EUR/USD ticks
  └── Log bid/ask in real time

Phase 2 — First Strategy
  └── Implement handle_message() dispatcher
  └── Simple logic in on_tick() (e.g. moving average crossover)
  └── Paper trading (log signals, no real orders yet)

Phase 3 — Live Orders (after KYC approval)
  └── ProtoOANewOrderReq
  └── Risk management (max loss, position sizing)
  └── Order lifecycle tracking

Phase 4 — Observability
  └── TimescaleDB for trade history
  └── Grafana dashboard
  └── Alertmanager for critical events

Phase 5 — ML / Backtesting (Python sidecar)
  └── Historical data from cTrader
  └── Strategy research in Python/Jupyter
  └── Hot path stays in Rust via PyO3
```

---

## Infrastructure Notes

- **Deploy location**: VPS close to FxPro EU servers (Hetzner Nuremberg recommended over FxPro's BeeksFX VPS)
- **OS**: Linux — no Wine/Windows dependency (unlike MT4/MT5)
- **Demo host**: `demo.ctraderapi.com:5035`
- **Live host**: `live.ctraderapi.com:5035` (use only when strategy is proven)
- **TCP_NODELAY**: must be set — disables Nagle algorithm, critical for latency
