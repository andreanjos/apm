use axum::{extract::State, http::StatusCode, Json};
use serde::Serialize;

use super::AppState;

#[derive(Debug, Serialize)]
pub struct HealthResponse {
    status: &'static str,
    database: &'static str,
}

pub async fn health_check(
    State(state): State<AppState>,
) -> Result<Json<HealthResponse>, (StatusCode, Json<HealthResponse>)> {
    match sqlx::query_scalar::<_, i32>("SELECT 1")
        .fetch_one(&state.pool)
        .await
    {
        Ok(_) => Ok(Json(HealthResponse {
            status: "ok",
            database: "connected",
        })),
        Err(_) => Err((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(HealthResponse {
                status: "degraded",
                database: "unreachable",
            }),
        )),
    }
}
