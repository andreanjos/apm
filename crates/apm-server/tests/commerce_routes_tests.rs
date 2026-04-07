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
    sqlx::query("TRUNCATE agent_purchase_attempts, api_key_purchase_policies, stripe_events, orders, purchase_intents, catalog_products, api_keys, refresh_tokens, device_authorizations, users RESTART IDENTITY CASCADE")
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
    seed_user_and_api_key_with_name_and_scopes(pool, "test", "read,purchase").await
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
            .bind("buyer@example.com")
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

#[tokio::test]
async fn checkout_is_idempotent() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping commerce_routes_tests: DATABASE_URL unavailable");
        return;
    };
    let api_key = seed_user_and_api_key(&pool).await;
    seed_product(&pool, "paid-plugin").await;
    let app = routes::router(state(pool));

    let request_body = json!({
        "plugin_slug": "paid-plugin",
        "idempotency_key": "intent-1"
    });

    let first = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/commerce/checkout")
                .header("content-type", "application/json")
                .header("x-apm-api-key", &api_key)
                .body(Body::from(request_body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(first.status(), StatusCode::OK);
    let first_json = json_response(first).await;

    let second = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/commerce/checkout")
                .header("content-type", "application/json")
                .header("x-apm-api-key", &api_key)
                .body(Body::from(request_body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(second.status(), StatusCode::OK);
    let second_json = json_response(second).await;

    assert_eq!(first_json["order_id"], second_json["order_id"]);
    assert_eq!(first_json["checkout_url"], second_json["checkout_url"]);
    assert_eq!(first_json["idempotency_key"], "intent-1");
    assert_eq!(first_json["tax_enabled"], true);
}

#[tokio::test]
async fn read_only_key_cannot_create_checkout() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping commerce_routes_tests: DATABASE_URL unavailable");
        return;
    };
    let user_id: i64 =
        sqlx::query("INSERT INTO users (email, password_hash) VALUES ($1, $2) RETURNING id")
            .bind("reader-checkout@example.com")
            .bind("ignored")
            .fetch_one(&pool)
            .await
            .unwrap()
            .get("id");
    let api_key = "apm_live_reader_checkout_secret";
    sqlx::query(
        "INSERT INTO api_keys (user_id, name, key_prefix, key_hash, scopes) VALUES ($1, $2, $3, $4, $5)"
    )
    .bind(user_id)
    .bind("reader-checkout")
    .bind("reader-checkout")
    .bind(hash_secret(api_key))
    .bind("read")
    .execute(&pool)
    .await
    .unwrap();
    seed_product(&pool, "paid-plugin").await;
    let app = routes::router(state(pool));

    let response = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/commerce/checkout")
                .header("content-type", "application/json")
                .header("x-apm-api-key", api_key)
                .body(Body::from(
                    json!({"plugin_slug":"paid-plugin","idempotency_key":"reader-intent"})
                        .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let json = json_response(response).await;
    assert_eq!(json["code"], "INSUFFICIENT_SCOPE");
}

#[tokio::test]
async fn webhook_drives_fulfillment_and_duplicate_delivery_is_safe() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping commerce_routes_tests: DATABASE_URL unavailable");
        return;
    };
    let api_key = seed_user_and_api_key(&pool).await;
    seed_product(&pool, "paid-plugin").await;
    let app = routes::router(state(pool.clone()));

    let checkout = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/commerce/checkout")
                .header("content-type", "application/json")
                .header("x-apm-api-key", &api_key)
                .body(Body::from(
                    json!({"plugin_slug":"paid-plugin","idempotency_key":"intent-2"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let checkout_json = json_response(checkout).await;
    let order_id = checkout_json["order_id"].as_i64().unwrap();
    let session_id = checkout_json["checkout_session_id"]
        .as_str()
        .unwrap()
        .to_string();

    let success = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/commerce/success")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(success.status(), StatusCode::OK);

    let before = app
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
    let before_json = json_response(before).await;
    assert_eq!(before_json["status"], "pending");

    let payload = json!({
        "id": "evt_test_1",
        "event_type": "checkout.session.completed",
        "checkout_session_id": session_id,
        "refund_id": null
    });

    let first_webhook = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/commerce/webhooks/stripe")
                .header("stripe-signature", "test")
                .body(Body::from(payload.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(first_webhook.status(), StatusCode::OK);

    let duplicate = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/commerce/webhooks/stripe")
                .header("stripe-signature", "test")
                .body(Body::from(payload.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(duplicate.status(), StatusCode::OK);
    let duplicate_json = json_response(duplicate).await;
    assert_eq!(duplicate_json["duplicate"], true);

    let after = app
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
    let after_json = json_response(after).await;
    assert_eq!(after_json["status"], "fulfilled");
    assert!(after_json["license_token"]
        .as_str()
        .unwrap()
        .starts_with("lic_"));
    assert!(after_json["download_token"]
        .as_str()
        .unwrap()
        .starts_with("dl_"));
}

#[tokio::test]
async fn refund_marks_order_refunded() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping commerce_routes_tests: DATABASE_URL unavailable");
        return;
    };
    let api_key = seed_user_and_api_key(&pool).await;
    seed_product(&pool, "paid-plugin").await;
    let app = routes::router(state(pool.clone()));

    let checkout = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/commerce/checkout")
                .header("content-type", "application/json")
                .header("x-apm-api-key", &api_key)
                .body(Body::from(
                    json!({"plugin_slug":"paid-plugin","idempotency_key":"intent-3"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let checkout_json = json_response(checkout).await;
    let order_id = checkout_json["order_id"].as_i64().unwrap();

    sqlx::query(
        "UPDATE orders SET status = 'fulfilled', fulfilled_at = NOW(), license_token = 'lic_test', download_token = 'dl_test' WHERE id = $1"
    )
    .bind(order_id)
    .execute(&pool)
    .await
    .unwrap();

    let refund = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/commerce/refunds")
                .header("content-type", "application/json")
                .header("x-apm-api-key", &api_key)
                .body(Body::from(json!({"order_id": order_id}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(refund.status(), StatusCode::OK);
    let refund_json = json_response(refund).await;
    assert_eq!(refund_json["status"], "refunded");
}

#[tokio::test]
async fn read_only_keys_cannot_checkout_query_orders_or_refund() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping commerce_routes_tests: DATABASE_URL unavailable");
        return;
    };
    let purchase_key = seed_user_and_api_key(&pool).await;
    seed_product(&pool, "paid-plugin").await;
    let app = routes::router(state(pool.clone()));

    let checkout = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/commerce/checkout")
                .header("content-type", "application/json")
                .header("x-apm-api-key", &purchase_key)
                .body(Body::from(
                    json!({"plugin_slug":"paid-plugin","idempotency_key":"intent-scope"})
                        .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(checkout.status(), StatusCode::OK);
    let checkout_json = json_response(checkout).await;
    let order_id = checkout_json["order_id"].as_i64().unwrap();

    let read_key = seed_user_and_api_key_with_name_and_scopes(&pool, "reader", "read").await;

    let denied_checkout = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/commerce/checkout")
                .header("content-type", "application/json")
                .header("x-apm-api-key", &read_key)
                .body(Body::from(
                    json!({"plugin_slug":"paid-plugin","idempotency_key":"intent-read"})
                        .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(denied_checkout.status(), StatusCode::FORBIDDEN);
    let denied_checkout_json = json_response(denied_checkout).await;
    assert_eq!(denied_checkout_json["code"], "INSUFFICIENT_SCOPE");

    let denied_order = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri(format!("/commerce/orders/{order_id}"))
                .header("x-apm-api-key", &read_key)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(denied_order.status(), StatusCode::FORBIDDEN);
    let denied_order_json = json_response(denied_order).await;
    assert_eq!(denied_order_json["code"], "INSUFFICIENT_SCOPE");

    let denied_refund = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/commerce/refunds")
                .header("content-type", "application/json")
                .header("x-apm-api-key", &read_key)
                .body(Body::from(json!({"order_id": order_id}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(denied_refund.status(), StatusCode::FORBIDDEN);
    let denied_refund_json = json_response(denied_refund).await;
    assert_eq!(denied_refund_json["code"], "INSUFFICIENT_SCOPE");
}
