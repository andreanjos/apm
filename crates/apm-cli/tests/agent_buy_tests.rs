mod support;

use std::fs;

use serde_json::Value;

use support::{command, spawn_mock_commerce_server, CliTestEnv};

fn parse_stdout(output: &std::process::Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap()
}

fn write_free_plugin_fixture(env: &CliTestEnv) {
    let plugins_dir = env
        .cache_home
        .path()
        .join("apm/registries/official/plugins");
    fs::create_dir_all(&plugins_dir).unwrap();
    fs::write(
        plugins_dir.join("test-free.toml"),
        r#"
slug = "test-free"
name = "Test Free"
vendor = "Apm"
version = "1.0.0"
description = "A free plugin used for dry-run auth tests."
category = "effect"
license = "Freeware"
tags = ["free"]

[formats.au]
url = "https://example.com/test-free-au.zip"
sha256 = "manual"
install_type = "zip"
download_type = "direct"
"#,
    )
    .unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn success_confirm_json_returns_final_agent_purchase_payload() {
    let env = CliTestEnv::new();
    let server = spawn_mock_commerce_server(0).await;

    let output = command(&env)
        .env("APM_SERVER_URL", &server.base_url)
        .env("APM_API_KEY", "apm_live_manage_secret")
        .args(["--json", "buy", "paid-plugin", "--confirm"])
        .output()
        .unwrap();
    assert!(output.status.success());

    let json = parse_stdout(&output);
    assert_eq!(json["plugin_slug"], "paid-plugin");
    assert_eq!(json["status"], "fulfilled");
    assert_eq!(json["fulfilled"], true);
    assert_eq!(json["install_ready"], true);
    assert_eq!(json["cost_cents"], 4999);
    assert_eq!(json["currency"], "usd");
    assert!(json["transaction_id"]
        .as_str()
        .unwrap()
        .starts_with("agent_tx_"));
    assert!(json["license_token"].as_str().unwrap().starts_with("lic_"));
    assert!(json["download_token"].as_str().unwrap().starts_with("dl_"));
}

#[tokio::test(flavor = "multi_thread")]
async fn denial_confirm_json_preserves_machine_readable_detail() {
    let env = CliTestEnv::new();
    let server = spawn_mock_commerce_server(0).await;

    let output = command(&env)
        .env("APM_SERVER_URL", &server.base_url)
        .env("APM_API_KEY", "apm_live_deny_secret")
        .args(["--json", "buy", "paid-plugin", "--confirm"])
        .output()
        .unwrap();
    assert!(!output.status.success());

    let json = parse_stdout(&output);
    assert_eq!(json["code"], "PREAUTHORIZED_PAYMENT_REQUIRED");
    assert_eq!(json["details"]["price_cents"], 4999);

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.contains("Checkout created"));
    assert!(!stdout.contains("Opened the hosted checkout page"));
}

#[tokio::test(flavor = "multi_thread")]
async fn dry_run_install_does_not_require_manage_scope() {
    let env = CliTestEnv::new();
    write_free_plugin_fixture(&env);

    std::fs::write(
        env.credential_path("api-key:readonly"),
        serde_json::json!({
            "name": "readonly",
            "key": "apm_live_readonly_secret",
            "scopes": ["read", "purchase"],
            "created_at": chrono::Utc::now().to_rfc3339(),
        })
        .to_string(),
    )
    .unwrap();
    std::fs::write(
        env.credential_path("api-key-index"),
        serde_json::json!(["readonly"]).to_string(),
    )
    .unwrap();

    let output = command(&env)
        .args(["install", "test-free", "--dry-run"])
        .output()
        .unwrap();
    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("[dry-run] Would install Test Free"));
}
