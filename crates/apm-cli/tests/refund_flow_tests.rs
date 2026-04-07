mod support;

use std::fs;

use serde_json::Value;
use support::{command, spawn_mock_commerce_server, CliTestEnv};

fn parse_stdout(output: &std::process::Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap()
}

fn write_registry_fixture(env: &CliTestEnv) {
    let plugins_dir = env
        .cache_home
        .path()
        .join("apm/registries/official/plugins");
    fs::create_dir_all(&plugins_dir).unwrap();

    fs::write(
        plugins_dir.join("paid-plugin.toml"),
        r#"
slug = "paid-plugin"
name = "Paid Plugin"
vendor = "Apm"
version = "2.0.0"
description = "A paid plugin."
category = "effect"
license = "Commercial"
tags = ["paid", "commercial"]
is_paid = true
price_cents = 4999
currency = "usd"

[formats.au]
url = "https://example.com/paid-plugin-au.zip"
sha256 = "manual"
install_type = "zip"
"#,
    )
    .unwrap();

    fs::write(
        plugins_dir.join("free-plugin.toml"),
        r#"
slug = "free-plugin"
name = "Free Plugin"
vendor = "Apm"
version = "1.0.0"
description = "A free plugin."
category = "effect"
license = "Freeware"
tags = ["free"]

[formats.au]
url = "https://example.com/free-plugin-au.zip"
sha256 = "manual"
install_type = "zip"
"#,
    )
    .unwrap();
}

#[test]
fn paid_catalog_filters_and_info_show_price_metadata() {
    let env = CliTestEnv::new();
    write_registry_fixture(&env);

    let paid = command(&env)
        .args(["--json", "search", "--paid"])
        .output()
        .unwrap();
    assert!(paid.status.success());
    let paid_json = parse_stdout(&paid);
    assert_eq!(paid_json.as_array().unwrap().len(), 1);
    assert_eq!(paid_json[0]["slug"], "paid-plugin");
    assert_eq!(paid_json[0]["price_display"], "USD 49.99");

    let free = command(&env)
        .args(["--json", "search", "--free"])
        .output()
        .unwrap();
    assert!(free.status.success());
    let free_json = parse_stdout(&free);
    assert_eq!(free_json.as_array().unwrap().len(), 1);
    assert_eq!(free_json[0]["slug"], "free-plugin");
    assert_eq!(free_json[0]["price_display"], "free");

    let info = command(&env)
        .args(["--json", "info", "paid-plugin"])
        .output()
        .unwrap();
    assert!(info.status.success());
    let info_json = parse_stdout(&info);
    assert_eq!(info_json["is_paid"], true);
    assert_eq!(info_json["price_cents"], 4999);
    assert_eq!(info_json["currency"], "usd");
    assert_eq!(info_json["price_display"], "USD 49.99");
}

#[tokio::test(flavor = "multi_thread")]
async fn refund_by_plugin_slug_works_after_purchase_and_success_redirect_does_not_fulfill() {
    let env = CliTestEnv::new();
    let server = spawn_mock_commerce_server(99).await;

    let buy = command(&env)
        .env("APM_SERVER_URL", &server.base_url)
        .env("APM_API_KEY", "apm_live_test_secret")
        .env("APM_BUY_POLL_INTERVAL_MS", "1")
        .env("APM_BUY_MAX_POLLS", "0")
        .args(["--json", "buy", "paid-plugin"])
        .output()
        .unwrap();
    assert!(buy.status.success());
    let buy_json = parse_stdout(&buy);
    let order_id = buy_json["order_id"].as_i64().unwrap();
    assert_eq!(buy_json["status"], "pending");

    let success = reqwest::get(format!("{}/commerce/success", server.base_url))
        .await
        .unwrap();
    assert!(success.status().is_success());

    let pending = reqwest::Client::new()
        .get(format!("{}/commerce/orders/{order_id}", server.base_url))
        .header("x-apm-api-key", "apm_live_test_secret")
        .send()
        .await
        .unwrap()
        .json::<Value>()
        .await
        .unwrap();
    assert_eq!(pending["status"], "pending");

    let refund = command(&env)
        .env("APM_SERVER_URL", &server.base_url)
        .env("APM_API_KEY", "apm_live_test_secret")
        .args(["--json", "refund", "paid-plugin"])
        .output()
        .unwrap();
    assert!(refund.status.success());
    let refund_json = parse_stdout(&refund);
    assert_eq!(refund_json["order_id"], order_id);
    assert_eq!(refund_json["status"], "refunded");
    assert_eq!(refund_json["refunded"], true);
}
