use axum::{extract::State, http::StatusCode, Json};
use serde::{Deserialize, Serialize};

use super::AppState;

// ── Submit learned bundle IDs ────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct SubmitRequest {
    mappings: Vec<MappingSubmission>,
    /// Opaque hash of the reporter's machine ID — for deduplication, not tracking.
    reporter_hash: String,
}

#[derive(Deserialize)]
struct MappingSubmission {
    bundle_id_prefix: String,
    registry_slug: String,
}

#[derive(Serialize)]
pub struct SubmitResponse {
    accepted: usize,
    duplicates: usize,
}

pub async fn submit(
    State(state): State<AppState>,
    Json(request): Json<SubmitRequest>,
) -> Result<Json<SubmitResponse>, (StatusCode, Json<serde_json::Value>)> {
    if request.reporter_hash.is_empty() || request.mappings.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "reporter_hash and mappings are required"})),
        ));
    }

    if request.mappings.len() > 500 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "too many mappings (max 500)"})),
        ));
    }

    let mut accepted = 0usize;
    let mut duplicates = 0usize;

    for mapping in &request.mappings {
        let result = sqlx::query(
            "INSERT INTO bundle_id_submissions (bundle_id_prefix, registry_slug, reporter_hash)
             VALUES ($1, $2, $3)
             ON CONFLICT (bundle_id_prefix, reporter_hash) DO NOTHING",
        )
        .bind(&mapping.bundle_id_prefix)
        .bind(&mapping.registry_slug)
        .bind(&request.reporter_hash)
        .execute(&state.pool)
        .await
        .map_err(|e| {
            tracing::error!("Failed to insert bundle ID submission: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "database error"})),
            )
        })?;

        if result.rows_affected() > 0 {
            accepted += 1;
        } else {
            duplicates += 1;
        }
    }

    Ok(Json(SubmitResponse {
        accepted,
        duplicates,
    }))
}

// ── Fetch confirmed mappings ─────────────────────────────────────────────────

#[derive(Serialize)]
pub struct ConfirmedMapping {
    bundle_id_prefix: String,
    registry_slug: String,
    reporter_count: i64,
}

#[derive(Serialize)]
pub struct ConfirmedResponse {
    mappings: Vec<ConfirmedMapping>,
}

pub async fn confirmed(
    State(state): State<AppState>,
) -> Result<Json<ConfirmedResponse>, (StatusCode, Json<serde_json::Value>)> {
    let rows = sqlx::query_as::<_, (String, String, i64)>(
        "SELECT bundle_id_prefix, registry_slug, reporter_count FROM confirmed_bundle_ids ORDER BY bundle_id_prefix",
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|e| {
        tracing::error!("Failed to fetch confirmed bundle IDs: {e}");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "database error"})),
        )
    })?;

    let mappings = rows
        .into_iter()
        .map(|(prefix, slug, count)| ConfirmedMapping {
            bundle_id_prefix: prefix,
            registry_slug: slug,
            reporter_count: count,
        })
        .collect();

    Ok(Json(ConfirmedResponse { mappings }))
}
