//! `GET /api/symbols` — per-symbol trading metadata.
//!
//! Returns an array of `SymbolInfo` for every symbol the bot has full
//! details for. That's typically the subscribed set (populated by
//! `bot_loop::fetch_symbol_details` at startup) plus anything a
//! future code path chooses to hydrate. Symbols we only know by name
//! (the broader 250+ entry catalog) are intentionally excluded — the
//! UI can't meaningfully validate volume against them anyway.

use std::sync::Arc;

use axum::{extract::State, Json};

use crate::api::dto::SymbolInfo;
use crate::state::AppState;

pub async fn list_symbols(State(state): State<Arc<AppState>>) -> Json<Vec<SymbolInfo>> {
    let mut out: Vec<SymbolInfo> = state
        .symbols
        .entries_with_meta()
        .into_iter()
        .map(|(_id, name, meta)| SymbolInfo::from_catalog(name, meta))
        .collect();
    // Stable alphabetical order — keeps the response deterministic and
    // plays nicely with React Query cache keys on repeat fetches.
    out.sort_by(|a, b| a.symbol.cmp(&b.symbol));
    Json(out)
}
