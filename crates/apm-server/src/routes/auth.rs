use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{Html, IntoResponse},
    routing::{delete, get, post},
    Json, Router,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::Row;

use crate::{
    auth::{
        api_keys::{generate_api_key, hash_secret, key_prefix},
        device_flow::{generate_device_code, generate_secret_token, generate_user_code},
        jwt::{issue_access_token, verify_access_token},
        password::{hash_password, verify_password},
    },
    routes::AppState,
};

pub(crate) const SCOPE_READ: &str = "read";
pub(crate) const SCOPE_PURCHASE: &str = "purchase";
pub(crate) const SCOPE_MANAGE: &str = "manage";

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/signup", post(signup))
        .route("/device/start", post(start_device_flow))
        .route("/device/verify", get(device_verify_page))
        .route("/device/approve", post(approve_device_flow))
        .route("/token/poll", post(poll_device_flow))
        .route("/token/refresh", post(refresh_token))
        .route("/status", get(auth_status))
        .route("/api-keys", post(create_api_key).get(list_api_keys))
        .route("/api-keys/{name}", delete(revoke_api_key))
}

#[derive(Debug, Serialize)]
pub(crate) struct ApiError {
    pub(crate) error: &'static str,
    pub(crate) code: &'static str,
    pub(crate) message: String,
}

type ApiResult<T> = Result<Json<T>, (StatusCode, Json<ApiError>)>;

pub(crate) fn api_error(
    status: StatusCode,
    error: &'static str,
    code: &'static str,
    message: impl Into<String>,
) -> (StatusCode, Json<ApiError>) {
    (
        status,
        Json(ApiError {
            error,
            code,
            message: message.into(),
        }),
    )
}

#[derive(Debug, Deserialize)]
struct SignupRequest {
    email: String,
    password: String,
}

#[derive(Debug, Serialize)]
struct SignupResponse {
    user_id: i64,
    email: String,
}

async fn signup(
    State(state): State<AppState>,
    Json(request): Json<SignupRequest>,
) -> ApiResult<SignupResponse> {
    let email = request.email.trim().to_ascii_lowercase();
    if email.is_empty() || request.password.trim().is_empty() {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "INVALID_SIGNUP",
            "email and password are required",
        ));
    }

    let password_hash = hash_password(&request.password).map_err(internal_error)?;
    let result = sqlx::query(
        r#"
        INSERT INTO users (email, password_hash)
        VALUES ($1, $2)
        RETURNING id, email
        "#,
    )
    .bind(&email)
    .bind(&password_hash)
    .fetch_one(&state.pool)
    .await;

    match result {
        Ok(row) => Ok(Json(SignupResponse {
            user_id: row.get("id"),
            email: row.get("email"),
        })),
        Err(sqlx::Error::Database(error)) if error.is_unique_violation() => Err(api_error(
            StatusCode::CONFLICT,
            "conflict",
            "EMAIL_EXISTS",
            format!("An account already exists for {email}."),
        )),
        Err(error) => Err(internal_error(error)),
    }
}

#[derive(Debug, Deserialize)]
struct DeviceStartRequest {
    email: String,
}

#[derive(Debug, Serialize)]
struct DeviceStartResponse {
    device_code: String,
    user_code: String,
    verification_uri: String,
    verification_uri_complete: String,
    interval: u64,
    expires_at: DateTime<Utc>,
}

async fn start_device_flow(
    State(state): State<AppState>,
    Json(request): Json<DeviceStartRequest>,
) -> ApiResult<DeviceStartResponse> {
    let email = request.email.trim().to_ascii_lowercase();
    let row = sqlx::query("SELECT id FROM users WHERE email = $1")
        .bind(&email)
        .fetch_optional(&state.pool)
        .await
        .map_err(internal_error)?;

    let user_id: i64 = match row {
        Some(row) => row.get("id"),
        None => {
            return Err(api_error(
                StatusCode::NOT_FOUND,
                "not_found",
                "ACCOUNT_NOT_FOUND",
                format!("No account exists for {email}."),
            ))
        }
    };

    let device_code = generate_device_code();
    let user_code = generate_user_code();
    let expires_at = Utc::now() + chrono::Duration::seconds(state.auth.device_code_ttl_seconds);

    sqlx::query(
        r#"
        INSERT INTO device_authorizations (user_id, device_code, user_code, expires_at)
        VALUES ($1, $2, $3, $4)
        "#,
    )
    .bind(user_id)
    .bind(&device_code)
    .bind(&user_code)
    .bind(expires_at)
    .execute(&state.pool)
    .await
    .map_err(internal_error)?;

    let verification_uri = format!("{}/auth/device/verify", state.auth.public_base_url);
    let verification_uri_complete = format!("{verification_uri}?user_code={user_code}");

    Ok(Json(DeviceStartResponse {
        device_code,
        user_code,
        verification_uri,
        verification_uri_complete,
        interval: 1,
        expires_at,
    }))
}

#[derive(Debug, Deserialize)]
struct VerifyQuery {
    user_code: Option<String>,
}

async fn device_verify_page(Query(query): Query<VerifyQuery>) -> Html<String> {
    let body = format!(
        "<html><body><h1>apm device login</h1><p>Approve the pending login with user code: {}</p></body></html>",
        query.user_code.unwrap_or_else(|| "unknown".to_string())
    );
    Html(body)
}

#[derive(Debug, Deserialize)]
struct DeviceApproveRequest {
    user_code: String,
    email: String,
    password: String,
}

#[derive(Debug, Serialize)]
struct DeviceApproveResponse {
    approved: bool,
}

async fn approve_device_flow(
    State(state): State<AppState>,
    Json(request): Json<DeviceApproveRequest>,
) -> ApiResult<DeviceApproveResponse> {
    let email = request.email.trim().to_ascii_lowercase();
    let row = sqlx::query(
        r#"
        SELECT da.id, da.expires_at, da.consumed_at, u.password_hash
        FROM device_authorizations da
        JOIN users u ON u.id = da.user_id
        WHERE da.user_code = $1 AND u.email = $2
        "#,
    )
    .bind(&request.user_code)
    .bind(&email)
    .fetch_optional(&state.pool)
    .await
    .map_err(internal_error)?;

    let row = match row {
        Some(row) => row,
        None => {
            return Err(api_error(
                StatusCode::NOT_FOUND,
                "not_found",
                "DEVICE_CODE_NOT_FOUND",
                "No pending device flow matches that user code.",
            ))
        }
    };

    let expires_at: DateTime<Utc> = row.get("expires_at");
    let consumed_at: Option<DateTime<Utc>> = row.get("consumed_at");
    if consumed_at.is_some() || expires_at <= Utc::now() {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "DEVICE_CODE_EXPIRED",
            "That device authorization is no longer active.",
        ));
    }

    let password_hash: String = row.get("password_hash");
    if !verify_password(&request.password, &password_hash).map_err(internal_error)? {
        return Err(api_error(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "INVALID_CREDENTIALS",
            "The provided credentials are invalid.",
        ));
    }

    sqlx::query("UPDATE device_authorizations SET approved_at = NOW() WHERE id = $1")
        .bind(row.get::<i64, _>("id"))
        .execute(&state.pool)
        .await
        .map_err(internal_error)?;

    Ok(Json(DeviceApproveResponse { approved: true }))
}

#[derive(Debug, Deserialize)]
struct DevicePollRequest {
    device_code: String,
}

#[derive(Debug, Serialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: String,
    expires_at: DateTime<Utc>,
    token_type: &'static str,
    user_id: i64,
    email: String,
}

async fn poll_device_flow(
    State(state): State<AppState>,
    Json(request): Json<DevicePollRequest>,
) -> ApiResult<TokenResponse> {
    let row = sqlx::query(
        r#"
        SELECT da.id, da.user_id, da.expires_at, da.approved_at, da.consumed_at, u.email
        FROM device_authorizations da
        JOIN users u ON u.id = da.user_id
        WHERE da.device_code = $1
        "#,
    )
    .bind(&request.device_code)
    .fetch_optional(&state.pool)
    .await
    .map_err(internal_error)?;

    let row = match row {
        Some(row) => row,
        None => {
            return Err(api_error(
                StatusCode::NOT_FOUND,
                "not_found",
                "DEVICE_CODE_NOT_FOUND",
                "No pending device authorization matches that device code.",
            ))
        }
    };

    let expires_at: DateTime<Utc> = row.get("expires_at");
    let approved_at: Option<DateTime<Utc>> = row.get("approved_at");
    let consumed_at: Option<DateTime<Utc>> = row.get("consumed_at");
    if expires_at <= Utc::now() || consumed_at.is_some() {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "DEVICE_CODE_EXPIRED",
            "That device authorization is no longer active.",
        ));
    }
    if approved_at.is_none() {
        return Err(api_error(
            StatusCode::PRECONDITION_REQUIRED,
            "authorization_pending",
            "AUTHORIZATION_PENDING",
            "The device authorization has not been approved yet.",
        ));
    }

    let user_id: i64 = row.get("user_id");
    let email: String = row.get("email");
    let (access_token, access_expires_at) = issue_access_token(
        user_id,
        &email,
        &state.auth.jwt_secret,
        state.auth.access_token_ttl_seconds,
    )
    .map_err(internal_error)?;
    let refresh_token = issue_refresh_token(&state, user_id, None).await?;

    sqlx::query("UPDATE device_authorizations SET consumed_at = NOW() WHERE id = $1")
        .bind(row.get::<i64, _>("id"))
        .execute(&state.pool)
        .await
        .map_err(internal_error)?;

    Ok(Json(TokenResponse {
        access_token,
        refresh_token,
        expires_at: access_expires_at,
        token_type: "Bearer",
        user_id,
        email,
    }))
}

#[derive(Debug, Deserialize)]
struct RefreshTokenRequest {
    refresh_token: String,
}

async fn refresh_token(
    State(state): State<AppState>,
    Json(request): Json<RefreshTokenRequest>,
) -> ApiResult<TokenResponse> {
    let token_hash = hash_secret(&request.refresh_token);
    let row = sqlx::query(
        r#"
        SELECT rt.id, rt.user_id, rt.expires_at, rt.revoked_at, u.email
        FROM refresh_tokens rt
        JOIN users u ON u.id = rt.user_id
        WHERE rt.token_hash = $1
        "#,
    )
    .bind(&token_hash)
    .fetch_optional(&state.pool)
    .await
    .map_err(internal_error)?;

    let row = match row {
        Some(row) => row,
        None => {
            return Err(api_error(
                StatusCode::UNAUTHORIZED,
                "unauthorized",
                "INVALID_REFRESH_TOKEN",
                "Refresh token is invalid or has been revoked.",
            ))
        }
    };

    let expires_at: DateTime<Utc> = row.get("expires_at");
    let revoked_at: Option<DateTime<Utc>> = row.get("revoked_at");
    if revoked_at.is_some() || expires_at <= Utc::now() {
        return Err(api_error(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "INVALID_REFRESH_TOKEN",
            "Refresh token is invalid or has been revoked.",
        ));
    }

    let token_id: i64 = row.get("id");
    let user_id: i64 = row.get("user_id");
    let email: String = row.get("email");

    sqlx::query("UPDATE refresh_tokens SET revoked_at = NOW(), last_used_at = NOW() WHERE id = $1")
        .bind(token_id)
        .execute(&state.pool)
        .await
        .map_err(internal_error)?;

    let (access_token, access_expires_at) = issue_access_token(
        user_id,
        &email,
        &state.auth.jwt_secret,
        state.auth.access_token_ttl_seconds,
    )
    .map_err(internal_error)?;
    let refresh_token = issue_refresh_token(&state, user_id, Some(token_id)).await?;

    Ok(Json(TokenResponse {
        access_token,
        refresh_token,
        expires_at: access_expires_at,
        token_type: "Bearer",
        user_id,
        email,
    }))
}

#[derive(Debug, Serialize)]
struct AuthStatusResponse {
    authenticated: bool,
    user_id: i64,
    email: String,
    source: &'static str,
    scopes: Vec<String>,
}

async fn auth_status(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<AuthStatusResponse> {
    let user = authenticate(&state, &headers).await?;
    Ok(Json(AuthStatusResponse {
        authenticated: true,
        user_id: user.user_id,
        email: user.email,
        source: user.source,
        scopes: user.scopes,
    }))
}

#[derive(Debug, Deserialize)]
struct CreateApiKeyRequest {
    name: String,
    scopes: Vec<String>,
}

#[derive(Debug, Serialize)]
struct CreateApiKeyResponse {
    name: String,
    api_key: String,
    scopes: Vec<String>,
}

async fn create_api_key(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<CreateApiKeyRequest>,
) -> ApiResult<CreateApiKeyResponse> {
    let user = authenticate(&state, &headers).await?;
    require_scope(&user, SCOPE_MANAGE)?;
    let api_key = generate_api_key();
    let scopes = normalize_scopes(request.scopes)?;
    let scopes_text = scopes.join(",");

    let row = sqlx::query(
        r#"
        INSERT INTO api_keys (user_id, name, key_prefix, key_hash, scopes)
        VALUES ($1, $2, $3, $4, $5)
        RETURNING id
        "#,
    )
    .bind(user.user_id)
    .bind(&request.name)
    .bind(key_prefix(&api_key))
    .bind(hash_secret(&api_key))
    .bind(&scopes_text)
    .fetch_one(&state.pool)
    .await
    .map_err(internal_error)?;

    sqlx::query(
        r#"
        INSERT INTO api_key_purchase_policies (api_key_id)
        VALUES ($1)
        ON CONFLICT (api_key_id) DO NOTHING
        "#,
    )
    .bind(row.get::<i64, _>("id"))
    .execute(&state.pool)
    .await
    .map_err(internal_error)?;

    Ok(Json(CreateApiKeyResponse {
        name: request.name,
        api_key,
        scopes,
    }))
}

#[derive(Debug, Serialize)]
struct ApiKeyRecord {
    name: String,
    key_prefix: String,
    scopes: Vec<String>,
    revoked: bool,
}

async fn list_api_keys(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Vec<ApiKeyRecord>> {
    let user = authenticate(&state, &headers).await?;
    require_scope(&user, SCOPE_MANAGE)?;
    let rows = sqlx::query(
        r#"
        SELECT name, key_prefix, scopes, revoked_at
        FROM api_keys
        WHERE user_id = $1
        ORDER BY name ASC
        "#,
    )
    .bind(user.user_id)
    .fetch_all(&state.pool)
    .await
    .map_err(internal_error)?;

    let records = rows
        .into_iter()
        .map(|row| ApiKeyRecord {
            name: row.get("name"),
            key_prefix: row.get("key_prefix"),
            scopes: split_scopes(row.get::<String, _>("scopes")),
            revoked: row.get::<Option<DateTime<Utc>>, _>("revoked_at").is_some(),
        })
        .collect();

    Ok(Json(records))
}

#[derive(Debug, Serialize)]
struct RevokeApiKeyResponse {
    revoked: bool,
    name: String,
}

async fn revoke_api_key(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(name): Path<String>,
) -> ApiResult<RevokeApiKeyResponse> {
    let user = authenticate(&state, &headers).await?;
    require_scope(&user, SCOPE_MANAGE)?;
    sqlx::query("UPDATE api_keys SET revoked_at = NOW() WHERE user_id = $1 AND name = $2")
        .bind(user.user_id)
        .bind(&name)
        .execute(&state.pool)
        .await
        .map_err(internal_error)?;

    Ok(Json(RevokeApiKeyResponse {
        revoked: true,
        name,
    }))
}

#[derive(Debug)]
pub(crate) struct AuthenticatedUser {
    pub user_id: i64,
    pub email: String,
    pub source: &'static str,
    pub scopes: Vec<String>,
    pub api_key_id: Option<i64>,
}

pub(crate) async fn authenticate(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<AuthenticatedUser, (StatusCode, Json<ApiError>)> {
    if let Some(header) = headers.get(axum::http::header::AUTHORIZATION) {
        if let Ok(value) = header.to_str() {
            if let Some(token) = value.strip_prefix("Bearer ") {
                let claims = verify_access_token(token, &state.auth.jwt_secret).map_err(|_| {
                    api_error(
                        StatusCode::UNAUTHORIZED,
                        "unauthorized",
                        "INVALID_ACCESS_TOKEN",
                        "Access token is invalid or expired.",
                    )
                })?;

                return Ok(AuthenticatedUser {
                    user_id: claims.sub,
                    email: claims.email,
                    source: "bearer",
                    scopes: vec![
                        SCOPE_READ.to_string(),
                        SCOPE_PURCHASE.to_string(),
                        SCOPE_MANAGE.to_string(),
                    ],
                    api_key_id: None,
                });
            }
        }
    }

    if let Some(header) = headers.get("x-apm-api-key") {
        let api_key = header.to_str().map_err(|_| {
            api_error(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                "INVALID_API_KEY_HEADER",
                "x-apm-api-key header must be valid UTF-8.",
            )
        })?;
        let row = sqlx::query(
            r#"
            SELECT u.id AS user_id, u.email, ak.scopes, ak.id AS api_key_id, ak.expires_at, ak.revoked_at
            FROM api_keys ak
            JOIN users u ON u.id = ak.user_id
            WHERE ak.key_hash = $1
            "#,
        )
        .bind(hash_secret(api_key))
        .fetch_optional(&state.pool)
        .await
        .map_err(internal_error)?;

        let row = row.ok_or_else(|| {
            api_error(
                StatusCode::UNAUTHORIZED,
                "unauthorized",
                "INVALID_API_KEY",
                "API key is invalid or revoked.",
            )
        })?;

        let expires_at: Option<DateTime<Utc>> = row.get("expires_at");
        let revoked_at: Option<DateTime<Utc>> = row.get("revoked_at");
        if revoked_at.is_some() || expires_at.map(|value| value <= Utc::now()).unwrap_or(false) {
            return Err(api_error(
                StatusCode::UNAUTHORIZED,
                "unauthorized",
                "INVALID_API_KEY",
                "API key is invalid or revoked.",
            ));
        }

        sqlx::query("UPDATE api_keys SET last_used_at = NOW() WHERE id = $1")
            .bind(row.get::<i64, _>("api_key_id"))
            .execute(&state.pool)
            .await
            .map_err(internal_error)?;

        return Ok(AuthenticatedUser {
            user_id: row.get("user_id"),
            email: row.get("email"),
            source: "api_key",
            scopes: split_scopes(row.get::<String, _>("scopes")),
            api_key_id: Some(row.get("api_key_id")),
        });
    }

    Err(api_error(
        StatusCode::UNAUTHORIZED,
        "unauthorized",
        "AUTH_REQUIRED",
        "Authentication is required.",
    ))
}

async fn issue_refresh_token(
    state: &AppState,
    user_id: i64,
    rotated_from_id: Option<i64>,
) -> Result<String, (StatusCode, Json<ApiError>)> {
    let refresh_token = generate_secret_token();
    let expires_at = Utc::now() + chrono::Duration::seconds(state.auth.refresh_token_ttl_seconds);

    sqlx::query(
        r#"
        INSERT INTO refresh_tokens (user_id, token_hash, expires_at, rotated_from_id)
        VALUES ($1, $2, $3, $4)
        "#,
    )
    .bind(user_id)
    .bind(hash_secret(&refresh_token))
    .bind(expires_at)
    .bind(rotated_from_id)
    .execute(&state.pool)
    .await
    .map_err(internal_error)?;

    Ok(refresh_token)
}

fn normalize_scope(scope: &str) -> Option<&'static str> {
    match scope.trim() {
        "account:read" | "read" => Some(SCOPE_READ),
        "account:write" | "write" | "manage" => Some(SCOPE_MANAGE),
        "purchase" => Some(SCOPE_PURCHASE),
        _ => None,
    }
}

fn normalize_scopes(mut scopes: Vec<String>) -> Result<Vec<String>, (StatusCode, Json<ApiError>)> {
    if scopes.is_empty() {
        scopes.push(SCOPE_READ.to_string());
    }

    let mut normalized = scopes
        .into_iter()
        .map(|scope| {
            normalize_scope(&scope)
                .map(|value| value.to_string())
                .ok_or_else(|| {
                    api_error(
                        StatusCode::BAD_REQUEST,
                        "invalid_request",
                        "INVALID_SCOPE",
                        format!(
                            "Unknown scope '{}'. Allowed scopes: read, purchase, manage.",
                            scope.trim()
                        ),
                    )
                })
        })
        .collect::<Result<Vec<_>, _>>()?;

    normalized.sort();
    normalized.dedup();
    Ok(normalized)
}

fn split_scopes(scopes: String) -> Vec<String> {
    let mut normalized = scopes
        .split(',')
        .filter_map(|value| normalize_scope(value).map(str::to_string))
        .collect::<Vec<_>>();
    normalized.sort();
    normalized.dedup();
    normalized
}

pub(crate) fn require_scope(
    user: &AuthenticatedUser,
    scope: &'static str,
) -> Result<(), (StatusCode, Json<ApiError>)> {
    if user.scopes.iter().any(|value| value == scope) {
        return Ok(());
    }

    Err(api_error(
        StatusCode::FORBIDDEN,
        "forbidden",
        "INSUFFICIENT_SCOPE",
        format!("The authenticated principal is missing the required '{scope}' scope."),
    ))
}

pub(crate) fn require_api_key(
    user: &AuthenticatedUser,
) -> Result<i64, (StatusCode, Json<ApiError>)> {
    user.api_key_id.ok_or_else(|| {
        api_error(
            StatusCode::FORBIDDEN,
            "forbidden",
            "AGENT_PURCHASE_REQUIRES_API_KEY",
            "Agent purchase requires API key authentication.",
        )
    })
}

fn internal_error(error: impl std::fmt::Display) -> (StatusCode, Json<ApiError>) {
    tracing::error!(error = %error, "auth route failed");
    api_error(
        StatusCode::INTERNAL_SERVER_ERROR,
        "internal_error",
        "INTERNAL_SERVER_ERROR",
        "An internal server error occurred.",
    )
}

impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        (StatusCode::BAD_REQUEST, Json(self)).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn internal_error_hides_backend_details() {
        let (status, Json(error)) = internal_error("constraint violated");
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(error.code, "INTERNAL_SERVER_ERROR");
        assert_eq!(error.message, "An internal server error occurred.");
    }
}
