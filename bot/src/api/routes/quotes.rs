use crate::state::{AppState, Quote};
use axum::{extract::State, Json};
use std::sync::Arc;

pub async fn get_quotes(State(state): State<Arc<AppState>>) -> Json<Vec<Quote>> {
    let quotes: Vec<Quote> = state.quotes.iter().map(|e| e.value().clone()).collect();
    Json(quotes)
}
