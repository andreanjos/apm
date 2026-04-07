use apm_server::{
    auth::AuthConfig,
    license::LicenseConfig,
    routes::{self, AppState},
    stripe::StripeConfig,
};
use axum::{
    body::{to_bytes, Body},
    http::{Method, Request, StatusCode},
};
use serde_json::{json, Value};
use sqlx::PgPool;
use tower::ServiceExt;

async fn test_pool() -> Option<PgPool> {
    let database_url = match std::env::var("DATABASE_URL") {
        Ok(value) if !value.trim().is_empty() => value,
        _ => return None,
    };

    let pool = PgPool::connect(&database_url).await.ok()?;
    sqlx::migrate!("../../migrations").run(&pool).await.ok()?;
    sqlx::query(
        "TRUNCATE agent_purchase_attempts, api_key_purchase_policies, api_keys, refresh_tokens, device_authorizations, users RESTART IDENTITY CASCADE",
    )
    .execute(&pool)
    .await
    .ok()?;
    Some(pool)
}

fn test_state(pool: PgPool) -> AppState {
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

async fn request(
    app: axum::Router,
    method: Method,
    uri: &str,
    body: Value,
) -> axum::response::Response {
    app.oneshot(
        Request::builder()
            .method(method)
            .uri(uri)
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap(),
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn signup_rejects_duplicate_email() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping auth_routes_tests: DATABASE_URL unavailable");
        return;
    };
    let app = routes::router(test_state(pool));

    let first = request(
        app.clone(),
        Method::POST,
        "/auth/signup",
        json!({"email":"duplicate@example.com","password":"password123"}),
    )
    .await;
    assert_eq!(first.status(), StatusCode::OK);

    let second = request(
        app,
        Method::POST,
        "/auth/signup",
        json!({"email":"duplicate@example.com","password":"password123"}),
    )
    .await;
    assert_eq!(second.status(), StatusCode::CONFLICT);
    let json = json_response(second).await;
    assert_eq!(json["code"], "EMAIL_EXISTS");
}

#[tokio::test]
async fn device_flow_start_returns_codes() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping auth_routes_tests: DATABASE_URL unavailable");
        return;
    };
    let app = routes::router(test_state(pool));

    let signup = request(
        app.clone(),
        Method::POST,
        "/auth/signup",
        json!({"email":"device@example.com","password":"password123"}),
    )
    .await;
    assert_eq!(signup.status(), StatusCode::OK);

    let response = request(
        app,
        Method::POST,
        "/auth/device/start",
        json!({"email":"device@example.com"}),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let json = json_response(response).await;
    assert!(json["device_code"].as_str().unwrap().len() > 10);
    assert!(json["user_code"].as_str().unwrap().len() >= 8);
    assert!(json["verification_uri"]
        .as_str()
        .unwrap()
        .contains("/auth/device/verify"));
}

#[tokio::test]
async fn refresh_rejects_invalid_token() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping auth_routes_tests: DATABASE_URL unavailable");
        return;
    };
    let app = routes::router(test_state(pool));

    let response = request(
        app,
        Method::POST,
        "/auth/token/refresh",
        json!({"refresh_token":"not-a-real-token"}),
    )
    .await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let json = json_response(response).await;
    assert_eq!(json["code"], "INVALID_REFRESH_TOKEN");
}

#[tokio::test]
async fn approved_device_flow_can_refresh_and_access_status() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping auth_routes_tests: DATABASE_URL unavailable");
        return;
    };
    let app = routes::router(test_state(pool));

    let signup = request(
        app.clone(),
        Method::POST,
        "/auth/signup",
        json!({"email":"approved@example.com","password":"password123"}),
    )
    .await;
    assert_eq!(signup.status(), StatusCode::OK);

    let start = request(
        app.clone(),
        Method::POST,
        "/auth/device/start",
        json!({"email":"approved@example.com"}),
    )
    .await;
    let start_json = json_response(start).await;

    let approve = request(
        app.clone(),
        Method::POST,
        "/auth/device/approve",
        json!({
            "email":"approved@example.com",
            "password":"password123",
            "user_code": start_json["user_code"].as_str().unwrap()
        }),
    )
    .await;
    assert_eq!(approve.status(), StatusCode::OK);

    let token = request(
        app.clone(),
        Method::POST,
        "/auth/token/poll",
        json!({"device_code": start_json["device_code"].as_str().unwrap()}),
    )
    .await;
    assert_eq!(token.status(), StatusCode::OK);
    let token_json = json_response(token).await;
    let access_token = token_json["access_token"].as_str().unwrap().to_string();
    let refresh_token = token_json["refresh_token"].as_str().unwrap().to_string();

    let status = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/auth/status")
                .header("authorization", format!("Bearer {access_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(status.status(), StatusCode::OK);
    let status_json = json_response(status).await;
    assert_eq!(status_json["source"], "bearer");

    let api_key_create = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/auth/api-keys")
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {access_token}"))
                .body(Body::from(
                    json!({"name":"agent","scopes":["account:read"]}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(api_key_create.status(), StatusCode::OK);
    let api_key_json = json_response(api_key_create).await;
    let api_key = api_key_json["api_key"].as_str().unwrap();

    let api_status = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/auth/status")
                .header("x-apm-api-key", api_key)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(api_status.status(), StatusCode::OK);
    let api_status_json = json_response(api_status).await;
    assert_eq!(api_status_json["source"], "api_key");

    let refresh = request(
        app,
        Method::POST,
        "/auth/token/refresh",
        json!({"refresh_token": refresh_token}),
    )
    .await;
    assert_eq!(refresh.status(), StatusCode::OK);
    let refresh_json = json_response(refresh).await;
    assert_ne!(refresh_json["refresh_token"], refresh_token);
}

#[tokio::test]
async fn api_key_management_requires_manage_scope_and_rejects_invalid_scope_names() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping auth_routes_tests: DATABASE_URL unavailable");
        return;
    };
    let app = routes::router(test_state(pool));

    let signup = request(
        app.clone(),
        Method::POST,
        "/auth/signup",
        json!({"email":"scopes@example.com","password":"password123"}),
    )
    .await;
    assert_eq!(signup.status(), StatusCode::OK);

    let start = request(
        app.clone(),
        Method::POST,
        "/auth/device/start",
        json!({"email":"scopes@example.com"}),
    )
    .await;
    assert_eq!(start.status(), StatusCode::OK);
    let start_json = json_response(start).await;

    let approve = request(
        app.clone(),
        Method::POST,
        "/auth/device/approve",
        json!({
            "email":"scopes@example.com",
            "password":"password123",
            "user_code": start_json["user_code"].as_str().unwrap()
        }),
    )
    .await;
    assert_eq!(approve.status(), StatusCode::OK);

    let token = request(
        app.clone(),
        Method::POST,
        "/auth/token/poll",
        json!({"device_code": start_json["device_code"].as_str().unwrap()}),
    )
    .await;
    assert_eq!(token.status(), StatusCode::OK);
    let token_json = json_response(token).await;
    let access_token = token_json["access_token"].as_str().unwrap().to_string();

    let read_key_create = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/auth/api-keys")
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {access_token}"))
                .body(Body::from(
                    json!({"name":"reader","scopes":["read"]}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(read_key_create.status(), StatusCode::OK);
    let read_key_json = json_response(read_key_create).await;
    let read_key = read_key_json["api_key"].as_str().unwrap().to_string();

    let denied_list = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/auth/api-keys")
                .header("x-apm-api-key", &read_key)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(denied_list.status(), StatusCode::FORBIDDEN);
    let denied_json = json_response(denied_list).await;
    assert_eq!(denied_json["code"], "INSUFFICIENT_SCOPE");

    let invalid_scope = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/auth/api-keys")
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {access_token}"))
                .body(Body::from(
                    json!({"name":"bad","scopes":["root"]}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(invalid_scope.status(), StatusCode::BAD_REQUEST);
    let invalid_scope_json = json_response(invalid_scope).await;
    assert_eq!(invalid_scope_json["code"], "INVALID_SCOPE");

    let manage_key_create = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/auth/api-keys")
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {access_token}"))
                .body(Body::from(
                    json!({"name":"manager","scopes":["manage"]}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(manage_key_create.status(), StatusCode::OK);
    let manage_key_json = json_response(manage_key_create).await;
    let manage_key = manage_key_json["api_key"].as_str().unwrap().to_string();

    let allowed_list = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/auth/api-keys")
                .header("x-apm-api-key", &manage_key)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(allowed_list.status(), StatusCode::OK);

    let revoke = app
        .oneshot(
            Request::builder()
                .method(Method::DELETE)
                .uri("/auth/api-keys/reader")
                .header("x-apm-api-key", &manage_key)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(revoke.status(), StatusCode::OK);
}

#[tokio::test]
async fn api_key_management_rejects_unknown_scopes() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping auth_routes_tests: DATABASE_URL unavailable");
        return;
    };
    let app = routes::router(test_state(pool));

    let signup = request(
        app.clone(),
        Method::POST,
        "/auth/signup",
        json!({"email":"scopes@example.com","password":"password123"}),
    )
    .await;
    assert_eq!(signup.status(), StatusCode::OK);

    let start = request(
        app.clone(),
        Method::POST,
        "/auth/device/start",
        json!({"email":"scopes@example.com"}),
    )
    .await;
    let start_json = json_response(start).await;

    let approve = request(
        app.clone(),
        Method::POST,
        "/auth/device/approve",
        json!({
            "email":"scopes@example.com",
            "password":"password123",
            "user_code": start_json["user_code"].as_str().unwrap()
        }),
    )
    .await;
    assert_eq!(approve.status(), StatusCode::OK);

    let token = request(
        app.clone(),
        Method::POST,
        "/auth/token/poll",
        json!({"device_code": start_json["device_code"].as_str().unwrap()}),
    )
    .await;
    let token_json = json_response(token).await;
    let access_token = token_json["access_token"].as_str().unwrap().to_string();

    let response = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/auth/api-keys")
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {access_token}"))
                .body(Body::from(
                    json!({"name":"bad-scope","scopes":["totally:unknown"]}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let json = json_response(response).await;
    assert_eq!(json["code"], "INVALID_SCOPE");
}

#[tokio::test]
async fn read_only_api_key_cannot_manage_api_keys() {
    let Some(pool) = test_pool().await else {
        eprintln!("skipping auth_routes_tests: DATABASE_URL unavailable");
        return;
    };
    let app = routes::router(test_state(pool));

    let signup = request(
        app.clone(),
        Method::POST,
        "/auth/signup",
        json!({"email":"readonly@example.com","password":"password123"}),
    )
    .await;
    assert_eq!(signup.status(), StatusCode::OK);

    let start = request(
        app.clone(),
        Method::POST,
        "/auth/device/start",
        json!({"email":"readonly@example.com"}),
    )
    .await;
    let start_json = json_response(start).await;

    let approve = request(
        app.clone(),
        Method::POST,
        "/auth/device/approve",
        json!({
            "email":"readonly@example.com",
            "password":"password123",
            "user_code": start_json["user_code"].as_str().unwrap()
        }),
    )
    .await;
    assert_eq!(approve.status(), StatusCode::OK);

    let token = request(
        app.clone(),
        Method::POST,
        "/auth/token/poll",
        json!({"device_code": start_json["device_code"].as_str().unwrap()}),
    )
    .await;
    let token_json = json_response(token).await;
    let access_token = token_json["access_token"].as_str().unwrap().to_string();

    let create = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/auth/api-keys")
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {access_token}"))
                .body(Body::from(
                    json!({"name":"reader","scopes":["read"]}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(create.status(), StatusCode::OK);
    let create_json = json_response(create).await;
    let api_key = create_json["api_key"].as_str().unwrap();

    let list = app
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/auth/api-keys")
                .header("x-apm-api-key", api_key)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(list.status(), StatusCode::FORBIDDEN);
    let json = json_response(list).await;
    assert_eq!(json["code"], "INSUFFICIENT_SCOPE");
}
