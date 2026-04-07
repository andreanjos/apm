mod support;

use serde_json::Value;

use support::{command, spawn_mock_commerce_server, CliTestEnv};

fn parse_stdout(output: &std::process::Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn featured_json_returns_server_managed_sections() {
    let env = CliTestEnv::new();
    let server = spawn_mock_commerce_server(0).await;

    let output = command(&env)
        .env("APM_SERVER_URL", &server.base_url)
        .args(["--json", "featured"])
        .output()
        .unwrap();
    assert!(output.status.success());

    let json = parse_stdout(&output);
    assert_eq!(json["sections"].as_array().unwrap().len(), 2);
    assert_eq!(json["sections"][0]["slug"], "staff-picks");
    assert_eq!(
        json["sections"][0]["plugins"][0]["slug"],
        "staff-picked-pro"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn featured_human_renders_curated_sections() {
    let env = CliTestEnv::new();
    let server = spawn_mock_commerce_server(0).await;

    let output = command(&env)
        .env("APM_SERVER_URL", &server.base_url)
        .args(["featured"])
        .output()
        .unwrap();
    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Featured"));
    assert!(stdout.contains("Staff Picks"));
    assert!(stdout.contains("staff-picked-pro"));
    assert!(stdout.contains("USD 49.00"));
}

#[tokio::test(flavor = "multi_thread")]
async fn explore_json_returns_editorial_categories() {
    let env = CliTestEnv::new();
    let server = spawn_mock_commerce_server(0).await;

    let output = command(&env)
        .env("APM_SERVER_URL", &server.base_url)
        .args(["--json", "explore"])
        .output()
        .unwrap();
    assert!(output.status.success());

    let json = parse_stdout(&output);
    assert_eq!(json["categories"].as_array().unwrap().len(), 1);
    assert_eq!(json["categories"][0]["title"], "Build and Release");
    assert_eq!(
        json["categories"][0]["plugins"][1]["slug"],
        "bundle-archiver"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn explore_human_renders_categories() {
    let env = CliTestEnv::new();
    let server = spawn_mock_commerce_server(0).await;

    let output = command(&env)
        .env("APM_SERVER_URL", &server.base_url)
        .args(["explore"])
        .output()
        .unwrap();
    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Explore"));
    assert!(stdout.contains("Build and Release"));
    assert!(stdout.contains("bundle-archiver"));
    assert!(stdout.contains("free"));
}

#[tokio::test(flavor = "multi_thread")]
async fn compare_json_returns_machine_parseable_payload() {
    let env = CliTestEnv::new();
    let server = spawn_mock_commerce_server(0).await;

    let output = command(&env)
        .env("APM_SERVER_URL", &server.base_url)
        .args(["--json", "compare", "staff-picked-pro", "bundle-archiver"])
        .output()
        .unwrap();
    assert!(output.status.success());

    let json = parse_stdout(&output);
    assert_eq!(json["left"]["slug"], "staff-picked-pro");
    assert_eq!(json["right"]["slug"], "bundle-archiver");
    assert_eq!(
        json["right"]["formats"],
        serde_json::json!(["cli", "binary"])
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn compare_human_renders_side_by_side_summary() {
    let env = CliTestEnv::new();
    let server = spawn_mock_commerce_server(0).await;

    let output = command(&env)
        .env("APM_SERVER_URL", &server.base_url)
        .args(["compare", "staff-picked-pro", "bundle-archiver"])
        .output()
        .unwrap();
    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Compare: staff-picked-pro vs bundle-archiver"));
    assert!(stdout.contains("Vendor:"));
    assert!(stdout.contains("Acme Audio"));
    assert!(stdout.contains("Release Ops"));
}

#[tokio::test(flavor = "multi_thread")]
async fn compare_missing_plugin_returns_clear_error() {
    let env = CliTestEnv::new();
    let server = spawn_mock_commerce_server(0).await;

    let output = command(&env)
        .env("APM_SERVER_URL", &server.base_url)
        .args(["compare", "staff-picked-pro", "missing-plugin"])
        .output()
        .unwrap();
    assert!(!output.status.success());

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("compare request failed"));
    assert!(stderr.contains("No storefront plugin exists for 'missing-plugin'."));
}
