use std::sync::Arc;

use axum::{extract::State, Json};

use crate::api::dto;
use crate::state::AppState;

pub async fn get_quotes(State(state): State<Arc<AppState>>) -> Json<dto::QuotesMap> {
    let map: dto::QuotesMap = state
        .quotes
        .iter()
        .map(|entry| {
            let q = dto::Quote::from_state(entry.value(), &state.symbols);
            (q.symbol.clone(), q)
        })
        .collect();
    Json(map)
}
