pub mod bundle_ids;
pub mod health;

use axum::{
    routing::{get, post},
    Router,
};
use sqlx::PgPool;

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health::health_check))
        .route("/api/bundle-ids", post(bundle_ids::submit))
        .route("/api/bundle-ids/confirmed", get(bundle_ids::confirmed))
        .with_state(state)
}
