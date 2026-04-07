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
    sqlx::query(
        "TRUNCATE agent_purchase_attempts, api_key_purchase_policies, licenses, stripe_events, orders, purchase_intents, storefront_section_items, storefront_sections, storefront_plugins, catalog_products, api_keys, refresh_tokens, device_authorizations, users RESTART IDENTITY CASCADE",
    )
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

async fn json_response(response: axum::response::Response) -> Value {
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

async fn seed_user(pool: &PgPool, email: &str) -> i64 {
    sqlx::query("INSERT INTO users (email, password_hash) VALUES ($1, $2) RETURNING id")
        .bind(email)
        .bind("ignored")
        .fetch_one(pool)
        .await
        .unwrap()
        .get("id")
}

async fn seed_api_key(pool: &PgPool, user_id: i64, name: &str, scopes: &str) -> String {
    let api_key = format!("apm_live_{name}_secret");
    let api_key_id: i64 = sqlx::query(
        r#"
        INSERT INTO api_keys (user_id, name, key_prefix, key_hash, scopes)
        VALUES ($1, $2, $3, $4, $5)
        RETURNING id
        "#,
    )
    .bind(user_id)
    .bind(name)
    .bind(name)
    .bind(hash_secret(&api_key))
    .bind(scopes)
    .fetch_one(pool)
    .await
    .unwrap()
    .get("id");

    sqlx::query("INSERT INTO api_key_purchase_policies (api_key_id) VALUES ($1)")
        .bind(api_key_id)
        .execute(pool)
        .await
        .unwrap();

    api_key
}

async fn update_policy(
    pool: &PgPool,
    api_key_name: &str,
    preauthorized_payment_method: bool,
    per_transaction_limit_cents: Option<i64>,
    period_limit_cents: Option<i64>,
    period_spent_cents: i64,
) {
    sqlx::query(
        r#"
        UPDATE api_key_purchase_policies
        SET preauthorized_payment_method = $2,
            per_transaction_limit_cents = $3,
            period_limit_cents = $4,
            period_spent_cents = $5,
            period_started_at = NOW(),
            updated_at = NOW()
        WHERE api_key_id = (SELECT id FROM api_keys WHERE name = $1)
        "#,
    )
    .bind(api_key_name)
    .bind(preauthorized_payment_method)
    .bind(per_transaction_limit_cents)
    .bind(period_limit_cents)
    .bind(period_spent_cents)
    .execute(pool)
    .await
    .unwrap();
}

async fn seed_product(pool: &PgPool, slug: &str, price_cents: i64) {
    sqlx::query(
        "INSERT INTO catalog_products (slug, price_cents, currency, is_paid, active) VALUES ($1, $2, 'usd', TRUE, TRUE)"
    )
    .bind(slug)
    .bind(price_cents)
    .execute(pool)
    .await
    .unwrap();

    sqlx::query(
        r#"
        INSERT INTO storefront_plugins (slug, name, vendor, version, category, description, tags, formats)
        VALUES ($1, $2, $3, $4, $5, $6, '[]'::jsonb, '["cli"]'::jsonb)
        "#,
    )
    .bind(slug)
    .bind("Policy Test Plugin")
    .bind("Acme")
    .bind("1.0.0")
    .bind("testing")
    .bind("Used for policy route coverage.")
    .execute(pool)
    .await
    .unwrap();

    let section_id: i64 = sqlx::query(
        r#"
        INSERT INTO storefront_sections (slug, kind, title, description, sort_order)
        VALUES ('staff-picks', 'featured', 'Staff Picks', 'Test section', 1)
        ON CONFLICT (slug) DO UPDATE SET title = EXCLUDED.title
        RETURNING id
        "#,
    )
    .fetch_one(pool)
    .await
    .unwrap()
    .get("id");

    sqlx::query(
        r#"
        INSERT INTO storefront_section_items (section_id, plugin_slug, sort_order)
        VALUES ($1, $2, 1)
        ON CONFLICT (section_id, plugin_slug) DO NOTHING
        "#,
    )
    .bind(section_id)
    .bind(slug)
    .execute(pool)
    .await
    .unwrap();
}

#[tokio::test]
async fn scope_read_key_can_browse_but_cannot_agent_purchase() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping agent_policy_tests scope: DATABASE_URL unavailable");
        return;
    };
    let user_id = seed_user(&pool, "reader@example.com").await;
    let api_key = seed_api_key(&pool, user_id, "reader", "read").await;
    seed_product(&pool, "policy-plugin", 4900).await;
    let app = routes::router(state(pool));

    let browse = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/commerce/featured")
                .header("x-apm-api-key", &api_key)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(browse.status(), StatusCode::OK);

    let denied = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/commerce/agent/purchase")
                .header("content-type", "application/json")
                .header("x-apm-api-key", &api_key)
                .body(Body::from(
                    json!({"plugin_slug":"policy-plugin","idempotency_key":"scope-1"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(denied.status(), StatusCode::FORBIDDEN);
    let denied_json = json_response(denied).await;
    assert_eq!(denied_json["code"], "INSUFFICIENT_SCOPE");
}

#[tokio::test]
async fn limits_period_limit_denial_is_structured() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping agent_policy_tests limits: DATABASE_URL unavailable");
        return;
    };
    let user_id = seed_user(&pool, "limits@example.com").await;
    let api_key = seed_api_key(&pool, user_id, "limits", "purchase").await;
    seed_product(&pool, "policy-plugin", 4900).await;
    update_policy(&pool, "limits", true, Some(10_000), Some(5_000), 2_000).await;
    let app = routes::router(state(pool));

    let denied = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/commerce/agent/purchase")
                .header("content-type", "application/json")
                .header("x-apm-api-key", &api_key)
                .body(Body::from(
                    json!({"plugin_slug":"policy-plugin","idempotency_key":"limits-1"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(denied.status(), StatusCode::FORBIDDEN);
    let denied_json = json_response(denied).await;
    assert_eq!(denied_json["code"], "PERIOD_LIMIT_EXCEEDED");
    assert_eq!(denied_json["details"]["spent_cents"], 2000);
    assert_eq!(denied_json["details"]["price_cents"], 4900);
}

#[tokio::test]
async fn agent_purchase_returns_structured_denials_and_success() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping agent_policy_tests agent_purchase: DATABASE_URL unavailable");
        return;
    };
    let denied_user_id = seed_user(&pool, "denied@example.com").await;
    let denied_key = seed_api_key(&pool, denied_user_id, "denied", "purchase").await;
    let success_user_id = seed_user(&pool, "success@example.com").await;
    let success_key = seed_api_key(&pool, success_user_id, "success", "purchase").await;
    seed_product(&pool, "policy-plugin", 4900).await;
    update_policy(&pool, "denied", false, Some(10_000), Some(20_000), 0).await;
    update_policy(&pool, "success", true, Some(10_000), Some(20_000), 0).await;
    let app = routes::router(state(pool.clone()));

    let denied = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/commerce/agent/purchase")
                .header("content-type", "application/json")
                .header("x-apm-api-key", &denied_key)
                .body(Body::from(
                    json!({"plugin_slug":"policy-plugin","idempotency_key":"agent-denied"})
                        .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(denied.status(), StatusCode::PRECONDITION_REQUIRED);
    let denied_json = json_response(denied).await;
    assert_eq!(denied_json["code"], "PREAUTHORIZED_PAYMENT_REQUIRED");

    let success = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/commerce/agent/purchase")
                .header("content-type", "application/json")
                .header("x-apm-api-key", &success_key)
                .body(Body::from(
                    json!({"plugin_slug":"policy-plugin","idempotency_key":"agent-success"})
                        .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(success.status(), StatusCode::OK);
    let success_json = json_response(success).await;
    assert_eq!(success_json["status"], "fulfilled");
    assert_eq!(success_json["fulfilled"], true);
    assert_eq!(success_json["install_ready"], true);
    assert_eq!(success_json["cost_cents"], 4900);
    assert!(success_json["transaction_id"]
        .as_str()
        .unwrap()
        .starts_with("agent_tx_"));
    assert!(success_json["license_token"]
        .as_str()
        .unwrap()
        .starts_with("lic_"));
    assert!(success_json["download_token"]
        .as_str()
        .unwrap()
        .starts_with("dl_"));

    let order_id = success_json["order_id"].as_i64().unwrap();
    let order_status = app
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri(format!("/commerce/orders/{order_id}"))
                .header("x-apm-api-key", &success_key)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(order_status.status(), StatusCode::OK);
    let order_json = json_response(order_status).await;
    assert_eq!(order_json["status"], "fulfilled");

    let attempt_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM agent_purchase_attempts")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(attempt_count, 2);
}
