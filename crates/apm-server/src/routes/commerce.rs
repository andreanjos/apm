use apm_core::license::{LicensePayload, LicenseStatus, SignedLicense};
use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::Html,
    routing::{get, post},
    Json, Router,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::Row;

use crate::{
    routes::{
        auth::{api_error, authenticate, require_api_key, require_scope, ApiError, SCOPE_PURCHASE},
        AppState,
    },
    stripe::{create_checkout_session, verify_webhook},
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/agent/purchase", post(agent_purchase))
        .route("/checkout", post(create_checkout))
        .route("/compare", post(compare_plugins))
        .route("/explore", get(explore_sections))
        .route("/featured", get(featured_sections))
        .route("/orders/{order_id}", get(order_status))
        .route("/licenses", get(list_licenses))
        .route("/refunds", post(create_refund))
        .route("/restore", post(restore_manifest))
        .route("/webhooks/stripe", post(handle_stripe_webhook))
        .route("/success", get(success_page))
        .route("/cancel", get(cancel_page))
}

type ApiResult<T> = Result<Json<T>, (StatusCode, Json<ApiError>)>;

#[derive(Debug, Serialize)]
struct FeaturedResponse {
    sections: Vec<StorefrontSection>,
}

#[derive(Debug, Serialize)]
struct ExploreResponse {
    categories: Vec<StorefrontSection>,
}

#[derive(Debug, Serialize)]
struct StorefrontSection {
    slug: String,
    title: String,
    description: Option<String>,
    plugins: Vec<StorefrontPlugin>,
}

#[derive(Debug, Serialize)]
struct StorefrontPlugin {
    slug: String,
    name: String,
    vendor: String,
    version: String,
    category: String,
    description: String,
    tags: Vec<String>,
    formats: Vec<String>,
    price_cents: i64,
    currency: String,
    is_paid: bool,
}

#[derive(Debug, Deserialize)]
struct CompareRequest {
    left_slug: String,
    right_slug: String,
}

#[derive(Debug, Serialize)]
struct CompareResponse {
    left: StorefrontPlugin,
    right: StorefrontPlugin,
}

#[derive(Debug, Deserialize)]
struct AgentPurchaseRequest {
    plugin_slug: String,
    idempotency_key: String,
}

#[derive(Debug, Serialize)]
struct AgentPurchaseResponse {
    transaction_id: String,
    order_id: i64,
    plugin_slug: String,
    status: String,
    fulfilled: bool,
    install_ready: bool,
    cost_cents: i64,
    currency: String,
    license_token: Option<String>,
    license: Option<SignedLicense>,
    download_token: Option<String>,
}

async fn featured_sections(State(state): State<AppState>) -> ApiResult<FeaturedResponse> {
    Ok(Json(FeaturedResponse {
        sections: load_storefront_sections(&state, "featured").await?,
    }))
}

async fn explore_sections(State(state): State<AppState>) -> ApiResult<ExploreResponse> {
    Ok(Json(ExploreResponse {
        categories: load_storefront_sections(&state, "explore").await?,
    }))
}

async fn compare_plugins(
    State(state): State<AppState>,
    Json(request): Json<CompareRequest>,
) -> ApiResult<CompareResponse> {
    if request.left_slug == request.right_slug {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "COMPARE_REQUIRES_DISTINCT_PLUGINS",
            "Compare requires two distinct plugin slugs.",
        ));
    }

    let left = load_storefront_plugin(&state, &request.left_slug).await?;
    let right = load_storefront_plugin(&state, &request.right_slug).await?;

    Ok(Json(CompareResponse { left, right }))
}

async fn agent_purchase(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<AgentPurchaseRequest>,
) -> Result<Json<AgentPurchaseResponse>, (StatusCode, Json<serde_json::Value>)> {
    let user = authenticate(&state, &headers)
        .await
        .map_err(|(status, Json(error))| {
            (
                status,
                Json(serde_json::to_value(error).unwrap_or_else(|_| serde_json::json!({}))),
            )
        })?;
    let api_key_id = require_api_key(&user).map_err(error_json)?;
    require_scope(&user, SCOPE_PURCHASE).map_err(error_json)?;

    let product = sqlx::query(
        r#"
        SELECT slug, price_cents, currency
        FROM catalog_products
        WHERE slug = $1 AND is_paid = TRUE AND active = TRUE
        "#,
    )
    .bind(&request.plugin_slug)
    .fetch_optional(&state.pool)
    .await
    .map_err(internal_error_json)?;

    let product = product.ok_or_else(|| {
        agent_denial(
            StatusCode::NOT_FOUND,
            "PRODUCT_NOT_FOUND",
            format!(
                "No paid catalog product exists for '{}'.",
                request.plugin_slug
            ),
            None,
        )
    })?;

    let price_cents: i64 = product.get("price_cents");
    let currency: String = product.get("currency");

    let existing = sqlx::query(
        r#"
        SELECT o.id AS order_id,
               o.checkout_session_id AS transaction_id
        FROM purchase_intents pi
        JOIN orders o ON o.purchase_intent_id = pi.id
        WHERE pi.user_id = $1 AND pi.plugin_slug = $2 AND pi.idempotency_key = $3
        ORDER BY pi.id DESC
        LIMIT 1
        "#,
    )
    .bind(user.user_id)
    .bind(&request.plugin_slug)
    .bind(&request.idempotency_key)
    .fetch_optional(&state.pool)
    .await
    .map_err(internal_error_json)?;

    if let Some(existing) = existing {
        let order_id: i64 = existing.get("order_id");
        let order = query_order_status(&state, user.user_id, order_id)
            .await
            .map_err(error_json)?;
        return Ok(Json(agent_purchase_response(
            existing.get("transaction_id"),
            price_cents,
            &currency,
            order,
        )));
    }

    let mut tx = state.pool.begin().await.map_err(internal_error_json)?;
    let policy = ensure_and_lock_api_key_policy(&mut tx, api_key_id)
        .await
        .map_err(error_json)?;

    let updated_period_spent_cents =
        if policy.period_started_at + chrono::Duration::days(30) <= Utc::now() {
            0
        } else {
            policy.period_spent_cents
        };

    if policy
        .per_transaction_limit_cents
        .map(|limit| price_cents > limit)
        .unwrap_or(false)
    {
        record_agent_purchase_attempt(
            &mut tx,
            api_key_id,
            user.user_id,
            &request.plugin_slug,
            price_cents,
            &currency,
            "denied",
            Some("PER_TRANSACTION_LIMIT_EXCEEDED"),
            None,
        )
        .await
        .map_err(error_json)?;
        tx.commit().await.map_err(internal_error_json)?;
        return Err(agent_denial(
            StatusCode::FORBIDDEN,
            "PER_TRANSACTION_LIMIT_EXCEEDED",
            "Agent purchase exceeds the per-transaction spending limit.",
            Some(serde_json::json!({
                "price_cents": price_cents,
                "limit_cents": policy.per_transaction_limit_cents,
            })),
        ));
    }

    if policy
        .period_limit_cents
        .map(|limit| updated_period_spent_cents + price_cents > limit)
        .unwrap_or(false)
    {
        record_agent_purchase_attempt(
            &mut tx,
            api_key_id,
            user.user_id,
            &request.plugin_slug,
            price_cents,
            &currency,
            "denied",
            Some("PERIOD_LIMIT_EXCEEDED"),
            None,
        )
        .await
        .map_err(error_json)?;
        tx.commit().await.map_err(internal_error_json)?;
        return Err(agent_denial(
            StatusCode::FORBIDDEN,
            "PERIOD_LIMIT_EXCEEDED",
            "Agent purchase exceeds the configured spending period limit.",
            Some(serde_json::json!({
                "price_cents": price_cents,
                "limit_cents": policy.period_limit_cents,
                "spent_cents": updated_period_spent_cents,
            })),
        ));
    }

    if !policy.preauthorized_payment_method {
        record_agent_purchase_attempt(
            &mut tx,
            api_key_id,
            user.user_id,
            &request.plugin_slug,
            price_cents,
            &currency,
            "denied",
            Some("PREAUTHORIZED_PAYMENT_REQUIRED"),
            None,
        )
        .await
        .map_err(error_json)?;
        tx.commit().await.map_err(internal_error_json)?;
        return Err(agent_denial(
            StatusCode::PRECONDITION_REQUIRED,
            "PREAUTHORIZED_PAYMENT_REQUIRED",
            "A pre-authorized payment method is required for non-interactive agent purchase.",
            Some(serde_json::json!({
                "price_cents": price_cents,
                "currency": currency,
            })),
        ));
    }

    let purchase_intent_id: i64 = sqlx::query(
        r#"
        INSERT INTO purchase_intents (user_id, plugin_slug, idempotency_key, status)
        VALUES ($1, $2, $3, 'agent_created')
        RETURNING id
        "#,
    )
    .bind(user.user_id)
    .bind(&request.plugin_slug)
    .bind(&request.idempotency_key)
    .fetch_one(&mut *tx)
    .await
    .map_err(internal_error_json)?
    .get("id");

    let transaction_id = format!("agent_tx_{}", uuid::Uuid::new_v4());
    let order_id: i64 = sqlx::query(
        r#"
        INSERT INTO orders (
            user_id, plugin_slug, purchase_intent_id, checkout_session_id, status
        )
        VALUES ($1, $2, $3, $4, 'pending')
        RETURNING id
        "#,
    )
    .bind(user.user_id)
    .bind(&request.plugin_slug)
    .bind(purchase_intent_id)
    .bind(&transaction_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(internal_error_json)?
    .get("id");

    sqlx::query(
        r#"
        UPDATE api_key_purchase_policies
        SET period_spent_cents = $2,
            period_started_at = CASE
                WHEN period_started_at + INTERVAL '30 days' <= NOW() THEN NOW()
                ELSE period_started_at
            END,
            updated_at = NOW()
        WHERE api_key_id = $1
        "#,
    )
    .bind(api_key_id)
    .bind(updated_period_spent_cents + price_cents)
    .execute(&mut *tx)
    .await
    .map_err(internal_error_json)?;

    record_agent_purchase_attempt(
        &mut tx,
        api_key_id,
        user.user_id,
        &request.plugin_slug,
        price_cents,
        &currency,
        "authorized",
        None,
        Some(order_id),
    )
    .await
    .map_err(error_json)?;

    tx.commit().await.map_err(internal_error_json)?;

    fulfill_order(
        &state,
        order_id,
        user.user_id,
        &request.plugin_slug,
        &transaction_id,
    )
    .await
    .map_err(error_json)?;

    let order = query_order_status(&state, user.user_id, order_id)
        .await
        .map_err(error_json)?;
    Ok(Json(agent_purchase_response(
        transaction_id,
        price_cents,
        &currency,
        order,
    )))
}

#[derive(Debug, Deserialize)]
struct CheckoutRequest {
    plugin_slug: String,
    idempotency_key: String,
}

#[derive(Debug, Serialize)]
struct CheckoutResponse {
    order_id: i64,
    checkout_session_id: String,
    checkout_url: String,
    idempotency_key: String,
    tax_enabled: bool,
}

async fn create_checkout(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<CheckoutRequest>,
) -> ApiResult<CheckoutResponse> {
    let user = authenticate(&state, &headers).await?;
    require_scope(&user, SCOPE_PURCHASE)?;

    let product = sqlx::query(
        r#"
        SELECT slug, price_cents, currency
        FROM catalog_products
        WHERE slug = $1 AND is_paid = TRUE AND active = TRUE
        "#,
    )
    .bind(&request.plugin_slug)
    .fetch_optional(&state.pool)
    .await
    .map_err(internal_error)?;

    let product = product.ok_or_else(|| {
        api_error(
            StatusCode::NOT_FOUND,
            "not_found",
            "PRODUCT_NOT_FOUND",
            format!(
                "No paid catalog product exists for '{}'.",
                request.plugin_slug
            ),
        )
    })?;

    let existing = sqlx::query(
        r#"
        SELECT o.id AS order_id, o.checkout_session_id, o.checkout_url
        FROM purchase_intents pi
        JOIN orders o ON o.purchase_intent_id = pi.id
        WHERE pi.user_id = $1 AND pi.plugin_slug = $2 AND pi.idempotency_key = $3
        ORDER BY pi.id DESC
        LIMIT 1
        "#,
    )
    .bind(user.user_id)
    .bind(&request.plugin_slug)
    .bind(&request.idempotency_key)
    .fetch_optional(&state.pool)
    .await
    .map_err(internal_error)?;

    if let Some(existing) = existing {
        let order_id: i64 = existing.get("order_id");
        let checkout_session_id: String = existing.get("checkout_session_id");
        return Ok(Json(CheckoutResponse {
            order_id,
            checkout_session_id,
            checkout_url: existing.get("checkout_url"),
            idempotency_key: request.idempotency_key,
            tax_enabled: true,
        }));
    }

    let session = create_checkout_session(
        &state.stripe,
        &request.plugin_slug,
        product.get("price_cents"),
        &product.get::<String, _>("currency"),
        &request.idempotency_key,
    )
    .await
    .map_err(internal_error)?;

    let purchase_intent_id: i64 = sqlx::query(
        r#"
        INSERT INTO purchase_intents (user_id, plugin_slug, idempotency_key, status)
        VALUES ($1, $2, $3, 'pending')
        RETURNING id
        "#,
    )
    .bind(user.user_id)
    .bind(&request.plugin_slug)
    .bind(&request.idempotency_key)
    .fetch_one(&state.pool)
    .await
    .map_err(internal_error)?
    .get("id");

    let order = sqlx::query(
        r#"
        INSERT INTO orders (
            user_id, plugin_slug, purchase_intent_id, checkout_session_id, checkout_url, status
        )
        VALUES ($1, $2, $3, $4, $5, 'pending')
        RETURNING id
        "#,
    )
    .bind(user.user_id)
    .bind(&request.plugin_slug)
    .bind(purchase_intent_id)
    .bind(&session.session_id)
    .bind(&session.checkout_url)
    .fetch_one(&state.pool)
    .await
    .map_err(internal_error)?;

    Ok(Json(CheckoutResponse {
        order_id: order.get("id"),
        checkout_session_id: session.session_id,
        checkout_url: session.checkout_url,
        idempotency_key: request.idempotency_key,
        tax_enabled: session.tax_enabled,
    }))
}

#[derive(Debug, Serialize)]
struct OrderStatusResponse {
    order_id: i64,
    plugin_slug: String,
    status: String,
    license_token: Option<String>,
    license: Option<SignedLicense>,
    download_token: Option<String>,
    checkout_session_id: String,
    refunded_at: Option<DateTime<Utc>>,
}

async fn order_status(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(order_id): Path<i64>,
) -> ApiResult<OrderStatusResponse> {
    let user = authenticate(&state, &headers).await?;
    require_scope(&user, SCOPE_PURCHASE)?;
    Ok(Json(
        query_order_status(&state, user.user_id, order_id).await?,
    ))
}

#[derive(Debug, Serialize)]
struct LicenseListResponse {
    public_key_hex: String,
    licenses: Vec<SignedLicenseRecord>,
}

#[derive(Debug, Serialize)]
struct SignedLicenseRecord {
    plugin_slug: String,
    order_id: i64,
    status: String,
    license: SignedLicense,
    activated_at: Option<DateTime<Utc>>,
    revoked_at: Option<DateTime<Utc>>,
}

async fn list_licenses(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<LicenseListResponse> {
    let user = authenticate(&state, &headers).await?;
    require_scope(&user, SCOPE_PURCHASE)?;
    let rows = sqlx::query(
        r#"
        SELECT l.plugin_slug,
               l.order_id,
               l.status,
               l.license_json,
               l.signature,
               l.key_id,
               l.activated_at,
               l.revoked_at
        FROM licenses l
        WHERE l.user_id = $1
        ORDER BY l.created_at DESC
        "#,
    )
    .bind(user.user_id)
    .fetch_all(&state.pool)
    .await
    .map_err(internal_error)?;

    let licenses = rows
        .into_iter()
        .map(|row| {
            Ok(SignedLicenseRecord {
                plugin_slug: row.get("plugin_slug"),
                order_id: row.get("order_id"),
                status: row.get("status"),
                license: signed_license_from_row(&row)?
                    .ok_or_else(|| internal_error("license row missing signed payload"))?,
                activated_at: row.get("activated_at"),
                revoked_at: row.get("revoked_at"),
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    Ok(Json(LicenseListResponse {
        public_key_hex: state.license.public_key_hex().to_string(),
        licenses,
    }))
}

#[derive(Debug, Deserialize)]
struct RefundRequest {
    order_id: i64,
}

#[derive(Debug, Serialize)]
struct RefundResponse {
    order_id: i64,
    refunded: bool,
    status: String,
}

async fn create_refund(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<RefundRequest>,
) -> ApiResult<RefundResponse> {
    let user = authenticate(&state, &headers).await?;
    require_scope(&user, SCOPE_PURCHASE)?;
    let order = sqlx::query("SELECT id, status FROM orders WHERE id = $1 AND user_id = $2")
        .bind(request.order_id)
        .bind(user.user_id)
        .fetch_optional(&state.pool)
        .await
        .map_err(internal_error)?;

    let order = order.ok_or_else(|| {
        api_error(
            StatusCode::NOT_FOUND,
            "not_found",
            "ORDER_NOT_FOUND",
            format!("No order exists for id {}.", request.order_id),
        )
    })?;

    sqlx::query(
        r#"
        UPDATE orders
        SET status = 'refunded', refunded_at = NOW(), fulfilled_at = NULL
        WHERE id = $1
        "#,
    )
    .bind(order.get::<i64, _>("id"))
    .execute(&state.pool)
    .await
    .map_err(internal_error)?;

    sqlx::query(
        r#"
        UPDATE licenses
        SET status = 'refunded',
            revoked_at = NOW()
        WHERE order_id = $1 AND user_id = $2
        "#,
    )
    .bind(request.order_id)
    .bind(user.user_id)
    .execute(&state.pool)
    .await
    .map_err(internal_error)?;

    Ok(Json(RefundResponse {
        order_id: request.order_id,
        refunded: true,
        status: "refunded".to_string(),
    }))
}

#[derive(Debug, Serialize)]
struct RestoreResponse {
    public_key_hex: String,
    restorable_plugins: Vec<RestoreItem>,
}

#[derive(Debug, Serialize)]
struct RestoreItem {
    plugin_slug: String,
    order_id: i64,
    download_token: Option<String>,
    license: SignedLicense,
}

async fn restore_manifest(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<RestoreResponse> {
    let user = authenticate(&state, &headers).await?;
    require_scope(&user, SCOPE_PURCHASE)?;
    let rows = sqlx::query(
        r#"
        SELECT l.plugin_slug,
               l.order_id,
               o.download_token,
               l.license_json,
               l.signature,
               l.key_id
        FROM licenses l
        JOIN orders o ON o.id = l.order_id
        WHERE l.user_id = $1 AND l.status = 'active'
        ORDER BY l.created_at DESC
        "#,
    )
    .bind(user.user_id)
    .fetch_all(&state.pool)
    .await
    .map_err(internal_error)?;

    let restorable_plugins = rows
        .into_iter()
        .map(|row| {
            Ok(RestoreItem {
                plugin_slug: row.get("plugin_slug"),
                order_id: row.get("order_id"),
                download_token: row.get("download_token"),
                license: signed_license_from_row(&row)?
                    .ok_or_else(|| internal_error("restore row missing signed license"))?,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    Ok(Json(RestoreResponse {
        public_key_hex: state.license.public_key_hex().to_string(),
        restorable_plugins,
    }))
}

async fn handle_stripe_webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    payload: String,
) -> ApiResult<serde_json::Value> {
    let signature = headers
        .get("stripe-signature")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();

    let event = verify_webhook(&state.stripe, &payload, signature).map_err(internal_error)?;

    let already_processed = sqlx::query("SELECT event_id FROM stripe_events WHERE event_id = $1")
        .bind(&event.id)
        .fetch_optional(&state.pool)
        .await
        .map_err(internal_error)?;

    if already_processed.is_some() {
        return Ok(Json(
            serde_json::json!({"processed": true, "duplicate": true}),
        ));
    }

    sqlx::query("INSERT INTO stripe_events (event_id, event_type, payload) VALUES ($1, $2, $3)")
        .bind(&event.id)
        .bind(&event.event_type)
        .bind(&payload)
        .execute(&state.pool)
        .await
        .map_err(internal_error)?;

    if event.event_type == "checkout.session.completed" {
        let session_id = event.checkout_session_id.ok_or_else(|| {
            api_error(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                "MISSING_CHECKOUT_SESSION",
                "checkout.session.completed event must include a checkout_session_id",
            )
        })?;
        let row = sqlx::query(
            r#"
            SELECT id, user_id, plugin_slug
            FROM orders
            WHERE checkout_session_id = $1
            "#,
        )
        .bind(&session_id)
        .fetch_one(&state.pool)
        .await
        .map_err(internal_error)?;

        fulfill_order(
            &state,
            row.get("id"),
            row.get("user_id"),
            &row.get::<String, _>("plugin_slug"),
            &session_id,
        )
        .await?;
    }

    Ok(Json(
        serde_json::json!({"processed": true, "duplicate": false}),
    ))
}

async fn success_page() -> Html<&'static str> {
    Html("<html><body><p>Payment received. Waiting for webhook fulfillment.</p></body></html>")
}

async fn cancel_page() -> Html<&'static str> {
    Html("<html><body><p>Checkout canceled.</p></body></html>")
}

#[derive(Debug)]
struct ApiKeyPolicy {
    preauthorized_payment_method: bool,
    per_transaction_limit_cents: Option<i64>,
    period_limit_cents: Option<i64>,
    period_spent_cents: i64,
    period_started_at: DateTime<Utc>,
}

async fn ensure_and_lock_api_key_policy(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    api_key_id: i64,
) -> Result<ApiKeyPolicy, (StatusCode, Json<ApiError>)> {
    sqlx::query(
        r#"
        INSERT INTO api_key_purchase_policies (api_key_id)
        VALUES ($1)
        ON CONFLICT (api_key_id) DO NOTHING
        "#,
    )
    .bind(api_key_id)
    .execute(&mut **tx)
    .await
    .map_err(internal_error)?;

    let row = sqlx::query(
        r#"
        SELECT preauthorized_payment_method,
               per_transaction_limit_cents,
               period_limit_cents,
               period_spent_cents,
               period_started_at
        FROM api_key_purchase_policies
        WHERE api_key_id = $1
        FOR UPDATE
        "#,
    )
    .bind(api_key_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(internal_error)?;

    Ok(ApiKeyPolicy {
        preauthorized_payment_method: row.get("preauthorized_payment_method"),
        per_transaction_limit_cents: row.get("per_transaction_limit_cents"),
        period_limit_cents: row.get("period_limit_cents"),
        period_spent_cents: row.get("period_spent_cents"),
        period_started_at: row.get("period_started_at"),
    })
}

#[allow(clippy::too_many_arguments)]
async fn record_agent_purchase_attempt(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    api_key_id: i64,
    user_id: i64,
    plugin_slug: &str,
    amount_cents: i64,
    currency: &str,
    outcome: &str,
    denial_code: Option<&str>,
    order_id: Option<i64>,
) -> Result<(), (StatusCode, Json<ApiError>)> {
    sqlx::query(
        r#"
        INSERT INTO agent_purchase_attempts (
            api_key_id, user_id, plugin_slug, amount_cents, currency, outcome, denial_code, order_id
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
        "#,
    )
    .bind(api_key_id)
    .bind(user_id)
    .bind(plugin_slug)
    .bind(amount_cents)
    .bind(currency)
    .bind(outcome)
    .bind(denial_code)
    .bind(order_id)
    .execute(&mut **tx)
    .await
    .map_err(internal_error)?;
    Ok(())
}

async fn fulfill_order(
    state: &AppState,
    order_id: i64,
    user_id: i64,
    plugin_slug: &str,
    checkout_session_id: &str,
) -> Result<(), (StatusCode, Json<ApiError>)> {
    let license_id = format!("lic_{order_id}");
    let signed_license = state
        .license
        .sign_license(LicensePayload {
            schema_version: 1,
            license_id: license_id.clone(),
            user_id,
            plugin_slug: plugin_slug.to_string(),
            plugin_version: None,
            order_id,
            issued_at: Utc::now(),
            revoked_at: None,
            status: LicenseStatus::Active,
        })
        .map_err(internal_error)?;

    let mut tx = state.pool.begin().await.map_err(internal_error)?;
    sqlx::query(
        r#"
        UPDATE orders
        SET status = 'fulfilled',
            fulfilled_at = NOW(),
            license_token = COALESCE(license_token, $2),
            download_token = COALESCE(download_token, $3)
        WHERE checkout_session_id = $1
        "#,
    )
    .bind(checkout_session_id)
    .bind(&license_id)
    .bind(format!("dl_{}", uuid::Uuid::new_v4()))
    .execute(&mut *tx)
    .await
    .map_err(internal_error)?;

    sqlx::query(
        r#"
        INSERT INTO licenses (user_id, plugin_slug, order_id, status, license_json, signature, key_id)
        VALUES ($1, $2, $3, 'active', $4, $5, $6)
        ON CONFLICT (order_id) DO NOTHING
        "#,
    )
    .bind(signed_license.payload.user_id)
    .bind(&signed_license.payload.plugin_slug)
    .bind(order_id)
    .bind(serde_json::to_value(&signed_license.payload).map_err(internal_error)?)
    .bind(&signed_license.signature)
    .bind(&signed_license.key_id)
    .execute(&mut *tx)
    .await
    .map_err(internal_error)?;
    tx.commit().await.map_err(internal_error)?;
    Ok(())
}

async fn query_order_status(
    state: &AppState,
    user_id: i64,
    order_id: i64,
) -> Result<OrderStatusResponse, (StatusCode, Json<ApiError>)> {
    let row = sqlx::query(
        r#"
        SELECT o.id,
               o.plugin_slug,
               o.status,
               o.license_token,
               o.download_token,
               o.checkout_session_id,
               o.refunded_at,
               l.license_json,
               l.signature,
               l.key_id
        FROM orders
        LEFT JOIN licenses l ON l.order_id = o.id
        WHERE o.id = $1 AND o.user_id = $2
        "#,
    )
    .bind(order_id)
    .bind(user_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(internal_error)?;

    let row = row.ok_or_else(|| {
        api_error(
            StatusCode::NOT_FOUND,
            "not_found",
            "ORDER_NOT_FOUND",
            format!("No order exists for id {order_id}."),
        )
    })?;

    Ok(OrderStatusResponse {
        order_id: row.get("id"),
        plugin_slug: row.get("plugin_slug"),
        status: row.get("status"),
        license_token: row.get("license_token"),
        license: signed_license_from_row(&row)?,
        download_token: row.get("download_token"),
        checkout_session_id: row.get("checkout_session_id"),
        refunded_at: row.get("refunded_at"),
    })
}

fn agent_purchase_response(
    transaction_id: String,
    cost_cents: i64,
    currency: &str,
    order: OrderStatusResponse,
) -> AgentPurchaseResponse {
    AgentPurchaseResponse {
        transaction_id,
        order_id: order.order_id,
        plugin_slug: order.plugin_slug,
        status: order.status.clone(),
        fulfilled: order.status == "fulfilled",
        install_ready: order.license.is_some() && order.download_token.is_some(),
        cost_cents,
        currency: currency.to_string(),
        license_token: order.license_token,
        license: order.license,
        download_token: order.download_token,
    }
}

fn error_json(error: (StatusCode, Json<ApiError>)) -> (StatusCode, Json<serde_json::Value>) {
    let (status, Json(error)) = error;
    (
        status,
        Json(serde_json::to_value(error).unwrap_or_else(|_| serde_json::json!({}))),
    )
}

fn internal_error_json(error: impl std::fmt::Display) -> (StatusCode, Json<serde_json::Value>) {
    error_json(internal_error(error))
}

fn agent_denial(
    status: StatusCode,
    code: &'static str,
    message: impl Into<String>,
    details: Option<serde_json::Value>,
) -> (StatusCode, Json<serde_json::Value>) {
    let mut body = serde_json::json!({
        "error": "forbidden",
        "code": code,
        "message": message.into(),
    });
    if let Some(details) = details {
        body["details"] = details;
    }
    (status, Json(body))
}

fn internal_error(error: impl std::fmt::Display) -> (StatusCode, Json<ApiError>) {
    tracing::error!(error = %error, "commerce route failed");
    api_error(
        StatusCode::INTERNAL_SERVER_ERROR,
        "internal_error",
        "INTERNAL_SERVER_ERROR",
        "An internal server error occurred.",
    )
}

fn signed_license_from_row(
    row: &sqlx::postgres::PgRow,
) -> Result<Option<SignedLicense>, (StatusCode, Json<ApiError>)> {
    let payload: Option<serde_json::Value> = row.try_get("license_json").map_err(internal_error)?;
    let signature: Option<String> = row.try_get("signature").map_err(internal_error)?;
    let key_id: Option<String> = row.try_get("key_id").map_err(internal_error)?;

    match (payload, signature, key_id) {
        (Some(payload), Some(signature), Some(key_id)) => {
            let payload = serde_json::from_value(payload).map_err(internal_error)?;
            Ok(Some(SignedLicense {
                payload,
                signature,
                key_id,
            }))
        }
        _ => Ok(None),
    }
}

async fn load_storefront_sections(
    state: &AppState,
    kind: &str,
) -> Result<Vec<StorefrontSection>, (StatusCode, Json<ApiError>)> {
    let rows = sqlx::query(
        r#"
        SELECT s.slug AS section_slug,
               s.title AS section_title,
               s.description AS section_description,
               p.slug AS plugin_slug,
               p.name,
               p.vendor,
               p.version,
               p.category,
               p.description,
               p.tags,
               p.formats,
               cp.price_cents,
               cp.currency,
               cp.is_paid
        FROM storefront_sections s
        JOIN storefront_section_items si ON si.section_id = s.id
        JOIN storefront_plugins p ON p.slug = si.plugin_slug
        JOIN catalog_products cp ON cp.slug = p.slug
        WHERE s.kind = $1 AND cp.active = TRUE
        ORDER BY s.sort_order ASC, si.sort_order ASC, p.slug ASC
        "#,
    )
    .bind(kind)
    .fetch_all(&state.pool)
    .await
    .map_err(internal_error)?;

    let mut sections = Vec::<StorefrontSection>::new();
    for row in rows {
        let section_slug: String = row.get("section_slug");
        let plugin = storefront_plugin_from_row(&row)?;

        if let Some(section) = sections
            .iter_mut()
            .find(|section| section.slug == section_slug)
        {
            section.plugins.push(plugin);
        } else {
            sections.push(StorefrontSection {
                slug: section_slug,
                title: row.get("section_title"),
                description: row.get("section_description"),
                plugins: vec![plugin],
            });
        }
    }

    Ok(sections)
}

async fn load_storefront_plugin(
    state: &AppState,
    slug: &str,
) -> Result<StorefrontPlugin, (StatusCode, Json<ApiError>)> {
    let row = sqlx::query(
        r#"
        SELECT p.slug AS plugin_slug,
               p.name,
               p.vendor,
               p.version,
               p.category,
               p.description,
               p.tags,
               p.formats,
               cp.price_cents,
               cp.currency,
               cp.is_paid
        FROM storefront_plugins p
        JOIN catalog_products cp ON cp.slug = p.slug
        WHERE p.slug = $1 AND cp.active = TRUE
        "#,
    )
    .bind(slug)
    .fetch_optional(&state.pool)
    .await
    .map_err(internal_error)?;

    let row = row.ok_or_else(|| {
        api_error(
            StatusCode::NOT_FOUND,
            "not_found",
            "STOREFRONT_PLUGIN_NOT_FOUND",
            format!("No storefront plugin exists for '{}'.", slug),
        )
    })?;

    storefront_plugin_from_row(&row)
}

fn storefront_plugin_from_row(
    row: &sqlx::postgres::PgRow,
) -> Result<StorefrontPlugin, (StatusCode, Json<ApiError>)> {
    Ok(StorefrontPlugin {
        slug: row.get("plugin_slug"),
        name: row.get("name"),
        vendor: row.get("vendor"),
        version: row.get("version"),
        category: row.get("category"),
        description: row.get("description"),
        tags: serde_json::from_value(row.get("tags")).map_err(internal_error)?,
        formats: serde_json::from_value(row.get("formats")).map_err(internal_error)?,
        price_cents: row.get("price_cents"),
        currency: row.get("currency"),
        is_paid: row.get("is_paid"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn internal_error_hides_backend_details() {
        let (status, Json(error)) = internal_error("sqlx pool timed out");
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(error.code, "INTERNAL_SERVER_ERROR");
        assert_eq!(error.message, "An internal server error occurred.");
    }
}
