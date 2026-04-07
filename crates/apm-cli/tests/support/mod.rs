#![allow(dead_code)]
#![allow(clippy::too_many_arguments)]

use std::{
    collections::HashMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use apm_core::{LicensePayload, LicenseStatus, SignedLicense};
use assert_fs::TempDir;
use axum::{
    extract::{Path as AxumPath, Query, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use chrono::{Duration, Utc};
use ed25519_dalek::{Signer, SigningKey};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use tokio::net::TcpListener;

pub struct CliTestEnv {
    pub config_home: TempDir,
    pub data_home: TempDir,
    pub cache_home: TempDir,
    pub credential_store_dir: TempDir,
}

impl CliTestEnv {
    pub fn new() -> Self {
        Self {
            config_home: TempDir::new().unwrap(),
            data_home: TempDir::new().unwrap(),
            cache_home: TempDir::new().unwrap(),
            credential_store_dir: TempDir::new().unwrap(),
        }
    }

    pub fn apply(&self, cmd: &mut std::process::Command) {
        cmd.env("XDG_CONFIG_HOME", self.config_home.path())
            .env("XDG_DATA_HOME", self.data_home.path())
            .env("XDG_CACHE_HOME", self.cache_home.path())
            .env(
                "APM_TEST_CREDENTIAL_STORE_DIR",
                self.credential_store_dir.path(),
            )
            .env("APM_TEST_SKIP_BROWSER", "1")
            .env("NO_COLOR", "1")
            .env("TERM", "dumb");
    }

    pub fn credential_path(&self, name: &str) -> PathBuf {
        self.credential_store_dir
            .path()
            .join(format!("{}.json", name.replace(':', "_")))
    }

    pub fn write_session(
        &self,
        access_token: &str,
        refresh_token: &str,
        email: &str,
        expires_in_seconds: i64,
    ) {
        let session = json!({
            "access_token": access_token,
            "refresh_token": refresh_token,
            "expires_at": (Utc::now() + Duration::seconds(expires_in_seconds)).to_rfc3339(),
            "user_id": 1,
            "email": email,
        });
        fs::write(
            self.credential_path("session"),
            serde_json::to_string(&session).unwrap(),
        )
        .unwrap();
    }
}

#[derive(Clone)]
pub struct MockAuthServer {
    pub base_url: String,
    state: Arc<Mutex<MockState>>,
}

#[derive(Clone)]
pub struct MockCommerceServer {
    pub base_url: String,
    state: Arc<Mutex<MockCommerceState>>,
}

#[derive(Default)]
struct MockState {
    users: HashMap<String, String>,
    device_codes: HashMap<String, DeviceRecord>,
    access_tokens: HashMap<String, String>,
    refresh_tokens: HashMap<String, String>,
    refresh_calls: usize,
    next_id: usize,
}

#[derive(Clone)]
struct DeviceRecord {
    email: String,
    user_code: String,
    approved: bool,
}

#[derive(Clone)]
struct MockCheckout {
    order_id: i64,
    checkout_session_id: String,
    checkout_url: String,
}

#[derive(Default)]
struct MockCommerceState {
    next_order_id: i64,
    checkouts: HashMap<(String, String), MockCheckout>,
    orders: HashMap<i64, MockOrder>,
    status_calls: HashMap<i64, usize>,
}

#[derive(Clone)]
struct MockOrder {
    plugin_slug: String,
    checkout_session_id: String,
    status: String,
    license_token: Option<String>,
    license: Option<SignedLicense>,
    download_token: Option<String>,
    fulfill_after: usize,
}

const TEST_LICENSE_SIGNING_KEY_HEX: &str =
    "1f1e1d1c1b1a19181716151413121110ffeeddccbbaa99887766554433221100";

#[derive(Deserialize)]
struct SignupRequest {
    email: String,
    password: String,
}

#[derive(Deserialize)]
struct DeviceStartRequest {
    email: String,
}

#[derive(Deserialize)]
struct DeviceApproveRequest {
    email: String,
    password: String,
    user_code: String,
}

#[derive(Deserialize)]
struct DevicePollRequest {
    device_code: String,
}

#[derive(Deserialize)]
struct RefreshRequest {
    refresh_token: String,
}

#[derive(Serialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: String,
    expires_at: chrono::DateTime<chrono::Utc>,
    token_type: String,
    user_id: i64,
    email: String,
}

pub async fn spawn_mock_auth_server() -> MockAuthServer {
    let state = Arc::new(Mutex::new(MockState::default()));
    let app = Router::new()
        .route("/auth/signup", post(signup))
        .route("/auth/device/start", post(device_start))
        .route("/auth/device/approve", post(device_approve))
        .route("/auth/token/poll", post(token_poll))
        .route("/auth/token/refresh", post(token_refresh))
        .route("/auth/status", get(status))
        .with_state(state.clone());

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    MockAuthServer {
        base_url: format!("http://{address}"),
        state,
    }
}

pub async fn spawn_mock_commerce_server(fulfill_after: usize) -> MockCommerceServer {
    let state = Arc::new(Mutex::new(MockCommerceState::default()));
    let app = Router::new()
        .route("/commerce/agent/purchase", post(commerce_agent_purchase))
        .route("/commerce/checkout", post(commerce_checkout))
        .route("/commerce/compare", post(commerce_compare))
        .route("/commerce/explore", get(commerce_explore))
        .route("/commerce/featured", get(commerce_featured))
        .route("/commerce/licenses", get(commerce_licenses))
        .route("/commerce/orders/{order_id}", get(commerce_order_status))
        .route("/commerce/refunds", post(commerce_refund))
        .route("/commerce/restore", post(commerce_restore))
        .route("/commerce/success", get(commerce_success))
        .route("/downloads/{slug}", get(plugin_download))
        .with_state((state.clone(), fulfill_after));

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    MockCommerceServer {
        base_url: format!("http://{address}"),
        state,
    }
}

impl MockAuthServer {
    pub fn refresh_calls(&self) -> usize {
        self.state.lock().unwrap().refresh_calls
    }
}

impl MockCommerceServer {
    pub fn distinct_checkout_intents(&self) -> usize {
        self.state.lock().unwrap().checkouts.len()
    }

    pub fn download_url(&self, slug: &str) -> String {
        format!("{}/downloads/{slug}", self.base_url)
    }
}

async fn signup(
    State(state): State<Arc<Mutex<MockState>>>,
    Json(request): Json<SignupRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let mut state = state.lock().unwrap();
    if state.users.contains_key(&request.email) {
        return Err((StatusCode::CONFLICT, Json(json!({"code":"EMAIL_EXISTS"}))));
    }
    state.users.insert(request.email.clone(), request.password);
    Ok(Json(json!({"email": request.email, "user_id": 1})))
}

async fn device_start(
    State(state): State<Arc<Mutex<MockState>>>,
    Json(request): Json<DeviceStartRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let mut state = state.lock().unwrap();
    if !state.users.contains_key(&request.email) {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({"code":"ACCOUNT_NOT_FOUND"})),
        ));
    }

    state.next_id += 1;
    let device_code = format!("device-{}", state.next_id);
    let user_code = format!("USER{:04}", state.next_id);
    state.device_codes.insert(
        device_code.clone(),
        DeviceRecord {
            email: request.email,
            user_code: user_code.clone(),
            approved: false,
        },
    );

    Ok(Json(json!({
        "device_code": device_code,
        "user_code": user_code,
        "verification_uri": "http://127.0.0.1/device",
        "verification_uri_complete": "http://127.0.0.1/device?user_code=test",
        "interval": 0,
    })))
}

async fn device_approve(
    State(state): State<Arc<Mutex<MockState>>>,
    Json(request): Json<DeviceApproveRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let mut state = state.lock().unwrap();
    let password = state.users.get(&request.email).cloned();
    if password.as_deref() != Some(request.password.as_str()) {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(json!({"code":"INVALID_CREDENTIALS"})),
        ));
    }

    let record = state
        .device_codes
        .values_mut()
        .find(|record| record.user_code == request.user_code && record.email == request.email)
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(json!({"code":"DEVICE_CODE_NOT_FOUND"})),
            )
        })?;
    record.approved = true;
    Ok(Json(json!({"approved": true})))
}

async fn token_poll(
    State(state): State<Arc<Mutex<MockState>>>,
    Json(request): Json<DevicePollRequest>,
) -> Result<Json<TokenResponse>, (StatusCode, Json<serde_json::Value>)> {
    let mut state = state.lock().unwrap();
    let record = state
        .device_codes
        .get(&request.device_code)
        .cloned()
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(json!({"code":"DEVICE_CODE_NOT_FOUND"})),
            )
        })?;
    if !record.approved {
        return Err((
            StatusCode::PRECONDITION_REQUIRED,
            Json(json!({"code":"AUTHORIZATION_PENDING"})),
        ));
    }

    state.next_id += 1;
    let access_token = format!("access-{}", state.next_id);
    let refresh_token = format!("refresh-{}", state.next_id);
    state
        .access_tokens
        .insert(access_token.clone(), record.email.clone());
    state
        .refresh_tokens
        .insert(refresh_token.clone(), record.email.clone());

    Ok(Json(TokenResponse {
        access_token,
        refresh_token,
        expires_at: Utc::now() + Duration::minutes(15),
        token_type: "Bearer".to_string(),
        user_id: 1,
        email: record.email,
    }))
}

async fn token_refresh(
    State(state): State<Arc<Mutex<MockState>>>,
    Json(request): Json<RefreshRequest>,
) -> Result<Json<TokenResponse>, (StatusCode, Json<serde_json::Value>)> {
    let mut state = state.lock().unwrap();
    let email = state
        .refresh_tokens
        .get(&request.refresh_token)
        .cloned()
        .ok_or_else(|| {
            (
                StatusCode::UNAUTHORIZED,
                Json(json!({"code":"INVALID_REFRESH_TOKEN"})),
            )
        })?;
    state.refresh_calls += 1;
    state.next_id += 1;
    let access_token = format!("access-{}", state.next_id);
    let refresh_token = format!("refresh-{}", state.next_id);
    state
        .access_tokens
        .insert(access_token.clone(), email.clone());
    state
        .refresh_tokens
        .insert(refresh_token.clone(), email.clone());

    Ok(Json(TokenResponse {
        access_token,
        refresh_token,
        expires_at: Utc::now() + Duration::minutes(15),
        token_type: "Bearer".to_string(),
        user_id: 1,
        email,
    }))
}

#[derive(Deserialize)]
struct StatusQuery {
    source: Option<String>,
}

async fn status(
    State(state): State<Arc<Mutex<MockState>>>,
    headers: axum::http::HeaderMap,
    Query(_query): Query<StatusQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let state = state.lock().unwrap();

    if let Some(api_key) = headers.get("x-apm-api-key") {
        let _ = api_key;
        return Ok(Json(json!({
            "authenticated": true,
            "user_id": 1,
            "email": "agent@example.com",
            "source": "api_key",
            "scopes": ["account:read"]
        })));
    }

    if let Some(auth) = headers.get(axum::http::header::AUTHORIZATION) {
        if let Ok(value) = auth.to_str() {
            if let Some(token) = value.strip_prefix("Bearer ") {
                if let Some(email) = state.access_tokens.get(token) {
                    return Ok(Json(json!({
                        "authenticated": true,
                        "user_id": 1,
                        "email": email,
                        "source": "bearer",
                        "scopes": ["account:read", "account:write"]
                    })));
                }
            }
        }
    }

    Err((
        StatusCode::UNAUTHORIZED,
        Json(json!({"code":"AUTH_REQUIRED"})),
    ))
}

#[derive(Deserialize)]
struct CommerceCheckoutRequest {
    plugin_slug: String,
    idempotency_key: String,
}

async fn commerce_checkout(
    State((state, _fulfill_after)): State<(Arc<Mutex<MockCommerceState>>, usize)>,
    headers: axum::http::HeaderMap,
    Json(request): Json<CommerceCheckoutRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if !headers.contains_key("x-apm-api-key")
        && !headers.contains_key(axum::http::header::AUTHORIZATION)
    {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(json!({"code":"AUTH_REQUIRED"})),
        ));
    }

    let mut state = state.lock().unwrap();
    let key = (request.plugin_slug.clone(), request.idempotency_key.clone());
    let checkout = if let Some(existing) = state.checkouts.get(&key) {
        existing.clone()
    } else {
        state.next_order_id += 1;
        let order_id = state.next_order_id;
        let checkout = MockCheckout {
            order_id,
            checkout_session_id: format!("cs_test_{order_id}"),
            checkout_url: format!("https://checkout.stripe.com/pay/cs_test_{order_id}"),
        };
        state.checkouts.insert(key.clone(), checkout.clone());
        state.orders.insert(
            order_id,
            MockOrder {
                plugin_slug: request.plugin_slug.clone(),
                checkout_session_id: checkout.checkout_session_id.clone(),
                status: "pending".to_string(),
                license_token: None,
                license: None,
                download_token: None,
                fulfill_after: 0,
            },
        );
        checkout
    };

    Ok(Json(json!({
        "order_id": checkout.order_id,
        "checkout_session_id": checkout.checkout_session_id,
        "checkout_url": checkout.checkout_url,
        "idempotency_key": request.idempotency_key,
        "tax_enabled": true,
    })))
}

async fn commerce_order_status(
    State((state, fulfill_after)): State<(Arc<Mutex<MockCommerceState>>, usize)>,
    AxumPath(order_id): AxumPath<i64>,
    headers: axum::http::HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if !headers.contains_key("x-apm-api-key")
        && !headers.contains_key(axum::http::header::AUTHORIZATION)
    {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(json!({"code":"AUTH_REQUIRED"})),
        ));
    }

    let mut state = state.lock().unwrap();
    let calls = {
        let entry = state.status_calls.entry(order_id).or_insert(0);
        *entry += 1;
        *entry
    };

    let order = state.orders.get_mut(&order_id).ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(json!({"code":"ORDER_NOT_FOUND"})),
        )
    })?;

    order.fulfill_after = fulfill_after;
    if order.status == "pending" && calls > order.fulfill_after {
        order.status = "fulfilled".to_string();
        order.license_token = Some(format!("lic_{order_id}"));
        order.license = Some(sign_license(
            order_id,
            &order.plugin_slug,
            LicenseStatus::Active,
        ));
        order.download_token = Some(format!("dl_{order_id}"));
    }

    Ok(Json(json!({
        "order_id": order_id,
        "plugin_slug": order.plugin_slug,
        "status": order.status,
        "license_token": order.license_token,
        "license": order.license,
        "download_token": order.download_token,
        "checkout_session_id": order.checkout_session_id,
        "refunded_at": serde_json::Value::Null,
    })))
}

async fn commerce_licenses(
    State((state, fulfill_after)): State<(Arc<Mutex<MockCommerceState>>, usize)>,
    headers: axum::http::HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if !headers.contains_key("x-apm-api-key")
        && !headers.contains_key(axum::http::header::AUTHORIZATION)
    {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(json!({"code":"AUTH_REQUIRED"})),
        ));
    }

    let mut state = state.lock().unwrap();
    for (order_id, order) in state.orders.iter_mut() {
        if order.status == "pending" && fulfill_after == 0 {
            order.status = "fulfilled".to_string();
            order.license_token = Some(format!("lic_{order_id}"));
            order.license = Some(sign_license(
                *order_id,
                &order.plugin_slug,
                LicenseStatus::Active,
            ));
            order.download_token = Some(format!("dl_{order_id}"));
        }
    }

    let licenses: Vec<_> = state
        .orders
        .iter()
        .filter_map(|(order_id, order)| {
            order.license.clone().map(|license| {
                let status = license_status_name(&license);
                json!({
                    "plugin_slug": order.plugin_slug,
                    "order_id": order_id,
                    "status": status,
                    "license": license,
                    "activated_at": serde_json::Value::Null,
                    "revoked_at": if status == "refunded" { json!(Utc::now()) } else { serde_json::Value::Null },
                })
            })
        })
        .collect();

    Ok(Json(json!({
        "public_key_hex": public_key_hex(),
        "licenses": licenses,
    })))
}

#[derive(Deserialize)]
struct CommerceRefundRequest {
    order_id: i64,
}

#[derive(Deserialize)]
struct CommerceCompareRequest {
    left_slug: String,
    right_slug: String,
}

#[derive(Deserialize)]
struct CommerceAgentPurchaseRequest {
    plugin_slug: String,
    idempotency_key: String,
}

async fn commerce_refund(
    State((state, _fulfill_after)): State<(Arc<Mutex<MockCommerceState>>, usize)>,
    headers: axum::http::HeaderMap,
    Json(request): Json<CommerceRefundRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if !headers.contains_key("x-apm-api-key")
        && !headers.contains_key(axum::http::header::AUTHORIZATION)
    {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(json!({"code":"AUTH_REQUIRED"})),
        ));
    }

    let mut state = state.lock().unwrap();
    let order = state.orders.get_mut(&request.order_id).ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(json!({"code":"ORDER_NOT_FOUND"})),
        )
    })?;
    order.status = "refunded".to_string();
    order.license = Some(sign_license(
        request.order_id,
        &order.plugin_slug,
        LicenseStatus::Refunded,
    ));
    order.download_token = None;

    Ok(Json(json!({
        "order_id": request.order_id,
        "refunded": true,
        "status": "refunded",
    })))
}

async fn commerce_restore(
    State((state, _fulfill_after)): State<(Arc<Mutex<MockCommerceState>>, usize)>,
    headers: axum::http::HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if !headers.contains_key("x-apm-api-key")
        && !headers.contains_key(axum::http::header::AUTHORIZATION)
    {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(json!({"code":"AUTH_REQUIRED"})),
        ));
    }

    let state = state.lock().unwrap();
    let restorable_plugins: Vec<_> = state
        .orders
        .iter()
        .filter_map(|(order_id, order)| {
            let license = order.license.clone()?;
            (license_status_name(&license) == "active").then(|| {
                json!({
                    "plugin_slug": order.plugin_slug,
                    "order_id": order_id,
                    "download_token": order.download_token,
                    "license": license,
                })
            })
        })
        .collect();

    Ok(Json(json!({
        "public_key_hex": public_key_hex(),
        "restorable_plugins": restorable_plugins,
    })))
}

async fn commerce_agent_purchase(
    State((state, _fulfill_after)): State<(Arc<Mutex<MockCommerceState>>, usize)>,
    headers: axum::http::HeaderMap,
    Json(request): Json<CommerceAgentPurchaseRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let api_key = headers
        .get("x-apm-api-key")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();

    if api_key.is_empty() {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(json!({
                "error": "unauthorized",
                "code": "AUTH_REQUIRED",
                "message": "Authentication is required."
            })),
        ));
    }

    if api_key.contains("readonly") {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({
                "error": "forbidden",
                "code": "INSUFFICIENT_SCOPE",
                "message": "The authenticated principal is missing the required 'purchase' scope."
            })),
        ));
    }

    if api_key.contains("deny") {
        return Err((
            StatusCode::PRECONDITION_REQUIRED,
            Json(json!({
                "error": "forbidden",
                "code": "PREAUTHORIZED_PAYMENT_REQUIRED",
                "message": "A pre-authorized payment method is required for non-interactive agent purchase.",
                "details": {"price_cents": 4999, "currency": "usd"}
            })),
        ));
    }

    let mut state = state.lock().unwrap();
    let key = (request.plugin_slug.clone(), request.idempotency_key.clone());
    let checkout = if let Some(existing) = state.checkouts.get(&key) {
        existing.clone()
    } else {
        state.next_order_id += 1;
        let order_id = state.next_order_id;
        let checkout = MockCheckout {
            order_id,
            checkout_session_id: format!("agent_tx_{order_id}"),
            checkout_url: String::new(),
        };
        state.checkouts.insert(key.clone(), checkout.clone());
        state.orders.insert(
            order_id,
            MockOrder {
                plugin_slug: request.plugin_slug.clone(),
                checkout_session_id: checkout.checkout_session_id.clone(),
                status: "fulfilled".to_string(),
                license_token: Some(format!("lic_{order_id}")),
                license: Some(sign_license(
                    order_id,
                    &request.plugin_slug,
                    LicenseStatus::Active,
                )),
                download_token: Some(format!("dl_{order_id}")),
                fulfill_after: 0,
            },
        );
        checkout
    };

    let order = state.orders.get(&checkout.order_id).unwrap();
    Ok(Json(json!({
        "transaction_id": checkout.checkout_session_id,
        "order_id": checkout.order_id,
        "plugin_slug": order.plugin_slug,
        "status": order.status,
        "fulfilled": true,
        "install_ready": true,
        "cost_cents": 4999,
        "currency": "usd",
        "license_token": order.license_token,
        "license": order.license,
        "download_token": order.download_token,
    })))
}

async fn commerce_featured() -> Json<serde_json::Value> {
    Json(json!({
        "sections": [
            {
                "slug": "staff-picks",
                "title": "Staff Picks",
                "description": "Curated by the storefront service.",
                "plugins": [
                    storefront_plugin(
                        "staff-picked-pro",
                        "Staff Picked Pro",
                        "Acme Audio",
                        "1.4.0",
                        "mixing",
                        "A polished mastering chain.",
                        &["featured", "mix"],
                        &["vst3", "au"],
                        4900,
                        "usd",
                        true
                    )
                ]
            },
            {
                "slug": "new-releases",
                "title": "New Releases",
                "description": "Fresh additions without a CLI release.",
                "plugins": [
                    storefront_plugin(
                        "fast-lint",
                        "Fast Lint",
                        "Build Tools Inc.",
                        "2.1.0",
                        "developer-tools",
                        "A fast static analysis plugin.",
                        &["new", "quality"],
                        &["cli"],
                        0,
                        "usd",
                        false
                    )
                ]
            }
        ]
    }))
}

async fn commerce_explore() -> Json<serde_json::Value> {
    Json(json!({
        "categories": [
            {
                "slug": "developer-tools",
                "title": "Build and Release",
                "description": "Editorial categories owned by the server.",
                "plugins": [
                    storefront_plugin(
                        "fast-lint",
                        "Fast Lint",
                        "Build Tools Inc.",
                        "2.1.0",
                        "developer-tools",
                        "A fast static analysis plugin.",
                        &["new", "quality"],
                        &["cli"],
                        0,
                        "usd",
                        false
                    ),
                    storefront_plugin(
                        "bundle-archiver",
                        "Bundle Archiver",
                        "Release Ops",
                        "0.9.1",
                        "deployment",
                        "Archives release bundles deterministically.",
                        &["ops", "shipping"],
                        &["cli", "binary"],
                        1900,
                        "usd",
                        true
                    )
                ]
            }
        ]
    }))
}

async fn commerce_compare(
    Json(request): Json<CommerceCompareRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if request.left_slug == request.right_slug {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": {
                    "message": "Compare requires two distinct plugin slugs."
                }
            })),
        ));
    }

    let left = storefront_plugin_by_slug(&request.left_slug).ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": {
                    "message": format!("No storefront plugin exists for '{}'.", request.left_slug)
                }
            })),
        )
    })?;
    let right = storefront_plugin_by_slug(&request.right_slug).ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": {
                    "message": format!("No storefront plugin exists for '{}'.", request.right_slug)
                }
            })),
        )
    })?;

    Ok(Json(json!({
        "left": left,
        "right": right,
    })))
}

async fn commerce_success() -> axum::response::Html<&'static str> {
    axum::response::Html("<html><body><p>Checkout success.</p></body></html>")
}

async fn plugin_download(
    AxumPath(slug): AxumPath<String>,
) -> Result<([(axum::http::header::HeaderName, &'static str); 1], Vec<u8>), StatusCode> {
    Ok((
        [(axum::http::header::CONTENT_TYPE, "application/zip")],
        plugin_archive_bytes(&slug),
    ))
}

pub fn command(env: &CliTestEnv) -> std::process::Command {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let binary = std::env::var("CARGO_BIN_EXE_apm")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            manifest
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .join("target/debug/apm")
        });
    let mut command = std::process::Command::new(binary);
    env.apply(&mut command);
    command
}

pub fn read_to_string(path: &Path) -> String {
    fs::read_to_string(path).unwrap()
}

pub fn test_plugin_archive_sha256(slug: &str) -> String {
    let bytes = plugin_archive_bytes(slug);
    let digest = Sha256::digest(bytes);
    hex::encode(digest)
}

fn sign_license(order_id: i64, plugin_slug: &str, status: LicenseStatus) -> SignedLicense {
    let key_bytes = hex::decode(TEST_LICENSE_SIGNING_KEY_HEX).unwrap();
    let signing_key = SigningKey::from_bytes(&key_bytes.try_into().unwrap());
    let payload = LicensePayload {
        schema_version: 1,
        license_id: format!("lic_{order_id}"),
        user_id: 1,
        plugin_slug: plugin_slug.to_string(),
        plugin_version: None,
        order_id,
        issued_at: Utc::now(),
        revoked_at: match status {
            LicenseStatus::Active => None,
            _ => Some(Utc::now()),
        },
        status,
    };
    let signature = signing_key.sign(&serde_json::to_vec(&payload).unwrap());

    SignedLicense {
        payload,
        signature: hex::encode(signature.to_bytes()),
        key_id: "dev-ed25519-1".to_string(),
    }
}

fn public_key_hex() -> String {
    let key_bytes = hex::decode(TEST_LICENSE_SIGNING_KEY_HEX).unwrap();
    let signing_key = SigningKey::from_bytes(&key_bytes.try_into().unwrap());
    hex::encode(signing_key.verifying_key().to_bytes())
}

fn license_status_name(license: &SignedLicense) -> &'static str {
    match license.payload.status {
        LicenseStatus::Active => "active",
        LicenseStatus::Refunded => "refunded",
        LicenseStatus::Revoked => "revoked",
    }
}

fn storefront_plugin_by_slug(slug: &str) -> Option<serde_json::Value> {
    match slug {
        "staff-picked-pro" => Some(storefront_plugin(
            "staff-picked-pro",
            "Staff Picked Pro",
            "Acme Audio",
            "1.4.0",
            "mixing",
            "A polished mastering chain.",
            &["featured", "mix"],
            &["vst3", "au"],
            4900,
            "usd",
            true,
        )),
        "bundle-archiver" => Some(storefront_plugin(
            "bundle-archiver",
            "Bundle Archiver",
            "Release Ops",
            "0.9.1",
            "deployment",
            "Archives release bundles deterministically.",
            &["ops", "shipping"],
            &["cli", "binary"],
            1900,
            "usd",
            true,
        )),
        "fast-lint" => Some(storefront_plugin(
            "fast-lint",
            "Fast Lint",
            "Build Tools Inc.",
            "2.1.0",
            "developer-tools",
            "A fast static analysis plugin.",
            &["new", "quality"],
            &["cli"],
            0,
            "usd",
            false,
        )),
        _ => None,
    }
}

fn storefront_plugin(
    slug: &str,
    name: &str,
    vendor: &str,
    version: &str,
    category: &str,
    description: &str,
    tags: &[&str],
    formats: &[&str],
    price_cents: i64,
    currency: &str,
    is_paid: bool,
) -> serde_json::Value {
    json!({
        "slug": slug,
        "name": name,
        "vendor": vendor,
        "version": version,
        "category": category,
        "description": description,
        "tags": tags,
        "formats": formats,
        "price_cents": price_cents,
        "currency": currency,
        "is_paid": is_paid,
    })
}

fn plugin_archive_bytes(slug: &str) -> Vec<u8> {
    let mut cursor = std::io::Cursor::new(Vec::new());
    let mut archive = zip::ZipWriter::new(&mut cursor);
    let options = zip::write::SimpleFileOptions::default().unix_permissions(0o755);
    let bundle_dir = format!("{slug}.component/");
    let contents_dir = format!("{bundle_dir}Contents/");
    archive.add_directory(&bundle_dir, options).unwrap();
    archive.add_directory(&contents_dir, options).unwrap();
    archive
        .start_file(format!("{contents_dir}Info.plist"), options)
        .unwrap();
    archive
        .write_all(
            br#"<?xml version="1.0" encoding="UTF-8"?><plist version="1.0"><dict><key>CFBundleName</key><string>Test Plugin</string></dict></plist>"#,
        )
        .unwrap();
    archive.finish().unwrap();
    cursor.into_inner()
}
