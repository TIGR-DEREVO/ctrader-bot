use crate::api::dto;
use crate::state::AppState;
use axum::{extract::State, Json};
use std::sync::Arc;

pub async fn get_status(State(state): State<Arc<AppState>>) -> Json<dto::Status> {
    Json(dto::Status::from_state(&state))
}
