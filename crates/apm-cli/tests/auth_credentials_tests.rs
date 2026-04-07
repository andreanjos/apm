mod support;

use serde_json::Value;

use support::{command, read_to_string, spawn_mock_auth_server, CliTestEnv};

fn parse_stdout(output: &std::process::Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn stored_api_keys_use_test_backend_not_xdg_paths() {
    let env = CliTestEnv::new();
    let output = command(&env)
        .args([
            "--json",
            "auth",
            "set-api-key",
            "local",
            "apm_live_local_secret",
            "--scope",
            "account:read",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());

    let stored = read_to_string(&env.credential_path("api-key_local"));
    assert!(stored.contains("apm_live_local_secret"));
    assert_no_secret_in_dir(env.config_home.path(), "apm_live_local_secret");
    assert_no_secret_in_dir(env.data_home.path(), "apm_live_local_secret");
}

#[tokio::test(flavor = "multi_thread")]
async fn env_api_key_takes_precedence_over_stored_credentials() {
    let env = CliTestEnv::new();
    let server = spawn_mock_auth_server().await;

    let set_output = command(&env)
        .env("APM_SERVER_URL", &server.base_url)
        .args(["auth", "set-api-key", "local", "apm_live_local_secret"])
        .output()
        .unwrap();
    assert!(set_output.status.success());

    env.write_session("access-stale", "refresh-stale", "session@example.com", 900);

    let output = command(&env)
        .env("APM_SERVER_URL", &server.base_url)
        .env("APM_API_KEY", "apm_live_env_secret")
        .args(["--json", "auth", "status"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let json = parse_stdout(&output);
    assert_eq!(json["active_source"], "api_key");
    assert_eq!(json["email"], "agent@example.com");
}

#[tokio::test(flavor = "multi_thread")]
async fn keychain_errors_are_explicit_without_plaintext_fallback() {
    let env = CliTestEnv::new();
    let output = command(&env)
        .env_remove("APM_TEST_CREDENTIAL_STORE_DIR")
        .env("APM_TEST_FORCE_KEYCHAIN_ERROR", "1")
        .args(["auth", "set-api-key", "broken", "apm_live_failure"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("macOS Keychain"));
    assert_no_secret_in_dir(env.config_home.path(), "apm_live_failure");
    assert_no_secret_in_dir(env.data_home.path(), "apm_live_failure");
}

fn assert_no_secret_in_dir(root: &std::path::Path, secret: &str) {
    if !root.exists() {
        return;
    }
    for entry in walkdir::WalkDir::new(root)
        .into_iter()
        .filter_map(Result::ok)
    {
        if entry.file_type().is_file() {
            let content = std::fs::read_to_string(entry.path()).unwrap_or_default();
            assert!(
                !content.contains(secret),
                "secret leaked into {}",
                entry.path().display()
            );
        }
    }
}
