use apm_core::verify_signed_license;
use apm_server::{
    auth::{api_keys::hash_secret, AuthConfig},
    license::LicenseConfig,
    routes::{self, AppState},
    stripe::StripeConfig,
};
use axum::{
    body::{to_bytes, Body},
    http::{Method, Request, StatusCode},
};
use serde_json::{json, Value};
use sqlx::{PgPool, Row};
use tower::ServiceExt;

async fn test_pool() -> Option<PgPool> {
    let database_url = match std::env::var("DATABASE_URL") {
        Ok(value) if !value.trim().is_empty() => value,
        _ => return None,
    };

    let pool = PgPool::connect(&database_url).await.ok()?;
    sqlx::migrate!("../../migrations").run(&pool).await.ok()?;
    sqlx::query("TRUNCATE agent_purchase_attempts, api_key_purchase_policies, licenses, stripe_events, orders, purchase_intents, catalog_products, api_keys, refresh_tokens, device_authorizations, users RESTART IDENTITY CASCADE")
        .execute(&pool)
        .await
        .ok()?;
    Some(pool)
}

fn state(pool: PgPool) -> AppState {
    AppState {
        pool,
        auth: AuthConfig {
            public_base_url: "http://127.0.0.1:3000".to_string(),
            jwt_secret: "test-secret".to_string(),
            access_token_ttl_seconds: 900,
            refresh_token_ttl_seconds: 3600,
            device_code_ttl_seconds: 600,
        },
        stripe: StripeConfig {
            secret_key: "sk_test_mock".to_string(),
            webhook_secret: "whsec_mock".to_string(),
            success_url: "http://127.0.0.1:3000/commerce/success".to_string(),
            cancel_url: "http://127.0.0.1:3000/commerce/cancel".to_string(),
        },
        license: LicenseConfig::from_env().unwrap(),
    }
}

async fn seed_user_and_api_key(pool: &PgPool) -> String {
    seed_user_and_api_key_with_name_and_scopes(pool, "license", "read,purchase").await
}

async fn seed_user_and_api_key_with_name_and_scopes(
    pool: &PgPool,
    name: &str,
    scopes: &str,
) -> String {
    let user_id: i64 =
        sqlx::query(
            "INSERT INTO users (email, password_hash) VALUES ($1, $2) ON CONFLICT (email) DO UPDATE SET email = EXCLUDED.email RETURNING id"
        )
            .bind("licensed@example.com")
            .bind("ignored")
            .fetch_one(pool)
            .await
            .unwrap()
            .get("id");

    let api_key = format!("apm_live_{name}_secret");
    sqlx::query(
        "INSERT INTO api_keys (user_id, name, key_prefix, key_hash, scopes) VALUES ($1, $2, $3, $4, $5)"
    )
    .bind(user_id)
    .bind(name)
    .bind(name)
    .bind(hash_secret(&api_key))
    .bind(scopes)
    .execute(pool)
    .await
    .unwrap();

    api_key
}

async fn seed_product(pool: &PgPool, slug: &str) {
    sqlx::query(
        "INSERT INTO catalog_products (slug, price_cents, currency, is_paid, active) VALUES ($1, $2, $3, TRUE, TRUE)"
    )
    .bind(slug)
    .bind(4999_i64)
    .bind("usd")
    .execute(pool)
    .await
    .unwrap();
}

async fn json_response(response: axum::response::Response) -> Value {
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

async fn create_fulfilled_order(app: &axum::Router, api_key: &str, idempotency_key: &str) -> Value {
    let checkout = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/commerce/checkout")
                .header("content-type", "application/json")
                .header("x-apm-api-key", api_key)
                .body(Body::from(
                    json!({"plugin_slug":"paid-plugin","idempotency_key": idempotency_key})
                        .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let checkout_json = json_response(checkout).await;
    let session_id = checkout_json["checkout_session_id"]
        .as_str()
        .unwrap()
        .to_string();

    let webhook = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/commerce/webhooks/stripe")
                .header("stripe-signature", "test")
                .body(Body::from(
                    json!({
                        "id": format!("evt_{idempotency_key}"),
                        "event_type": "checkout.session.completed",
                        "checkout_session_id": session_id,
                        "refund_id": null
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(webhook.status(), StatusCode::OK);
    checkout_json
}

#[tokio::test]
async fn issuance_returns_verifiable_signed_license() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping license_routes_tests: DATABASE_URL unavailable");
        return;
    };
    let api_key = seed_user_and_api_key(&pool).await;
    seed_product(&pool, "paid-plugin").await;
    let state = state(pool);
    let public_key = state.license.public_key_hex().to_string();
    let app = routes::router(state);

    let checkout_json = create_fulfilled_order(&app, &api_key, "license-1").await;
    let order_id = checkout_json["order_id"].as_i64().unwrap();

    let status = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri(format!("/commerce/orders/{order_id}"))
                .header("x-apm-api-key", &api_key)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status_json = json_response(status).await;
    let license: apm_core::SignedLicense =
        serde_json::from_value(status_json["license"].clone()).unwrap();
    verify_signed_license(&public_key, &license).unwrap();
    assert_eq!(license.payload.plugin_slug, "paid-plugin");
    assert_eq!(license.payload.order_id, order_id);
}

#[tokio::test]
async fn sync_lists_signed_licenses_and_duplicate_webhook_reuses_existing_record() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping license_routes_tests: DATABASE_URL unavailable");
        return;
    };
    let api_key = seed_user_and_api_key(&pool).await;
    seed_product(&pool, "paid-plugin").await;
    let state = state(pool.clone());
    let public_key = state.license.public_key_hex().to_string();
    let app = routes::router(state);

    let checkout_json = create_fulfilled_order(&app, &api_key, "license-2").await;
    let session_id = checkout_json["checkout_session_id"]
        .as_str()
        .unwrap()
        .to_string();

    let duplicate = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/commerce/webhooks/stripe")
                .header("stripe-signature", "test")
                .body(Body::from(
                    json!({
                        "id": "evt_license-2",
                        "event_type": "checkout.session.completed",
                        "checkout_session_id": session_id,
                        "refund_id": null
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(duplicate.status(), StatusCode::OK);

    let license_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM licenses")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(license_count, 1);

    let sync = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/commerce/licenses")
                .header("x-apm-api-key", &api_key)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(sync.status(), StatusCode::OK);
    let sync_json = json_response(sync).await;
    assert_eq!(sync_json["licenses"].as_array().unwrap().len(), 1);
    let license: apm_core::SignedLicense =
        serde_json::from_value(sync_json["licenses"][0]["license"].clone()).unwrap();
    verify_signed_license(&public_key, &license).unwrap();
}

#[tokio::test]
async fn restore_returns_only_active_licenses() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping license_routes_tests: DATABASE_URL unavailable");
        return;
    };
    let api_key = seed_user_and_api_key(&pool).await;
    seed_product(&pool, "paid-plugin").await;
    let app = routes::router(state(pool.clone()));

    let checkout_json = create_fulfilled_order(&app, &api_key, "license-3").await;
    let order_id = checkout_json["order_id"].as_i64().unwrap();

    let restore_before = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/commerce/restore")
                .header("x-apm-api-key", &api_key)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(restore_before.status(), StatusCode::OK);
    let restore_before_json = json_response(restore_before).await;
    assert_eq!(
        restore_before_json["restorable_plugins"]
            .as_array()
            .unwrap()
            .len(),
        1
    );

    let refund = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/commerce/refunds")
                .header("content-type", "application/json")
                .header("x-apm-api-key", &api_key)
                .body(Body::from(json!({ "order_id": order_id }).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(refund.status(), StatusCode::OK);

    let restore_after = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/commerce/restore")
                .header("x-apm-api-key", &api_key)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let restore_after_json = json_response(restore_after).await;
    assert!(restore_after_json["restorable_plugins"]
        .as_array()
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn read_only_keys_cannot_list_licenses_or_restore() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping license_routes_tests: DATABASE_URL unavailable");
        return;
    };
    let purchase_key = seed_user_and_api_key(&pool).await;
    let read_key = seed_user_and_api_key_with_name_and_scopes(&pool, "reader", "read").await;
    seed_product(&pool, "paid-plugin").await;
    let app = routes::router(state(pool));

    create_fulfilled_order(&app, &purchase_key, "license-scope").await;

    let denied_licenses = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/commerce/licenses")
                .header("x-apm-api-key", &read_key)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(denied_licenses.status(), StatusCode::FORBIDDEN);
    let denied_licenses_json = json_response(denied_licenses).await;
    assert_eq!(denied_licenses_json["code"], "INSUFFICIENT_SCOPE");

    let denied_restore = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/commerce/restore")
                .header("x-apm-api-key", &read_key)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(denied_restore.status(), StatusCode::FORBIDDEN);
    let denied_restore_json = json_response(denied_restore).await;
    assert_eq!(denied_restore_json["code"], "INSUFFICIENT_SCOPE");
}

#[tokio::test]
async fn purchase_only_key_cannot_read_license_sync_routes() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping license_routes_tests: DATABASE_URL unavailable");
        return;
    };
    let user_id: i64 =
        sqlx::query("INSERT INTO users (email, password_hash) VALUES ($1, $2) RETURNING id")
            .bind("purchase-only@example.com")
            .bind("ignored")
            .fetch_one(&pool)
            .await
            .unwrap()
            .get("id");

    let api_key = "apm_live_purchase_only_secret";
    sqlx::query(
        "INSERT INTO api_keys (user_id, name, key_prefix, key_hash, scopes) VALUES ($1, $2, $3, $4, $5)"
    )
    .bind(user_id)
    .bind("purchase-only")
    .bind("purchase-only")
    .bind(hash_secret(api_key))
    .bind("purchase")
    .execute(&pool)
    .await
    .unwrap();

    seed_product(&pool, "paid-plugin").await;
    let app = routes::router(state(pool));
    let _ = create_fulfilled_order(&app, api_key, "license-read-scope").await;

    let licenses = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/commerce/licenses")
                .header("x-apm-api-key", api_key)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(licenses.status(), StatusCode::FORBIDDEN);
    let licenses_json = json_response(licenses).await;
    assert_eq!(licenses_json["code"], "INSUFFICIENT_SCOPE");

    let restore = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/commerce/restore")
                .header("x-apm-api-key", api_key)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(restore.status(), StatusCode::FORBIDDEN);
    let restore_json = json_response(restore).await;
    assert_eq!(restore_json["code"], "INSUFFICIENT_SCOPE");
}
