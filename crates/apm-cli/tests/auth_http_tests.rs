mod support;

use serde_json::Value;

use support::{command, read_to_string, spawn_mock_auth_server, CliTestEnv};

fn parse_stdout(output: &std::process::Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn login_flow_completes_against_mock_server() {
    let env = CliTestEnv::new();
    let server = spawn_mock_auth_server().await;

    let output = command(&env)
        .env("APM_SERVER_URL", &server.base_url)
        .args([
            "--json",
            "signup",
            "--email",
            "device@example.com",
            "--password",
            "password123",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());

    let json = parse_stdout(&output);
    assert_eq!(json["authenticated"], true);
    assert_eq!(json["email"], "device@example.com");
}

#[tokio::test(flavor = "multi_thread")]
async fn expired_session_refreshes_and_persists_rotated_tokens() {
    let env = CliTestEnv::new();
    let server = spawn_mock_auth_server().await;

    let signup = command(&env)
        .env("APM_SERVER_URL", &server.base_url)
        .args([
            "--json",
            "signup",
            "--email",
            "session@example.com",
            "--password",
            "password123",
        ])
        .output()
        .unwrap();
    assert!(signup.status.success());

    let session_path = env.credential_path("session");
    let mut session: Value = serde_json::from_str(&read_to_string(&session_path)).unwrap();
    session["expires_at"] =
        Value::String((chrono::Utc::now() - chrono::Duration::minutes(5)).to_rfc3339());
    std::fs::write(&session_path, serde_json::to_string(&session).unwrap()).unwrap();

    let output = command(&env)
        .env("APM_SERVER_URL", &server.base_url)
        .args(["--json", "auth", "status"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let json = parse_stdout(&output);
    assert_eq!(json["active_source"], "bearer");

    let session = read_to_string(&env.credential_path("session"));
    assert!(session.contains("refresh-"));
    assert_eq!(server.refresh_calls(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn env_api_key_status_does_not_trigger_refresh() {
    let env = CliTestEnv::new();
    let server = spawn_mock_auth_server().await;

    let output = command(&env)
        .env("APM_SERVER_URL", &server.base_url)
        .env("APM_API_KEY", "apm_live_env_secret")
        .args(["--json", "auth", "status"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let json = parse_stdout(&output);
    assert_eq!(json["active_source"], "api_key");
    assert_eq!(server.refresh_calls(), 0);
}
