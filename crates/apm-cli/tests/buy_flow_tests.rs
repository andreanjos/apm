mod support;

use serde_json::Value;

use support::{command, read_to_string, spawn_mock_commerce_server, CliTestEnv};

fn parse_stdout(output: &std::process::Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn buy_json_creates_checkout_and_waits_for_fulfillment() {
    let env = CliTestEnv::new();
    let server = spawn_mock_commerce_server(1).await;

    let output = command(&env)
        .env("APM_SERVER_URL", &server.base_url)
        .env("APM_API_KEY", "apm_live_test_secret")
        .env("APM_BUY_POLL_INTERVAL_MS", "1")
        .env("APM_BUY_MAX_POLLS", "3")
        .args(["--json", "buy", "paid-plugin"])
        .output()
        .unwrap();
    assert!(output.status.success());

    let json = parse_stdout(&output);
    assert_eq!(json["plugin_slug"], "paid-plugin");
    assert_eq!(json["status"], "fulfilled");
    assert_eq!(json["fulfilled"], true);
    assert_eq!(json["install_ready"], true);
    assert_eq!(json["browser_opened"], false);
}

#[tokio::test(flavor = "multi_thread")]
async fn buy_retry_reuses_same_purchase_intent() {
    let env = CliTestEnv::new();
    let server = spawn_mock_commerce_server(99).await;

    let first = command(&env)
        .env("APM_SERVER_URL", &server.base_url)
        .env("APM_API_KEY", "apm_live_test_secret")
        .env("APM_BUY_POLL_INTERVAL_MS", "1")
        .env("APM_BUY_MAX_POLLS", "1")
        .args(["--json", "buy", "paid-plugin"])
        .output()
        .unwrap();
    assert!(first.status.success());
    let first_json = parse_stdout(&first);

    let second = command(&env)
        .env("APM_SERVER_URL", &server.base_url)
        .env("APM_API_KEY", "apm_live_test_secret")
        .env("APM_BUY_POLL_INTERVAL_MS", "1")
        .env("APM_BUY_MAX_POLLS", "1")
        .args(["--json", "buy", "paid-plugin"])
        .output()
        .unwrap();
    assert!(second.status.success());
    let second_json = parse_stdout(&second);

    assert_eq!(first_json["status"], "pending");
    assert_eq!(second_json["status"], "pending");
    assert_eq!(first_json["order_id"], second_json["order_id"]);
    assert_eq!(second_json["reused_intent"], true);
    assert_eq!(server.distinct_checkout_intents(), 1);

    let intent_path = env
        .data_home
        .path()
        .join("apm/commerce/intents/paid-plugin.json");
    let intent = read_to_string(&intent_path);
    assert!(intent.contains("buy-paid-plugin-"));
}

#[tokio::test(flavor = "multi_thread")]
async fn buy_timeout_reports_pending_without_install_ready() {
    let env = CliTestEnv::new();
    let server = spawn_mock_commerce_server(99).await;

    let output = command(&env)
        .env("APM_SERVER_URL", &server.base_url)
        .env("APM_API_KEY", "apm_live_test_secret")
        .env("APM_BUY_POLL_INTERVAL_MS", "1")
        .env("APM_BUY_MAX_POLLS", "1")
        .args(["--json", "buy", "pending-plugin"])
        .output()
        .unwrap();
    assert!(output.status.success());

    let json = parse_stdout(&output);
    assert_eq!(json["status"], "pending");
    assert_eq!(json["fulfilled"], false);
    assert_eq!(json["install_ready"], false);
    assert!(json["license_token"].is_null());
    assert!(json["download_token"].is_null());
}
