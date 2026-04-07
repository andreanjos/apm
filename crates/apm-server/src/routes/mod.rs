pub mod auth;
pub mod commerce;
pub mod health;

use axum::{routing::get, Router};
use sqlx::PgPool;

use crate::auth::AuthConfig;
use crate::license::LicenseConfig;
use crate::stripe::StripeConfig;

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub auth: AuthConfig,
    pub stripe: StripeConfig,
    pub license: LicenseConfig,
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health::health_check))
        .nest("/auth", auth::router())
        .nest("/commerce", commerce::router())
        .with_state(state)
}
