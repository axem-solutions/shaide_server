use std::sync::Arc;

use axum::{Json, Router, extract::State, routing};

use crate::services::health::{self};

#[utoipa::path(
    get,
    path = "/v1/health",
    tag = "health",
    responses((status = 200, description = "Health state", body = health::HealthState))
)]
pub async fn health(State(state): State<Arc<health::HealthState>>) -> Json<health::HealthState> {
    Json(state.as_ref().clone())
}

pub fn health_router(state: Arc<health::HealthState>) -> Router {
    Router::new()
        .route("/v1/health", routing::get(health))
        .with_state(state)
}
