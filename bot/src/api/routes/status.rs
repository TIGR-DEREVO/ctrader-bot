use crate::state::{AppState, StatusInfo};
use axum::{extract::State, Json};
use std::sync::Arc;

pub async fn get_status(State(state): State<Arc<AppState>>) -> Json<StatusInfo> {
    Json(state.status_info())
}
