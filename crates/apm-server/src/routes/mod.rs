pub mod health;

use axum::{routing::get, Router};
use sqlx::PgPool;

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health::health_check))
        .with_state(state)
}
