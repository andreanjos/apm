mod support;

use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use apm_core::{
    config::Config,
    registry::PluginFormat,
    service::LOOPBACK_TOKEN_HEADER,
    state::{InstallOrigin, InstallState, InstalledFormat, InstalledPlugin},
};
use chrono::Utc;
use sha2::{Digest, Sha256};
use support::{
    command,
    serve::{
        free_loopback_port, http_request_with_auth, http_request_with_extra_headers,
        operation_token_path, response_body, serve_once, spawn_server, wait_for_http,
        wait_for_http_with_auth, wait_for_operation_state, wait_for_persisted_operation,
        TestServer,
    },
    write_fixture_config, CliTestEnv,
};
use zip::write::SimpleFileOptions;

#[test]
fn serve_run_help_exits_successfully() {
    let env = CliTestEnv::new();
    let output = command(&env)
        .args(["serve", "run", "--help"])
        .output()
        .expect("run apm serve run --help");

    assert!(
        output.status.success(),
        "apm serve run --help should exit 0; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn serve_run_exposes_health() {
    let env = CliTestEnv::new();
    let port = free_loopback_port();
    let _server = spawn_server(&env, port);

    let health_response = wait_for_http("GET", port, "/v1/health", "");
    assert!(
        health_response.starts_with("HTTP/1.1 200 OK"),
        "health should return 200, got: {health_response}"
    );

    let health: serde_json::Value =
        serde_json::from_str(response_body(&health_response)).expect("health JSON");
    assert_eq!(health["status"], "ok");
    assert_eq!(health["daemon_status"], "foreground_preview");
    assert_eq!(health["bind"]["host"], "127.0.0.1");
    assert_eq!(health["bind"]["port"], port);
    assert_eq!(health["auth"]["required"], true);
    assert_eq!(health["auth"]["header"], LOOPBACK_TOKEN_HEADER);
    assert_eq!(
        health["auth"]["token_file"],
        operation_token_path(&env).display().to_string()
    );
    assert!(
        operation_token_path(&env).exists(),
        "service should issue a loopback token file"
    );
}

#[test]
fn serve_run_rejects_missing_or_invalid_loopback_token() {
    let env = CliTestEnv::new();
    let port = free_loopback_port();
    let _server = spawn_server(&env, port);

    let public_response = wait_for_http("GET", port, "/v1/health", "");
    assert!(
        public_response.starts_with("HTTP/1.1 200 OK"),
        "health should remain public, got: {public_response}"
    );

    let missing_response = wait_for_http("GET", port, "/v1/models/store", "");
    assert!(
        missing_response.starts_with("HTTP/1.1 401 Unauthorized"),
        "protected route should reject missing token, got: {missing_response}"
    );

    let invalid_response = http_request_with_extra_headers(
        "GET",
        port,
        "/v1/models/store",
        "",
        &format!("{LOOPBACK_TOKEN_HEADER}: nope\r\n"),
    );
    assert!(
        invalid_response.starts_with("HTTP/1.1 401 Unauthorized"),
        "protected route should reject invalid token, got: {invalid_response}"
    );

    let authed_response = wait_for_http_with_auth(&env, "GET", port, "/v1/models/store", "");
    assert!(
        authed_response.starts_with("HTTP/1.1 200 OK"),
        "protected route should accept the issued token, got: {authed_response}"
    );
}

#[test]
fn serve_run_exposes_diagnostics_report() {
    let env = CliTestEnv::new();
    let port = free_loopback_port();
    let _server = spawn_server(&env, port);

    let diagnostics_response = wait_for_http_with_auth(&env, "GET", port, "/v1/diagnostics", "");
    assert!(
        diagnostics_response.starts_with("HTTP/1.1 200 OK"),
        "diagnostics should return 200, got: {diagnostics_response}"
    );

    let diagnostics: serde_json::Value =
        serde_json::from_str(response_body(&diagnostics_response)).expect("diagnostics JSON");
    assert!(
        diagnostics["checks"]
            .as_array()
            .expect("checks should be an array")
            .iter()
            .any(|check| check["name"] == "State file"),
        "diagnostics should include doctor checks, got: {diagnostics}"
    );
    assert!(
        diagnostics["summary"]["ok"].is_number()
            && diagnostics["summary"]["warnings"].is_number()
            && diagnostics["summary"]["failures"].is_number(),
        "diagnostics should include numeric summary counts, got: {diagnostics}"
    );
}

#[test]
fn serve_run_accepts_registry_sync_operation_and_reports_status() {
    let env = CliTestEnv::new();
    write_fixture_config(&env);
    let port = free_loopback_port();
    let _server = spawn_server(&env, port);

    let accepted_response = wait_for_http_with_auth(&env, "POST", port, "/v1/registry/sync", "");
    assert!(
        accepted_response.starts_with("HTTP/1.1 202 Accepted"),
        "registry sync should be accepted, got: {accepted_response}"
    );

    let accepted: serde_json::Value =
        serde_json::from_str(response_body(&accepted_response)).expect("accepted JSON");
    assert_eq!(accepted["kind"], "registry_sync");
    let status_url = accepted["status_url"]
        .as_str()
        .expect("accepted response should include status_url");

    let status = wait_for_operation_state(&env, port, status_url, "succeeded");
    assert_eq!(status["kind"], "registry_sync");
    assert_eq!(status["result"]["kind"], "registry_sync");
    assert!(
        status["events"]
            .as_array()
            .expect("events should be an array")
            .iter()
            .any(|event| event["event"] == "registry_sync_finished"),
        "registry sync status should include completion event: {status}"
    );

    let event_response =
        http_request_with_auth(&env, "GET", port, &format!("{status_url}/events"), "");
    assert!(
        event_response.starts_with("HTTP/1.1 200 OK"),
        "operation event stream should return 200, got: {event_response}"
    );
    assert!(
        event_response.contains("content-type: text/event-stream"),
        "operation event stream should use SSE content type, got: {event_response}"
    );
    let event_body = response_body(&event_response);
    assert!(
        event_body.contains("event: engine_event"),
        "operation event stream should name engine events, got: {event_body}"
    );
    assert!(
        event_body.contains("\"event\":\"registry_sync_finished\""),
        "operation event stream should replay completion event, got: {event_body}"
    );

    let history_response = http_request_with_auth(&env, "GET", port, "/v1/operations", "");
    assert!(
        history_response.starts_with("HTTP/1.1 200 OK"),
        "operation history should return 200, got: {history_response}"
    );
    let history: serde_json::Value =
        serde_json::from_str(response_body(&history_response)).expect("history JSON");
    let history = history.as_array().expect("history should be an array");
    assert!(
        history.iter().any(|operation| {
            operation["operation_id"] == accepted["operation_id"]
                && operation["state"] == "succeeded"
                && operation["kind"] == "registry_sync"
        }),
        "operation history should include completed sync: {history_response}"
    );
}

#[test]
fn serve_run_accepts_library_scan_operation_and_reconciles_external_plugins() {
    let env = CliTestEnv::new();
    write_fixture_config(&env);
    let scanned_bundle = temp_home_vst3_bundle(&env, "TestReverb.vst3");
    write_scanned_vst3_bundle(&scanned_bundle, "Test Reverb", "1.2.3");
    let port = free_loopback_port();
    let _server = spawn_server(&env, port);

    let accepted_response = wait_for_http_with_auth(&env, "POST", port, "/v1/library/scan", "");
    assert!(
        accepted_response.starts_with("HTTP/1.1 202 Accepted"),
        "library scan should be accepted, got: {accepted_response}"
    );

    let accepted: serde_json::Value =
        serde_json::from_str(response_body(&accepted_response)).expect("accepted JSON");
    assert_eq!(accepted["kind"], "library_scan");
    let status_url = accepted["status_url"]
        .as_str()
        .expect("accepted response should include status_url");

    let status = wait_for_operation_state(&env, port, status_url, "succeeded");
    assert_eq!(status["kind"], "library_scan");
    assert_eq!(status["result"]["kind"], "library_scan");
    assert!(
        status["result"]["result"]["adopted_count"]
            .as_u64()
            .expect("adopted_count should be numeric")
            >= 1,
        "library scan should adopt the fixture bundle: {status}"
    );
    assert!(
        status["result"]["result"]["plugins"]
            .as_array()
            .expect("scan result plugins should be an array")
            .iter()
            .any(|plugin| plugin["registry_slug"] == "test-reverb"),
        "library scan result should include the fixture bundle: {status}"
    );
    assert!(
        status["events"]
            .as_array()
            .expect("events should be an array")
            .iter()
            .any(|event| event["event"] == "scan_finished"),
        "library scan status should include completion event: {status}"
    );

    let library_response = wait_for_http_with_auth(&env, "GET", port, "/v1/library", "");
    assert!(
        library_response.starts_with("HTTP/1.1 200 OK"),
        "library should return 200 after scan, got: {library_response}"
    );
    let library: serde_json::Value =
        serde_json::from_str(response_body(&library_response)).expect("library JSON");
    assert!(library
        .as_array()
        .expect("library array")
        .iter()
        .any(|package| {
            package["slug"] == "test-reverb"
                && package["origin"] == "external"
                && package["formats"][0]["path"] == scanned_bundle.display().to_string()
        }));

    let event_response =
        http_request_with_auth(&env, "GET", port, &format!("{status_url}/events"), "");
    assert!(
        response_body(&event_response).contains("\"event\":\"scan_finished\""),
        "library scan event stream should replay completion event, got: {event_response}"
    );
}

#[test]
fn serve_run_persists_operation_status_across_restart() {
    let env = CliTestEnv::new();
    write_fixture_config(&env);
    let first_port = free_loopback_port();
    let first_server = spawn_server(&env, first_port);

    let accepted_response =
        wait_for_http_with_auth(&env, "POST", first_port, "/v1/registry/sync", "");
    assert!(
        accepted_response.starts_with("HTTP/1.1 202 Accepted"),
        "registry sync should be accepted, got: {accepted_response}"
    );
    let accepted: serde_json::Value =
        serde_json::from_str(response_body(&accepted_response)).expect("accepted JSON");
    let operation_id = accepted["operation_id"]
        .as_str()
        .expect("accepted response should include operation_id")
        .to_string();
    let status_url = accepted["status_url"]
        .as_str()
        .expect("accepted response should include status_url")
        .to_string();

    let status = wait_for_operation_state(&env, first_port, &status_url, "succeeded");
    assert_eq!(status["kind"], "registry_sync");
    wait_for_persisted_operation(&env, &operation_id);
    drop(first_server);

    let second_port = free_loopback_port();
    let _second_server = spawn_server(&env, second_port);
    let persisted_response = wait_for_http_with_auth(&env, "GET", second_port, &status_url, "");
    assert!(
        persisted_response.starts_with("HTTP/1.1 200 OK"),
        "persisted operation status should return 200, got: {persisted_response}"
    );
    let persisted: serde_json::Value =
        serde_json::from_str(response_body(&persisted_response)).expect("persisted status JSON");
    assert_eq!(persisted["operation_id"], operation_id);
    assert_eq!(persisted["state"], "succeeded");
    assert_eq!(persisted["result"]["kind"], "registry_sync");
    assert!(
        persisted["events"]
            .as_array()
            .expect("events should be an array")
            .iter()
            .any(|event| event["event"] == "registry_sync_finished"),
        "persisted status should include completion event: {persisted}"
    );

    let event_response = http_request_with_auth(
        &env,
        "GET",
        second_port,
        &format!("{status_url}/events"),
        "",
    );
    assert!(
        event_response.starts_with("HTTP/1.1 200 OK"),
        "persisted operation event stream should return 200, got: {event_response}"
    );
    assert!(
        response_body(&event_response).contains("\"event\":\"registry_sync_finished\""),
        "persisted operation event stream should replay events, got: {event_response}"
    );
}

#[test]
fn serve_run_accepts_archive_install_operation_and_reports_status() {
    let env = CliTestEnv::new();
    let archive = write_archive_install_config(&env);
    let port = free_loopback_port();
    let _server = spawn_server(&env, port);
    let body = serde_json::json!({
        "slug": "archive-effect",
        "format": "vst3",
        "archive_path": archive,
    })
    .to_string();

    let accepted_response =
        wait_for_http_with_auth(&env, "POST", port, "/v1/install/archive", &body);
    assert!(
        accepted_response.starts_with("HTTP/1.1 202 Accepted"),
        "archive install should be accepted, got: {accepted_response}"
    );

    let accepted: serde_json::Value =
        serde_json::from_str(response_body(&accepted_response)).expect("accepted JSON");
    let status_url = accepted["status_url"]
        .as_str()
        .expect("accepted response should include status_url");

    let status = wait_for_operation_state(&env, port, status_url, "succeeded");
    assert_eq!(status["kind"], "install_archive");
    assert_eq!(status["result"]["kind"], "install_package");
    assert_eq!(status["result"]["result"]["status"], "installed");
    assert_eq!(
        status["result"]["result"]["package"]["slug"],
        "archive-effect"
    );
    assert!(
        status["events"]
            .as_array()
            .expect("events should be an array")
            .iter()
            .any(|event| event["event"] == "install_finished"),
        "archive install status should include completion event: {status}"
    );

    let installed_bundle = env
        .data_home
        .path()
        .join("Library/Audio/Plug-Ins/VST3/ArchiveEffect.vst3");
    assert!(
        installed_bundle.join("Contents/Info.plist").exists(),
        "archive install should place the VST3 bundle under temp HOME"
    );
    let state = InstallState::load(&test_state_config(&env)).expect("state should reload");
    assert_eq!(
        state
            .find("archive-effect")
            .expect("installed package should be recorded")
            .formats[0]
            .path,
        installed_bundle
    );

    let event_response =
        http_request_with_auth(&env, "GET", port, &format!("{status_url}/events"), "");
    assert!(
        event_response.starts_with("HTTP/1.1 200 OK"),
        "archive install event stream should return 200, got: {event_response}"
    );
    assert!(
        response_body(&event_response).contains("\"event\":\"install_finished\""),
        "archive install event stream should replay completion event, got: {event_response}"
    );
}

#[test]
fn serve_run_accepts_url_install_operation_and_reports_status() {
    let env = CliTestEnv::new();
    let download_server = write_url_install_config_and_server(&env);
    let port = free_loopback_port();
    let _server = spawn_server(&env, port);

    let accepted_response = wait_for_http_with_auth(
        &env,
        "POST",
        port,
        "/v1/install/url",
        r#"{"slug":"url-effect","format":"vst3"}"#,
    );
    assert!(
        accepted_response.starts_with("HTTP/1.1 202 Accepted"),
        "URL install should be accepted, got: {accepted_response}"
    );

    let accepted: serde_json::Value =
        serde_json::from_str(response_body(&accepted_response)).expect("accepted JSON");
    let status_url = accepted["status_url"]
        .as_str()
        .expect("accepted response should include status_url");

    let status = wait_for_operation_state(&env, port, status_url, "succeeded");
    download_server.join();
    assert_eq!(status["kind"], "install_url");
    assert_eq!(status["result"]["kind"], "install_package");
    assert_eq!(status["result"]["result"]["status"], "installed");
    assert_eq!(status["result"]["result"]["package"]["slug"], "url-effect");
    assert!(
        status["events"]
            .as_array()
            .expect("events should be an array")
            .iter()
            .any(|event| event["event"] == "install_finished"),
        "URL install status should include completion event: {status}"
    );

    let installed_bundle = env
        .data_home
        .path()
        .join("Library/Audio/Plug-Ins/VST3/UrlEffect.vst3");
    assert!(
        installed_bundle.join("Contents/Info.plist").exists(),
        "URL install should place the downloaded VST3 bundle under temp HOME"
    );
}

#[test]
fn serve_run_accepts_package_update_operation_and_reports_status() {
    let env = CliTestEnv::new();
    let server = write_update_config_and_server(&env);
    let installed_bundle = temp_home_vst3_bundle(&env, "UpdateEffect.vst3");
    write_old_bundle(&installed_bundle);
    write_update_state(&env, "update-effect", installed_bundle.clone());
    let port = free_loopback_port();
    let _server = spawn_server(&env, port);

    let accepted_response = wait_for_http_with_auth(
        &env,
        "POST",
        port,
        "/v1/packages/update-effect/update",
        "{}",
    );
    assert!(
        accepted_response.starts_with("HTTP/1.1 202 Accepted"),
        "update operation should be accepted, got: {accepted_response}"
    );

    let accepted: serde_json::Value =
        serde_json::from_str(response_body(&accepted_response)).expect("accepted JSON");
    let status_url = accepted["status_url"]
        .as_str()
        .expect("accepted response should include status_url");

    let status = wait_for_operation_state(&env, port, status_url, "succeeded");
    server.join();
    assert_eq!(status["kind"], "package_update");
    assert_eq!(status["result"]["kind"], "update_package");
    assert_eq!(status["result"]["result"]["status"], "updated");
    assert_eq!(status["result"]["result"]["package"]["version"], "2.0.0");
    assert!(
        status["events"]
            .as_array()
            .expect("events should be an array")
            .iter()
            .any(|event| event["event"] == "install_finished"),
        "update status should include install completion event: {status}"
    );

    assert_eq!(
        fs::read_to_string(installed_bundle.join("Contents/Info.plist"))
            .expect("read updated bundle plist"),
        "test",
        "update should replace the old VST3 bundle with the downloaded archive"
    );
    let state = InstallState::load(&test_state_config(&env)).expect("state should reload");
    let updated = state
        .find("update-effect")
        .expect("updated package should be recorded");
    assert_eq!(updated.version, "2.0.0");
    assert_eq!(updated.formats[0].path, installed_bundle);

    let event_response =
        http_request_with_auth(&env, "GET", port, &format!("{status_url}/events"), "");
    assert!(
        event_response.starts_with("HTTP/1.1 200 OK"),
        "update event stream should return 200, got: {event_response}"
    );
    assert!(
        response_body(&event_response).contains("\"event\":\"install_finished\""),
        "update event stream should replay install completion event, got: {event_response}"
    );
}

#[test]
fn serve_run_resolves_manual_install_handoff_without_opening_target() {
    let env = CliTestEnv::new();
    write_manual_handoff_config(&env);
    let port = free_loopback_port();
    let _server = spawn_server(&env, port);

    let handoff_response = wait_for_http_with_auth(
        &env,
        "POST",
        port,
        "/v1/install/handoff",
        r#"{"slug":"manual-effect"}"#,
    );
    assert!(
        handoff_response.starts_with("HTTP/1.1 200 OK"),
        "handoff endpoint should return 200, got: {handoff_response}"
    );

    let handoff: serde_json::Value =
        serde_json::from_str(response_body(&handoff_response)).expect("handoff result JSON");
    assert_eq!(handoff["status"], "open");
    assert_eq!(handoff["plan"]["slug"], "manual-effect");
    assert_eq!(handoff["handoff"]["kind"], "manual_download");
    assert_eq!(handoff["handoff"]["target"]["kind"], "url");
    assert_eq!(
        handoff["handoff"]["target"]["url"],
        "https://example.com/manual-effect"
    );
}

#[test]
fn serve_run_sets_package_pin_state() {
    let env = CliTestEnv::new();
    write_installed_state(&env, "test-reverb", false);
    let port = free_loopback_port();
    let _server = spawn_server(&env, port);

    let pin_response = wait_for_http_with_auth(
        &env,
        "POST",
        port,
        "/v1/packages/test-reverb/pin",
        r#"{"pinned":true}"#,
    );
    assert!(
        pin_response.starts_with("HTTP/1.1 200 OK"),
        "pin endpoint should return 200, got: {pin_response}"
    );

    let pin_result: serde_json::Value =
        serde_json::from_str(response_body(&pin_response)).expect("pin result JSON");
    assert_eq!(pin_result["status"], "changed");
    assert_eq!(pin_result["package"]["slug"], "test-reverb");
    assert_eq!(pin_result["pinned"].as_bool(), Some(true));

    let state = InstallState::load(&test_state_config(&env)).expect("state should reload");
    assert!(
        state
            .find("test-reverb")
            .expect("installed package should remain in state")
            .pinned,
        "pin endpoint should persist the pinned flag"
    );
}

#[test]
fn serve_run_accepts_package_remove_operation_and_reports_status() {
    let env = CliTestEnv::new();
    let bundle = write_removable_state(&env, "test-reverb");
    let port = free_loopback_port();
    let _server = spawn_server(&env, port);

    let accepted_response = wait_for_http_with_auth(
        &env,
        "POST",
        port,
        "/v1/packages/test-reverb/remove",
        r#"{"dry_run":false}"#,
    );
    assert!(
        accepted_response.starts_with("HTTP/1.1 202 Accepted"),
        "remove operation should be accepted, got: {accepted_response}"
    );

    let accepted: serde_json::Value =
        serde_json::from_str(response_body(&accepted_response)).expect("accepted JSON");
    let status_url = accepted["status_url"]
        .as_str()
        .expect("accepted response should include status_url");

    let status = wait_for_operation_state(&env, port, status_url, "succeeded");
    assert_eq!(status["kind"], "package_remove");
    assert_eq!(status["result"]["kind"], "remove_package");
    assert_eq!(status["result"]["result"]["status"], "removed");
    assert!(
        status["events"]
            .as_array()
            .expect("events should be an array")
            .iter()
            .any(|event| event["event"] == "remove_finished"),
        "remove status should include completion event: {status}"
    );
    assert!(
        !bundle.exists(),
        "remove operation should delete the managed bundle"
    );
    assert!(
        InstallState::load(&test_state_config(&env))
            .expect("state should reload")
            .find("test-reverb")
            .is_none(),
        "remove operation should remove package from state"
    );

    let event_response =
        http_request_with_auth(&env, "GET", port, &format!("{status_url}/events"), "");
    assert!(
        event_response.starts_with("HTTP/1.1 200 OK"),
        "remove event stream should return 200, got: {event_response}"
    );
    assert!(
        response_body(&event_response).contains("\"event\":\"remove_finished\""),
        "remove event stream should replay completion event, got: {event_response}"
    );
}

fn write_removable_state(env: &CliTestEnv, slug: &str) -> PathBuf {
    let bundle = env.data_home.path().join(format!("{slug}.vst3"));
    fs::create_dir_all(bundle.join("Contents")).expect("create removable bundle");
    fs::write(bundle.join("Contents/Info.plist"), "test").expect("write removable bundle plist");

    let state = InstallState {
        version: 1,
        plugins: vec![InstalledPlugin {
            name: slug.to_string(),
            version: "1.0.0".to_string(),
            vendor: "Test Vendor".to_string(),
            formats: vec![InstalledFormat {
                format: PluginFormat::Vst3,
                path: bundle.clone(),
                sha256: String::new(),
            }],
            installed_at: Utc::now(),
            source: "official".to_string(),
            pinned: false,
            origin: InstallOrigin::Apm,
        }],
    };
    state
        .save(&test_state_config(env))
        .expect("write removable state");

    bundle
}

fn write_archive_install_config(env: &CliTestEnv) -> PathBuf {
    let archive = env.cache_home.path().join("archive-effect.zip");
    write_zip(&archive, &["ArchiveEffect.vst3/Contents/Info.plist"]);
    let sha256 = sha256_file(&archive);

    let registry_dir = env.config_home.path().join("archive-registry");
    let plugins_dir = registry_dir.join("plugins");
    fs::create_dir_all(&plugins_dir).expect("create archive registry plugins dir");
    fs::write(
        plugins_dir.join("archive-effect.toml"),
        format!(
            r#"
slug = "archive-effect"
name = "Archive Effect"
vendor = "Test Vendor"
version = "1.0.0"
description = "Archive install fixture"
category = "effects"
license = "freeware"

[formats.vst3]
url = "https://example.com/archive-effect.zip"
sha256 = "{sha256}"
install_type = "zip"
bundle_path = "ArchiveEffect.vst3"
"#
        ),
    )
    .expect("write archive install fixture");

    let config_dir = env.config_home.path().join("apm");
    fs::create_dir_all(&config_dir).expect("create test config dir");
    let config = format!(
        "default_registry_url = \"{}\"\n",
        registry_dir.display().to_string().replace('\\', "\\\\")
    );
    fs::write(config_dir.join("config.toml"), config).expect("write test config");
    archive
}

fn write_url_install_config_and_server(env: &CliTestEnv) -> TestServer {
    let archive = env.cache_home.path().join("url-effect.zip");
    write_zip(&archive, &["UrlEffect.vst3/Contents/Info.plist"]);
    let archive_bytes = fs::read(&archive).expect("read URL install archive");
    let sha256 = hex::encode(Sha256::digest(&archive_bytes));
    let server = serve_once(archive_bytes);

    let registry_dir = env.config_home.path().join("url-registry");
    let plugins_dir = registry_dir.join("plugins");
    fs::create_dir_all(&plugins_dir).expect("create URL registry plugins dir");
    fs::write(
        plugins_dir.join("url-effect.toml"),
        format!(
            r#"
slug = "url-effect"
name = "URL Effect"
vendor = "Test Vendor"
version = "1.0.0"
description = "URL install fixture"
category = "effects"
license = "freeware"

[formats.vst3]
url = "{}"
sha256 = "{sha256}"
install_type = "zip"
bundle_path = "UrlEffect.vst3"
"#,
            server.url
        ),
    )
    .expect("write URL install fixture");

    let config_dir = env.config_home.path().join("apm");
    fs::create_dir_all(&config_dir).expect("create test config dir");
    let config = format!(
        "default_registry_url = \"{}\"\n",
        registry_dir.display().to_string().replace('\\', "\\\\")
    );
    fs::write(config_dir.join("config.toml"), config).expect("write test config");
    server
}

fn write_update_config_and_server(env: &CliTestEnv) -> TestServer {
    let archive = env.cache_home.path().join("update-effect.zip");
    write_zip(&archive, &["UpdateEffect.vst3/Contents/Info.plist"]);
    let archive_bytes = fs::read(&archive).expect("read update archive");
    let sha256 = hex::encode(Sha256::digest(&archive_bytes));
    let server = serve_once(archive_bytes);

    let registry_dir = env.config_home.path().join("update-registry");
    let plugins_dir = registry_dir.join("plugins");
    fs::create_dir_all(&plugins_dir).expect("create update registry plugins dir");
    fs::write(
        plugins_dir.join("update-effect.toml"),
        format!(
            r#"
slug = "update-effect"
name = "Update Effect"
vendor = "Test Vendor"
version = "2.0.0"
description = "Update fixture"
category = "effects"
license = "freeware"

[formats.vst3]
url = "{}"
sha256 = "{sha256}"
install_type = "zip"
bundle_path = "UpdateEffect.vst3"
"#,
            server.url
        ),
    )
    .expect("write update fixture");

    let config_dir = env.config_home.path().join("apm");
    fs::create_dir_all(&config_dir).expect("create test config dir");
    let config = format!(
        "default_registry_url = \"{}\"\n",
        registry_dir.display().to_string().replace('\\', "\\\\")
    );
    fs::write(config_dir.join("config.toml"), config).expect("write test config");
    server
}

fn write_update_state(env: &CliTestEnv, slug: &str, bundle: PathBuf) {
    let state = InstallState {
        version: 1,
        plugins: vec![InstalledPlugin {
            name: slug.to_string(),
            version: "1.0.0".to_string(),
            vendor: "Test Vendor".to_string(),
            formats: vec![InstalledFormat {
                format: PluginFormat::Vst3,
                path: bundle,
                sha256: String::new(),
            }],
            installed_at: Utc::now(),
            source: "official".to_string(),
            pinned: false,
            origin: InstallOrigin::Apm,
        }],
    };
    state
        .save(&test_state_config(env))
        .expect("write update state");
}

fn temp_home_vst3_bundle(env: &CliTestEnv, bundle_name: &str) -> PathBuf {
    env.data_home
        .path()
        .join("Library/Audio/Plug-Ins/VST3")
        .join(bundle_name)
}

fn write_old_bundle(bundle: &Path) {
    fs::create_dir_all(bundle.join("Contents")).expect("create old bundle");
    fs::write(bundle.join("Contents/Info.plist"), "old").expect("write old bundle plist");
}

fn write_scanned_vst3_bundle(bundle: &Path, name: &str, version: &str) {
    fs::create_dir_all(bundle.join("Contents")).expect("create scanned bundle");
    fs::write(
        bundle.join("Contents/Info.plist"),
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
  <key>CFBundleName</key><string>{name}</string>
  <key>CFBundleShortVersionString</key><string>{version}</string>
  <key>CFBundleIdentifier</key><string>com.example.test-reverb</string>
</dict></plist>"#
        ),
    )
    .expect("write scanned bundle plist");
}

fn write_installed_state(env: &CliTestEnv, slug: &str, pinned: bool) {
    let state = InstallState {
        version: 1,
        plugins: vec![InstalledPlugin {
            name: slug.to_string(),
            version: "1.0.0".to_string(),
            vendor: "Test Vendor".to_string(),
            formats: vec![InstalledFormat {
                format: PluginFormat::Vst3,
                path: PathBuf::from(format!("/tmp/{slug}.vst3")),
                sha256: String::new(),
            }],
            installed_at: Utc::now(),
            source: "official".to_string(),
            pinned,
            origin: InstallOrigin::Apm,
        }],
    };
    state
        .save(&test_state_config(env))
        .expect("write installed state");
}

fn write_zip(path: &Path, entries: &[&str]) {
    let file = fs::File::create(path).expect("create zip");
    let mut writer = zip::ZipWriter::new(file);
    let options = SimpleFileOptions::default().unix_permissions(0o755);

    for entry in entries {
        writer.start_file(entry, options).expect("start zip file");
        writer.write_all(b"test").expect("write zip file");
    }

    writer.finish().expect("finish zip");
}

fn sha256_file(path: &Path) -> String {
    let bytes = fs::read(path).expect("read zip for sha256");
    hex::encode(Sha256::digest(bytes))
}

fn test_state_config(env: &CliTestEnv) -> Config {
    Config {
        data_dir: Some(env.data_home.path().join("apm")),
        cache_dir: Some(env.cache_home.path().join("apm")),
        ..Config::default()
    }
}

fn write_manual_handoff_config(env: &CliTestEnv) {
    let registry_dir = env.config_home.path().join("manual-registry");
    let plugins_dir = registry_dir.join("plugins");
    fs::create_dir_all(&plugins_dir).expect("create manual registry plugins dir");
    fs::write(
        plugins_dir.join("manual-effect.toml"),
        r#"
slug = "manual-effect"
name = "Manual Effect"
vendor = "Test Vendor"
version = "1.0.0"
description = "Manual install handoff fixture"
category = "effects"
license = "freeware"
homepage = "https://example.com/manual-effect"

[formats.vst3]
url = "manual"
sha256 = "manual"
install_type = "zip"
download_type = "manual"
"#,
    )
    .expect("write manual handoff fixture");

    let config_dir = env.config_home.path().join("apm");
    fs::create_dir_all(&config_dir).expect("create test config dir");
    let config = format!(
        "default_registry_url = \"{}\"\n",
        registry_dir.display().to_string().replace('\\', "\\\\")
    );
    fs::write(config_dir.join("config.toml"), config).expect("write test config");
}
