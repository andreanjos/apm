mod support;

use serde_json::Value;

use support::{command, spawn_mock_auth_server, CliTestEnv};

fn parse_stdout(output: &std::process::Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn signup_login_logout_and_api_key_commands_work() {
    let env = CliTestEnv::new();
    let server = spawn_mock_auth_server().await;

    let signup = command(&env)
        .env("APM_SERVER_URL", &server.base_url)
        .args([
            "--json",
            "signup",
            "--email",
            "person@example.com",
            "--password",
            "password123",
        ])
        .output()
        .unwrap();
    assert!(signup.status.success());
    assert_eq!(parse_stdout(&signup)["authenticated"], true);

    let set_api_key = command(&env)
        .env("APM_SERVER_URL", &server.base_url)
        .args([
            "--json",
            "auth",
            "set-api-key",
            "agent",
            "apm_live_local_secret",
            "--scope",
            "account:read",
        ])
        .output()
        .unwrap();
    assert!(set_api_key.status.success());

    let status = command(&env)
        .env("APM_SERVER_URL", &server.base_url)
        .args(["--json", "auth", "status"])
        .output()
        .unwrap();
    assert!(status.status.success());
    assert_eq!(parse_stdout(&status)["email"], "agent@example.com");

    let logout = command(&env).args(["--json", "logout"]).output().unwrap();
    assert!(logout.status.success());
    assert_eq!(parse_stdout(&logout)["logged_out"], true);
    assert!(!env.credential_path("session").exists());
    assert!(!env.credential_path("api-key_index").exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn env_api_key_allows_non_interactive_auth_status() {
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
    assert_eq!(json["email"], "agent@example.com");
    assert_eq!(json["active_source"], "api_key");
}
