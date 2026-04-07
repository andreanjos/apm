use apm_server::{
    auth::AuthConfig,
    license::LicenseConfig,
    routes::{self, AppState},
    stripe::StripeConfig,
};
use axum::{
    body::{to_bytes, Body},
    http::{Method, Request, StatusCode},
    Router,
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

async fn seed_catalog_product(
    pool: &PgPool,
    slug: &str,
    price_cents: i64,
    is_paid: bool,
    active: bool,
) {
    sqlx::query(
        r#"
        INSERT INTO catalog_products (slug, price_cents, currency, is_paid, active)
        VALUES ($1, $2, 'usd', $3, $4)
        "#,
    )
    .bind(slug)
    .bind(price_cents)
    .bind(is_paid)
    .bind(active)
    .execute(pool)
    .await
    .unwrap();
}

#[allow(clippy::too_many_arguments)]
async fn seed_storefront_plugin(
    pool: &PgPool,
    slug: &str,
    name: &str,
    vendor: &str,
    version: &str,
    category: &str,
    description: &str,
    tags: &[&str],
    formats: &[&str],
) {
    sqlx::query(
        r#"
        INSERT INTO storefront_plugins (
            slug, name, vendor, version, category, description, tags, formats
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7::jsonb, $8::jsonb)
        "#,
    )
    .bind(slug)
    .bind(name)
    .bind(vendor)
    .bind(version)
    .bind(category)
    .bind(description)
    .bind(serde_json::to_value(tags).unwrap())
    .bind(serde_json::to_value(formats).unwrap())
    .execute(pool)
    .await
    .unwrap();
}

async fn seed_section(
    pool: &PgPool,
    slug: &str,
    kind: &str,
    title: &str,
    description: Option<&str>,
    sort_order: i32,
) -> i64 {
    sqlx::query(
        r#"
        INSERT INTO storefront_sections (slug, kind, title, description, sort_order)
        VALUES ($1, $2, $3, $4, $5)
        RETURNING id
        "#,
    )
    .bind(slug)
    .bind(kind)
    .bind(title)
    .bind(description)
    .bind(sort_order)
    .fetch_one(pool)
    .await
    .unwrap()
    .get("id")
}

async fn seed_section_item(pool: &PgPool, section_id: i64, plugin_slug: &str, sort_order: i32) {
    sqlx::query(
        r#"
        INSERT INTO storefront_section_items (section_id, plugin_slug, sort_order)
        VALUES ($1, $2, $3)
        "#,
    )
    .bind(section_id)
    .bind(plugin_slug)
    .bind(sort_order)
    .execute(pool)
    .await
    .unwrap();
}

async fn seed_storefront(pool: &PgPool) {
    seed_catalog_product(pool, "staff-picked-pro", 4900, true, true).await;
    seed_catalog_product(pool, "fast-lint", 0, false, true).await;
    seed_catalog_product(pool, "bundle-archiver", 1900, true, true).await;
    seed_catalog_product(pool, "inactive-plugin", 9900, true, false).await;

    seed_storefront_plugin(
        pool,
        "staff-picked-pro",
        "Staff Picked Pro",
        "Acme Audio",
        "1.4.0",
        "mixing",
        "A polished mastering chain.",
        &["featured", "mix"],
        &["vst3", "au"],
    )
    .await;
    seed_storefront_plugin(
        pool,
        "fast-lint",
        "Fast Lint",
        "Build Tools Inc.",
        "2.1.0",
        "developer-tools",
        "A fast static analysis plugin.",
        &["new", "quality"],
        &["cli"],
    )
    .await;
    seed_storefront_plugin(
        pool,
        "bundle-archiver",
        "Bundle Archiver",
        "Release Ops",
        "0.9.1",
        "deployment",
        "Archives release bundles deterministically.",
        &["ops", "shipping"],
        &["cli", "binary"],
    )
    .await;
    seed_storefront_plugin(
        pool,
        "inactive-plugin",
        "Inactive Plugin",
        "Ghost Vendor",
        "9.9.9",
        "retired",
        "Should never appear in discovery payloads.",
        &["hidden"],
        &["cli"],
    )
    .await;

    let staff_picks = seed_section(
        pool,
        "staff-picks",
        "featured",
        "Staff Picks",
        Some("Curated by the server team."),
        10,
    )
    .await;
    let new_releases = seed_section(
        pool,
        "new-releases",
        "featured",
        "New Releases",
        Some("Fresh additions without a CLI deploy."),
        20,
    )
    .await;
    let developer_tools = seed_section(
        pool,
        "developer-tools",
        "explore",
        "Developer Tools",
        Some("Editorial categories owned by the server."),
        5,
    )
    .await;

    seed_section_item(pool, staff_picks, "staff-picked-pro", 1).await;
    seed_section_item(pool, new_releases, "fast-lint", 1).await;
    seed_section_item(pool, new_releases, "inactive-plugin", 2).await;
    seed_section_item(pool, developer_tools, "fast-lint", 1).await;
    seed_section_item(pool, developer_tools, "bundle-archiver", 2).await;
}

async fn json_response(response: axum::response::Response) -> Value {
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn featured_returns_curated_sections() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping discovery_routes_tests featured: DATABASE_URL unavailable");
        return;
    };
    seed_storefront(&pool).await;
    let app = routes::router(state(pool));

    let response = app
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/commerce/featured")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = json_response(response).await;
    let sections = body["sections"].as_array().unwrap();
    assert_eq!(sections.len(), 2);
    assert_eq!(sections[0]["slug"], "staff-picks");
    assert_eq!(sections[0]["plugins"][0]["slug"], "staff-picked-pro");
    assert_eq!(sections[0]["plugins"][0]["price_cents"], 4900);
    assert_eq!(sections[1]["slug"], "new-releases");
    assert_eq!(sections[1]["plugins"].as_array().unwrap().len(), 1);
    assert_eq!(sections[1]["plugins"][0]["slug"], "fast-lint");
}

#[tokio::test]
async fn explore_returns_editorial_categories() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping discovery_routes_tests explore: DATABASE_URL unavailable");
        return;
    };
    seed_storefront(&pool).await;

    sqlx::query("UPDATE storefront_sections SET title = $2 WHERE slug = $1")
        .bind("developer-tools")
        .bind("Build and Release")
        .execute(&pool)
        .await
        .unwrap();

    let app = routes::router(state(pool));
    let response = app
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/commerce/explore")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = json_response(response).await;
    let categories = body["categories"].as_array().unwrap();
    assert_eq!(categories.len(), 1);
    assert_eq!(categories[0]["slug"], "developer-tools");
    assert_eq!(categories[0]["title"], "Build and Release");
    assert_eq!(categories[0]["plugins"].as_array().unwrap().len(), 2);
    assert_eq!(categories[0]["plugins"][0]["is_paid"], false);
    assert_eq!(
        categories[0]["plugins"][1]["formats"],
        json!(["cli", "binary"])
    );
}

#[tokio::test]
async fn compare_returns_bounded_side_by_side_payload() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping discovery_routes_tests compare: DATABASE_URL unavailable");
        return;
    };
    seed_storefront(&pool).await;
    let app: Router = routes::router(state(pool.clone()));

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/commerce/compare")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "left_slug": "staff-picked-pro",
                        "right_slug": "bundle-archiver"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = json_response(response).await;
    assert_eq!(body["left"]["slug"], "staff-picked-pro");
    assert_eq!(body["left"]["vendor"], "Acme Audio");
    assert_eq!(body["left"]["formats"], json!(["vst3", "au"]));
    assert_eq!(body["right"]["slug"], "bundle-archiver");
    assert_eq!(body["right"]["price_cents"], 1900);
    assert_eq!(body["right"]["category"], "deployment");
    assert!(body.get("winner").is_none());
    assert!(body.get("score").is_none());

    let purchase_intents: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM purchase_intents")
        .fetch_one(&pool)
        .await
        .unwrap();
    let orders: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM orders")
        .fetch_one(&pool)
        .await
        .unwrap();
    let licenses: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM licenses")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(purchase_intents, 0);
    assert_eq!(orders, 0);
    assert_eq!(licenses, 0);

    let same_slug = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/commerce/compare")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "left_slug": "staff-picked-pro",
                        "right_slug": "staff-picked-pro"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(same_slug.status(), StatusCode::BAD_REQUEST);
    let same_slug_json = json_response(same_slug).await;
    assert_eq!(
        same_slug_json["error"]["code"],
        "COMPARE_REQUIRES_DISTINCT_PLUGINS"
    );

    let missing = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/commerce/compare")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "left_slug": "staff-picked-pro",
                        "right_slug": "missing-plugin"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
    let missing_json = json_response(missing).await;
    assert_eq!(missing_json["error"]["code"], "STOREFRONT_PLUGIN_NOT_FOUND");
}
