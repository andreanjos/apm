mod support;

use std::fs;

use support::{
    serve::{
        free_loopback_port, operation_history_path, response_body, spawn_server,
        wait_for_http_with_auth, wait_for_operation_state,
    },
    CliTestEnv,
};

#[test]
fn serve_run_reports_unknown_operation_cancel() {
    let env = CliTestEnv::new();
    let port = free_loopback_port();
    let _server = spawn_server(&env, port);

    let response =
        wait_for_http_with_auth(&env, "POST", port, "/v1/operations/not-real/cancel", "");
    assert!(
        response.starts_with("HTTP/1.1 404 Not Found"),
        "unknown operation cancel should return 404, got: {response}"
    );

    let body: serde_json::Value =
        serde_json::from_str(response_body(&response)).expect("error body should be JSON");
    assert_eq!(body["error"], "Unknown operation: not-real");
}

#[test]
fn serve_run_reports_unknown_operation_retry() {
    let env = CliTestEnv::new();
    let port = free_loopback_port();
    let _server = spawn_server(&env, port);

    let response = wait_for_http_with_auth(&env, "POST", port, "/v1/operations/not-real/retry", "");
    assert!(
        response.starts_with("HTTP/1.1 404 Not Found"),
        "unknown operation retry should return 404, got: {response}"
    );

    let body: serde_json::Value =
        serde_json::from_str(response_body(&response)).expect("error body should be JSON");
    assert_eq!(body["error"], "Unknown operation: not-real");
}

#[test]
fn serve_run_rejects_retry_without_saved_request_metadata() {
    let env = CliTestEnv::new();
    write_operation_history(
        &env,
        r#"{
          "schema_version": 1,
          "operations": [{
            "operation_id": "op-legacy",
            "kind": "registry_sync",
            "state": "failed",
            "created_at": "2026-06-30T00:00:00Z",
            "started_at": "2026-06-30T00:00:00Z",
            "finished_at": "2026-06-30T00:00:01Z",
            "result": null,
            "error": "old failure",
            "events": []
          }]
        }"#,
    );
    let port = free_loopback_port();
    let _server = spawn_server(&env, port);

    let response =
        wait_for_http_with_auth(&env, "POST", port, "/v1/operations/op-legacy/retry", "");
    assert!(
        response.starts_with("HTTP/1.1 409 Conflict"),
        "legacy operation retry should return 409, got: {response}"
    );

    let body: serde_json::Value =
        serde_json::from_str(response_body(&response)).expect("error body should be JSON");
    assert_eq!(
        body["error"],
        "Operation does not have saved request metadata."
    );
}

#[test]
fn serve_run_retries_failed_operation_from_saved_request_metadata() {
    let env = CliTestEnv::new();
    write_operation_history(
        &env,
        r#"{
          "schema_version": 2,
          "operations": [{
            "operation_id": "op-failed",
            "kind": "registry_sync",
            "request": { "kind": "registry_sync" },
            "state": "failed",
            "created_at": "2026-06-30T00:00:00Z",
            "started_at": "2026-06-30T00:00:00Z",
            "finished_at": "2026-06-30T00:00:01Z",
            "result": null,
            "error": "transient failure",
            "events": []
          }]
        }"#,
    );
    let port = free_loopback_port();
    let _server = spawn_server(&env, port);

    let response =
        wait_for_http_with_auth(&env, "POST", port, "/v1/operations/op-failed/retry", "");
    assert!(
        response.starts_with("HTTP/1.1 202 Accepted"),
        "retry should be accepted, got: {response}"
    );

    let retry: serde_json::Value =
        serde_json::from_str(response_body(&response)).expect("retry response JSON");
    assert_eq!(retry["original_operation_id"], "op-failed");
    assert_eq!(retry["message"], "Retry operation accepted.");
    let operation = &retry["operation"];
    assert_ne!(operation["operation_id"], "op-failed");
    assert_eq!(operation["kind"], "registry_sync");
    assert_eq!(
        operation["status_url"],
        format!(
            "/v1/operations/{}",
            operation["operation_id"]
                .as_str()
                .expect("new operation id")
        )
    );

    let status = wait_for_operation_state(
        &env,
        port,
        operation["status_url"].as_str().expect("status url"),
        "succeeded",
    );
    assert_eq!(status["kind"], "registry_sync");
    assert_eq!(status["request"]["kind"], "registry_sync");
}

#[test]
fn serve_run_reports_restart_interrupted_recovery_candidates() {
    let env = CliTestEnv::new();
    write_operation_history(
        &env,
        r#"{
          "schema_version": 2,
          "operations": [{
            "operation_id": "op-interrupted",
            "kind": "registry_sync",
            "request": { "kind": "registry_sync" },
            "state": "running",
            "created_at": "2026-06-30T00:00:00Z",
            "started_at": "2026-06-30T00:00:00Z",
            "finished_at": null,
            "result": null,
            "error": null,
            "events": []
          }]
        }"#,
    );
    let port = free_loopback_port();
    let _server = spawn_server(&env, port);

    let response = wait_for_http_with_auth(&env, "GET", port, "/v1/operations/recovery", "");
    assert!(
        response.starts_with("HTTP/1.1 200 OK"),
        "operation recovery should return 200, got: {response}"
    );

    let recovery: serde_json::Value =
        serde_json::from_str(response_body(&response)).expect("recovery response JSON");
    assert_eq!(recovery["interrupted_count"], 1);
    assert_eq!(recovery["retryable_count"], 1);
    assert_eq!(recovery["candidates"][0]["operation_id"], "op-interrupted");
    assert_eq!(recovery["candidates"][0]["retryable"], true);

    let retry_response = wait_for_http_with_auth(
        &env,
        "POST",
        port,
        "/v1/operations/op-interrupted/retry",
        "",
    );
    assert!(
        retry_response.starts_with("HTTP/1.1 202 Accepted"),
        "retry should be accepted, got: {retry_response}"
    );

    let retry: serde_json::Value =
        serde_json::from_str(response_body(&retry_response)).expect("retry response JSON");
    let operation = &retry["operation"];
    wait_for_operation_state(
        &env,
        port,
        operation["status_url"].as_str().expect("status url"),
        "succeeded",
    );

    let recovery_response =
        wait_for_http_with_auth(&env, "GET", port, "/v1/operations/recovery", "");
    assert!(
        recovery_response.starts_with("HTTP/1.1 200 OK"),
        "operation recovery should return 200, got: {recovery_response}"
    );

    let recovery_after_retry: serde_json::Value =
        serde_json::from_str(response_body(&recovery_response)).expect("recovery response JSON");
    assert_eq!(recovery_after_retry["interrupted_count"], 0);
    assert_eq!(recovery_after_retry["retryable_count"], 0);
    assert!(recovery_after_retry["candidates"]
        .as_array()
        .expect("recovery candidates")
        .is_empty());

    let retry_again_response = wait_for_http_with_auth(
        &env,
        "POST",
        port,
        "/v1/operations/op-interrupted/retry",
        "",
    );
    assert!(
        retry_again_response.starts_with("HTTP/1.1 409 Conflict"),
        "consumed recovery retry metadata should reject another retry, got: {retry_again_response}"
    );
    let retry_again: serde_json::Value =
        serde_json::from_str(response_body(&retry_again_response)).expect("retry error JSON");
    assert_eq!(
        retry_again["error"],
        "Operation does not have saved request metadata."
    );
}

#[test]
fn serve_run_retries_retryable_recovery_candidates() {
    let env = CliTestEnv::new();
    write_operation_history(
        &env,
        r#"{
          "schema_version": 2,
          "operations": [{
            "operation_id": "op-interrupted",
            "kind": "registry_sync",
            "request": { "kind": "registry_sync" },
            "state": "running",
            "created_at": "2026-06-30T00:00:00Z",
            "started_at": "2026-06-30T00:00:00Z",
            "finished_at": null,
            "result": null,
            "error": null,
            "events": []
          }, {
            "operation_id": "op-legacy",
            "kind": "registry_sync",
            "state": "running",
            "created_at": "2026-06-30T00:00:01Z",
            "started_at": "2026-06-30T00:00:01Z",
            "finished_at": null,
            "result": null,
            "error": null,
            "events": []
          }]
        }"#,
    );
    let port = free_loopback_port();
    let _server = spawn_server(&env, port);

    let response = wait_for_http_with_auth(&env, "POST", port, "/v1/operations/recovery/retry", "");
    assert!(
        response.starts_with("HTTP/1.1 202 Accepted"),
        "recovery retry should be accepted, got: {response}"
    );

    let retry: serde_json::Value =
        serde_json::from_str(response_body(&response)).expect("recovery retry response JSON");
    assert_eq!(retry["retried_count"], 1);
    assert_eq!(retry["message"], "Retry operation accepted.");
    assert_eq!(
        retry["operations"][0]["original_operation_id"],
        "op-interrupted"
    );

    let operation = &retry["operations"][0]["operation"];
    wait_for_operation_state(
        &env,
        port,
        operation["status_url"].as_str().expect("status url"),
        "succeeded",
    );

    let recovery_response =
        wait_for_http_with_auth(&env, "GET", port, "/v1/operations/recovery", "");
    assert!(
        recovery_response.starts_with("HTTP/1.1 200 OK"),
        "operation recovery should return 200, got: {recovery_response}"
    );

    let recovery: serde_json::Value =
        serde_json::from_str(response_body(&recovery_response)).expect("recovery response JSON");
    assert_eq!(recovery["interrupted_count"], 1);
    assert_eq!(recovery["retryable_count"], 0);
    assert_eq!(recovery["candidates"][0]["operation_id"], "op-legacy");
    assert_eq!(recovery["candidates"][0]["retryable"], false);

    let original_status_response =
        wait_for_http_with_auth(&env, "GET", port, "/v1/operations/op-interrupted", "");
    assert!(
        original_status_response.starts_with("HTTP/1.1 200 OK"),
        "original operation status should remain readable, got: {original_status_response}"
    );
    let original_status: serde_json::Value =
        serde_json::from_str(response_body(&original_status_response)).expect("status JSON");
    assert!(
        original_status["error"]
            .as_str()
            .expect("original status error")
            .starts_with("Retry submitted as op-"),
        "original operation should record submitted retry: {original_status_response}"
    );
    assert_eq!(original_status.get("request"), None);
}

fn write_operation_history(env: &CliTestEnv, content: &str) {
    let path = operation_history_path(env);
    fs::create_dir_all(path.parent().expect("history parent")).expect("history parent dir");
    fs::write(path, content).expect("write operation history");
}
