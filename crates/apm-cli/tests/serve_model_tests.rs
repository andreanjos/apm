mod support;

use std::{
    fs,
    io::{Read, Write},
    net::TcpListener,
    sync::mpsc,
    thread,
    time::Duration,
};

use apm_core::model::{
    provision_runtime_adapter, ModelManifest, ModelStore, ModelWeightPullResult,
    ModelWeightPullStatus,
};
use sha2::{Digest, Sha256};
use support::{
    serve::{
        free_loopback_port, response_body, serve_once, spawn_server, wait_for_http_with_auth,
        wait_for_operation_state,
    },
    CliTestEnv,
};

const EXAMPLE_DEMUCS_SHA256: &str =
    "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const TEST_WEIGHT_BYTES: &[u8] = b"weights";
const TEST_WEIGHT_SHA256: &str = "9a129038d9a00aed0cf6a7ea059ca50a813449061ab87848cf1a13eafdf33b2c";

#[test]
fn serve_run_validates_model_manifest_content() {
    let env = CliTestEnv::new();
    let port = free_loopback_port();
    let _server = spawn_server(&env, port);
    let body = serde_json::json!({
        "manifest_toml": include_str!("../../../examples/models/demucs.toml")
    })
    .to_string();

    let response =
        wait_for_http_with_auth(&env, "POST", port, "/v1/models/manifest/validate", &body);
    assert!(
        response.starts_with("HTTP/1.1 200 OK"),
        "model manifest validation should return 200, got: {response}"
    );
    let result: serde_json::Value =
        serde_json::from_str(response_body(&response)).expect("manifest validation JSON");

    assert_eq!(result["package"]["package_id"], "demucs@4.0.1");
    assert_eq!(result["package"]["runtime_mode"], "native-mlx");
    assert_eq!(result["package"]["input"], "audio");
    assert_eq!(result["package"]["output"], "stems");
    assert_eq!(result["package"]["parameter_count"], 2);
    assert_eq!(result["package"]["min_memory_gb"], 8);

    let invalid_body = serde_json::json!({
        "manifest_toml": "[package]\nname = \"broken\""
    })
    .to_string();
    let invalid_response = wait_for_http_with_auth(
        &env,
        "POST",
        port,
        "/v1/models/manifest/validate",
        &invalid_body,
    );
    assert!(
        invalid_response.starts_with("HTTP/1.1 400 Bad Request"),
        "invalid model manifest should return 400, got: {invalid_response}"
    );
    let error: serde_json::Value =
        serde_json::from_str(response_body(&invalid_response)).expect("manifest error JSON");
    assert!(error["error"]
        .as_str()
        .expect("error should be a string")
        .contains("Failed to parse model manifest TOML"));
}

#[test]
fn serve_run_lists_cached_model_manifests() {
    let env = CliTestEnv::new();
    write_cached_model_manifest(&env);
    let port = free_loopback_port();
    let _server = spawn_server(&env, port);

    let response = wait_for_http_with_auth(&env, "GET", port, "/v1/models", "");
    assert!(
        response.starts_with("HTTP/1.1 200 OK"),
        "model listing should return 200, got: {response}"
    );
    let result: serde_json::Value =
        serde_json::from_str(response_body(&response)).expect("model listing JSON");

    let package = &result["packages"][0];
    assert_eq!(
        result["packages"].as_array().expect("packages array").len(),
        1
    );
    assert_eq!(package["package"]["package_id"], "demucs@4.0.1");
    assert_eq!(package["package"]["input"], "audio");
    assert_eq!(package["package"]["output"], "stems");
    assert_eq!(package["runtime_entry"], "demucs_mlx.Separator");
    assert_eq!(package["weights"]["cached"], true);
    assert_eq!(package["weights"]["format"], "safetensors");
    assert_eq!(package["params"][0]["name"], "stems");
    assert_eq!(package["params"][0]["type"], "enum");
    assert_eq!(package["params"][1]["name"], "shifts");
    assert_eq!(package["params"][1]["type"], "int");

    let search_response = wait_for_http_with_auth(&env, "GET", port, "/v1/models?query=stems", "");
    let search: serde_json::Value =
        serde_json::from_str(response_body(&search_response)).expect("model search JSON");
    assert_eq!(
        search["packages"][0]["package"]["package_id"],
        "demucs@4.0.1"
    );

    let empty_response = wait_for_http_with_auth(&env, "GET", port, "/v1/models?query=whisper", "");
    let empty: serde_json::Value =
        serde_json::from_str(response_body(&empty_response)).expect("empty model search JSON");
    assert_eq!(
        empty["packages"].as_array().expect("packages array").len(),
        0
    );
}

#[test]
fn serve_run_lists_registry_model_catalog() {
    let env = CliTestEnv::new();
    let registry = assert_fs::TempDir::new().expect("model registry");
    write_test_config(&env, registry.path());
    write_registry_model(
        registry.path(),
        "https://example.test/model.safetensors",
        TEST_WEIGHT_SHA256,
    );
    let port = free_loopback_port();
    let _server = spawn_server(&env, port);

    let response = wait_for_http_with_auth(&env, "GET", port, "/v1/models/catalog?query=stems", "");
    assert!(
        response.starts_with("HTTP/1.1 200 OK"),
        "model catalog listing should return 200, got: {response}"
    );
    let result: serde_json::Value =
        serde_json::from_str(response_body(&response)).expect("model catalog JSON");

    let package = &result["packages"][0];
    assert_eq!(
        result["packages"].as_array().expect("packages array").len(),
        1
    );
    assert_eq!(package["package"]["package_id"], "test-model@1.0.0");
    assert_eq!(package["source_name"], "official");
    assert_eq!(package["manifest_cached"], false);
    assert_eq!(package["runtime_entry"], "test.Model");
    assert_eq!(package["weights"]["cached"], false);
    assert_eq!(package["weights"]["format"], "safetensors");
}

#[test]
fn serve_run_initializes_model_store() {
    let env = CliTestEnv::new();
    let port = free_loopback_port();
    let _server = spawn_server(&env, port);
    let store_root = env.data_home.path().join(".apm");

    assert!(!store_root.exists(), "model store should start missing");

    let response = wait_for_http_with_auth(&env, "POST", port, "/v1/models/store/init", "");
    assert!(
        response.starts_with("HTTP/1.1 200 OK"),
        "model store init should return 200, got: {response}"
    );
    let result: serde_json::Value =
        serde_json::from_str(response_body(&response)).expect("model store init JSON");

    assert_eq!(result["layout"]["root"], store_root.display().to_string());
    for dir in ["manifests", "weights", "runtimes", "cache", "logs"] {
        assert!(
            store_root.join(dir).is_dir(),
            "model store init should create {dir}"
        );
    }
}

#[test]
fn serve_run_caches_registry_model_catalog_manifest() {
    let env = CliTestEnv::new();
    let registry = assert_fs::TempDir::new().expect("model registry");
    write_test_config(&env, registry.path());
    write_registry_model(
        registry.path(),
        "https://example.test/model.safetensors",
        TEST_WEIGHT_SHA256,
    );
    let port = free_loopback_port();
    let _server = spawn_server(&env, port);

    let response = wait_for_http_with_auth(
        &env,
        "POST",
        port,
        "/v1/models/catalog/test-model/1.0.0/cache",
        "",
    );
    assert!(
        response.starts_with("HTTP/1.1 200 OK"),
        "model catalog cache should return 200, got: {response}"
    );
    let result: serde_json::Value =
        serde_json::from_str(response_body(&response)).expect("model catalog cache JSON");

    assert_eq!(result["model"]["package"]["package_id"], "test-model@1.0.0");
    assert_eq!(result["replaced"], false);
    assert!(env
        .data_home
        .path()
        .join(".apm/manifests/test-model/1.0.0.toml")
        .exists());

    let list_response = wait_for_http_with_auth(&env, "GET", port, "/v1/models", "");
    let list: serde_json::Value =
        serde_json::from_str(response_body(&list_response)).expect("model listing JSON");
    assert_eq!(
        list["packages"][0]["package"]["package_id"],
        "test-model@1.0.0"
    );
}

#[test]
fn serve_run_caches_model_manifest_content() {
    let env = CliTestEnv::new();
    let port = free_loopback_port();
    let _server = spawn_server(&env, port);
    let body = serde_json::json!({
        "manifest_toml": include_str!("../../../examples/models/demucs.toml")
    })
    .to_string();

    let response = wait_for_http_with_auth(&env, "POST", port, "/v1/models/manifest/cache", &body);
    assert!(
        response.starts_with("HTTP/1.1 200 OK"),
        "model manifest cache should return 200, got: {response}"
    );
    let result: serde_json::Value =
        serde_json::from_str(response_body(&response)).expect("manifest cache JSON");

    assert_eq!(result["model"]["package"]["package_id"], "demucs@4.0.1");
    assert_eq!(result["model"]["runtime_entry"], "demucs_mlx.Separator");
    assert_eq!(result["model"]["weights"]["cached"], false);
    assert_eq!(result["replaced"], false);
    assert!(result["manifest_path"]
        .as_str()
        .expect("manifest path should be a string")
        .ends_with("/.apm/manifests/demucs/4.0.1.toml"));

    let list_response = wait_for_http_with_auth(&env, "GET", port, "/v1/models", "");
    assert!(
        list_response.starts_with("HTTP/1.1 200 OK"),
        "model listing should return 200 after cache, got: {list_response}"
    );
    let list: serde_json::Value =
        serde_json::from_str(response_body(&list_response)).expect("model listing JSON");
    assert_eq!(list["packages"][0]["package"]["package_id"], "demucs@4.0.1");

    let replace_response =
        wait_for_http_with_auth(&env, "POST", port, "/v1/models/manifest/cache", &body);
    let replace: serde_json::Value =
        serde_json::from_str(response_body(&replace_response)).expect("replace JSON");
    assert_eq!(replace["replaced"], true);
}

#[test]
fn serve_run_removes_cached_model_manifest_and_unreferenced_weights() {
    let env = CliTestEnv::new();
    write_cached_model_manifest(&env);
    fs::create_dir_all(
        env.data_home
            .path()
            .join(".apm/runtimes/native-mlx/demucs/4.0.1"),
    )
    .expect("create model runtime metadata");
    let port = free_loopback_port();
    let _server = spawn_server(&env, port);

    let response = wait_for_http_with_auth(&env, "DELETE", port, "/v1/models/demucs/4.0.1", "");

    assert!(
        response.starts_with("HTTP/1.1 200 OK"),
        "model remove should return 200, got: {response}"
    );
    let result: serde_json::Value =
        serde_json::from_str(response_body(&response)).expect("model remove JSON");
    assert_eq!(result["package_id"], "demucs@4.0.1");
    assert_eq!(result["status"], "removed");
    assert_eq!(result["removed_manifest"], true);
    assert_eq!(result["removed_runtime"], true);
    assert_eq!(result["removed_weight"], true);
    assert_eq!(result["weight_still_referenced"], false);
    assert!(!env
        .data_home
        .path()
        .join(".apm/manifests/demucs/4.0.1.toml")
        .exists());
    assert!(!env
        .data_home
        .path()
        .join(".apm/runtimes/native-mlx/demucs/4.0.1")
        .exists());
    assert!(!env
        .data_home
        .path()
        .join(format!(".apm/weights/{TEST_WEIGHT_SHA256}"))
        .exists());

    let list_response = wait_for_http_with_auth(&env, "GET", port, "/v1/models", "");
    let list: serde_json::Value =
        serde_json::from_str(response_body(&list_response)).expect("model listing JSON");
    assert_eq!(
        list["packages"].as_array().expect("packages array").len(),
        0
    );
}

#[test]
fn serve_run_reports_model_remove_not_cached() {
    let env = CliTestEnv::new();
    let port = free_loopback_port();
    let _server = spawn_server(&env, port);

    let response = wait_for_http_with_auth(&env, "DELETE", port, "/v1/models/demucs/4.0.1", "");

    assert!(
        response.starts_with("HTTP/1.1 200 OK"),
        "missing model remove should return 200, got: {response}"
    );
    let result: serde_json::Value =
        serde_json::from_str(response_body(&response)).expect("model remove JSON");
    assert_eq!(result["package_id"], "demucs@4.0.1");
    assert_eq!(result["status"], "not_cached");
    assert_eq!(result["removed_manifest"], false);
    assert_eq!(result["removed_runtime"], false);
    assert_eq!(result["removed_weight"], false);
}

#[test]
fn serve_run_plan_requires_model_install_metadata() {
    let env = CliTestEnv::new();
    write_cached_model_manifest(&env);
    let port = free_loopback_port();
    let _server = spawn_server(&env, port);
    let body = serde_json::json!({
        "input_path": "mix.wav",
        "output_path": "stems/"
    })
    .to_string();

    let response = wait_for_http_with_auth(
        &env,
        "POST",
        port,
        "/v1/models/demucs/4.0.1/run/plan",
        &body,
    );

    assert!(
        response.starts_with("HTTP/1.1 400 Bad Request"),
        "model run plan without install metadata should return 400, got: {response}"
    );
    let error: serde_json::Value =
        serde_json::from_str(response_body(&response)).expect("model run plan error JSON");
    assert!(error["error"]
        .as_str()
        .expect("error should be a string")
        .contains("runtime adapter metadata"));
}

#[test]
fn serve_run_rejects_invalid_model_manifest_cache_content() {
    let env = CliTestEnv::new();
    let port = free_loopback_port();
    let _server = spawn_server(&env, port);
    let invalid_manifest = include_str!("../../../examples/models/demucs.toml").replacen(
        "name = \"demucs\"",
        "name = \"../demucs\"",
        1,
    );
    let body = serde_json::json!({
        "manifest_toml": invalid_manifest
    })
    .to_string();

    let response = wait_for_http_with_auth(&env, "POST", port, "/v1/models/manifest/cache", &body);

    assert!(
        response.starts_with("HTTP/1.1 400 Bad Request"),
        "invalid model manifest cache should return 400, got: {response}"
    );
    let error: serde_json::Value =
        serde_json::from_str(response_body(&response)).expect("manifest cache error JSON");
    assert!(error["error"]
        .as_str()
        .expect("error should be a string")
        .contains("package.name"));
}

#[test]
fn serve_run_pulls_cached_model_weights_as_operation() {
    let env = CliTestEnv::new();
    let bytes = b"model weights".to_vec();
    let sha256 = sha256_hex(&bytes);
    let server = serve_once(bytes);
    write_cached_model_manifest_with_weights(&env, &server.url, &sha256);
    let port = free_loopback_port();
    let _server = spawn_server(&env, port);

    let response = wait_for_http_with_auth(
        &env,
        "POST",
        port,
        "/v1/models/test-model/1.0.0/weights/pull",
        "",
    );
    assert!(
        response.starts_with("HTTP/1.1 202 Accepted"),
        "model weight pull should be accepted, got: {response}"
    );
    let accepted: serde_json::Value =
        serde_json::from_str(response_body(&response)).expect("accepted operation JSON");
    assert_eq!(accepted["kind"], "model_weight_pull");

    let status = wait_for_operation_state(
        &env,
        port,
        accepted["status_url"].as_str().expect("status url"),
        "succeeded",
    );
    server.join();

    assert_eq!(status["kind"], "model_weight_pull");
    assert_eq!(status["result"]["kind"], "model_weight_pull");
    assert_eq!(status["result"]["result"]["status"], "pulled");
    assert_eq!(status["result"]["result"]["sha256"], sha256);
    assert_eq!(status["events"][0]["event"], "model_weight_pull_started");
    assert_eq!(status["events"][1]["event"], "model_weight_pull_progress");
    assert_eq!(status["events"][1]["bytes"], 13);
    assert_eq!(status["events"][1]["total_bytes"], 13);
    assert_eq!(status["events"][2]["event"], "model_weight_pull_finished");
    assert_eq!(status["events"][2]["status"], "pulled");
    assert!(env
        .data_home
        .path()
        .join(format!(".apm/weights/{sha256}"))
        .exists());
}

#[test]
fn serve_run_cancels_running_model_weight_pull_operation() {
    let env = CliTestEnv::new();
    let bytes = vec![b'w'; 2 * 1024 * 1024];
    let sha256 = sha256_hex(&bytes);
    let server = serve_slowly(bytes, Duration::from_millis(10));
    write_cached_model_manifest_with_weights(&env, &server.url, &sha256);
    let port = free_loopback_port();
    let _server = spawn_server(&env, port);

    let response = wait_for_http_with_auth(
        &env,
        "POST",
        port,
        "/v1/models/test-model/1.0.0/weights/pull",
        "",
    );
    assert!(
        response.starts_with("HTTP/1.1 202 Accepted"),
        "model weight pull should be accepted, got: {response}"
    );
    let accepted: serde_json::Value =
        serde_json::from_str(response_body(&response)).expect("accepted operation JSON");
    let status_url = accepted["status_url"]
        .as_str()
        .expect("accepted response should include status url")
        .to_string();

    server.wait_for_request();
    let cancel_response =
        wait_for_http_with_auth(&env, "POST", port, &format!("{status_url}/cancel"), "");
    assert!(
        cancel_response.starts_with("HTTP/1.1 200 OK"),
        "model weight pull cancellation should return 200, got: {cancel_response}"
    );
    let cancel: serde_json::Value =
        serde_json::from_str(response_body(&cancel_response)).expect("cancel response JSON");
    assert_eq!(cancel["accepted"], true);
    assert_eq!(cancel["state"], "cancel_requested");

    let status = wait_for_operation_state(&env, port, &status_url, "canceled");
    server.join();

    assert_eq!(status["kind"], "model_weight_pull");
    assert_eq!(status["state"], "canceled");
    assert_eq!(status["result"], serde_json::Value::Null);
    assert_eq!(status["error"], "Operation canceled by request.");
    assert_eq!(status["events"][0]["event"], "model_weight_pull_started");
    let failed = status["events"]
        .as_array()
        .expect("operation events")
        .iter()
        .find(|event| event["event"] == "model_weight_pull_failed")
        .expect("model weight pull failed event");
    assert_eq!(failed["error"], "Operation canceled by request.");
    let weight_path = env.data_home.path().join(format!(".apm/weights/{sha256}"));
    assert!(!weight_path.exists());
    assert!(!weight_path
        .with_file_name(format!("{sha256}.part"))
        .exists());
}

#[test]
fn serve_run_installs_cached_model_as_operation() {
    let env = CliTestEnv::new();
    write_cached_model_manifest(&env);
    let port = free_loopback_port();
    let _server = spawn_server(&env, port);

    let response =
        wait_for_http_with_auth(&env, "POST", port, "/v1/models/demucs/4.0.1/install", "");
    assert!(
        response.starts_with("HTTP/1.1 202 Accepted"),
        "model install should be accepted, got: {response}"
    );
    let accepted: serde_json::Value =
        serde_json::from_str(response_body(&response)).expect("accepted operation JSON");
    assert_eq!(accepted["kind"], "model_install");

    let status = wait_for_operation_state(
        &env,
        port,
        accepted["status_url"].as_str().expect("status url"),
        "succeeded",
    );

    assert_eq!(status["kind"], "model_install");
    assert_eq!(status["request"]["kind"], "model_install");
    assert_eq!(status["result"]["kind"], "model_install");
    assert_eq!(status["result"]["result"]["package_id"], "demucs@4.0.1");
    assert_eq!(status["result"]["result"]["runtime_mode"], "native-mlx");
    assert_eq!(
        status["result"]["result"]["runtime"]["adapter"],
        "native-mlx"
    );
    assert_eq!(status["result"]["result"]["runtime"]["status"], "prepared");
    assert_eq!(status["result"]["result"]["weights"]["status"], "cached");
    assert_eq!(status["events"][0]["event"], "model_install_started");
    assert_eq!(status["events"][1]["event"], "model_install_finished");
    assert_eq!(status["events"][1]["adapter"], "native-mlx");
    assert_eq!(status["events"][1]["runtime_mode"], "native-mlx");
    assert_eq!(status["events"][1]["runtime_status"], "prepared");
    assert_eq!(status["events"][1]["weights_status"], "cached");
    assert!(env
        .data_home
        .path()
        .join(".apm/runtimes/native-mlx/demucs/4.0.1/adapter.toml")
        .exists());

    let run_body = serde_json::json!({
        "input_path": "mix.wav",
        "output_path": "stems/",
        "params": {
            "stems": "2",
            "shifts": "3"
        }
    })
    .to_string();
    let run_response = wait_for_http_with_auth(
        &env,
        "POST",
        port,
        "/v1/models/demucs/4.0.1/run/plan",
        &run_body,
    );
    assert!(
        run_response.starts_with("HTTP/1.1 200 OK"),
        "model run plan should return 200, got: {run_response}"
    );
    let run_plan: serde_json::Value =
        serde_json::from_str(response_body(&run_response)).expect("model run plan JSON");
    assert_eq!(run_plan["package_id"], "demucs@4.0.1");
    assert_eq!(run_plan["status"], "planned");
    assert_eq!(run_plan["runtime_mode"], "native-mlx");
    assert_eq!(run_plan["adapter"], "native-mlx");
    assert_eq!(run_plan["input_path"], "mix.wav");
    assert_eq!(run_plan["output_path"], "stems/");
    assert_eq!(run_plan["execution"]["status"], "blocked");
    assert_eq!(
        run_plan["execution"]["blocker"],
        "adapter_runner_unavailable"
    );
    assert!(run_plan["execution"]["message"]
        .as_str()
        .expect("execution message")
        .contains("native-mlx execution"));
    assert_eq!(run_plan["params"][0]["name"], "stems");
    assert_eq!(run_plan["params"][0]["value"], "2");
    assert_eq!(run_plan["params"][0]["source"], "request");
    assert_eq!(run_plan["params"][1]["name"], "shifts");
    assert_eq!(run_plan["params"][1]["value"], 3);
    assert_eq!(run_plan["params"][1]["source"], "request");
    assert!(run_plan["adapter_manifest_path"]
        .as_str()
        .expect("adapter manifest path")
        .ends_with(".apm/runtimes/native-mlx/demucs/4.0.1/adapter.toml"));

    let run_operation_response =
        wait_for_http_with_auth(&env, "POST", port, "/v1/models/demucs/4.0.1/run", &run_body);
    assert!(
        run_operation_response.starts_with("HTTP/1.1 202 Accepted"),
        "model run operation should be accepted, got: {run_operation_response}"
    );
    let accepted_run: serde_json::Value =
        serde_json::from_str(response_body(&run_operation_response))
            .expect("accepted run operation JSON");
    assert_eq!(accepted_run["kind"], "model_run");

    let run_status = wait_for_operation_state(
        &env,
        port,
        accepted_run["status_url"]
            .as_str()
            .expect("model run status url"),
        "failed",
    );
    assert_eq!(run_status["kind"], "model_run");
    assert_eq!(run_status["request"]["kind"], "model_run");
    assert_eq!(run_status["request"]["name"], "demucs");
    assert_eq!(run_status["request"]["version"], "4.0.1");
    assert_eq!(run_status["request"]["request"]["input_path"], "mix.wav");
    assert_eq!(run_status["request"]["request"]["output_path"], "stems/");
    assert_eq!(run_status["request"]["request"]["params"]["stems"], "2");
    assert_eq!(run_status["request"]["request"]["params"]["shifts"], "3");
    assert_eq!(run_status["result"]["kind"], "model_run");
    assert_eq!(run_status["result"]["result"]["status"], "blocked");
    assert_eq!(run_status["result"]["result"]["package_id"], "demucs@4.0.1");
    assert_eq!(
        run_status["result"]["result"]["plan"]["execution"]["blocker"],
        "adapter_runner_unavailable"
    );
    assert_eq!(
        run_status["result"]["result"]["plan"]["params"][0]["value"],
        "2"
    );
    assert_eq!(
        run_status["result"]["result"]["plan"]["params"][1]["value"],
        3
    );
    assert!(run_status["result"]["result"]["message"]
        .as_str()
        .expect("model run result message")
        .contains("native-mlx execution"));
    assert!(run_status["error"]
        .as_str()
        .expect("model run error")
        .contains("native-mlx execution"));
    assert_eq!(run_status["events"][0]["event"], "model_run_started");
    assert_eq!(run_status["events"][1]["event"], "model_run_blocked");
    assert_eq!(
        run_status["events"][1]["blocker"],
        "adapter_runner_unavailable"
    );
    assert!(run_status["events"][1]["message"]
        .as_str()
        .expect("model run blocked message")
        .contains("native-mlx execution"));
}

#[test]
fn serve_run_plans_prepared_model_chain() {
    let env = CliTestEnv::new();
    write_prepared_chain_model(
        &env,
        "demucs",
        "4.0.1",
        "audio",
        "stems",
        TEST_WEIGHT_SHA256,
    );
    write_prepared_chain_model(
        &env,
        "whisper",
        "3.0.0",
        "audio",
        "text",
        "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210",
    );
    let port = free_loopback_port();
    let _server = spawn_server(&env, port);
    let body = serde_json::json!({
        "input_path": "mix.wav",
        "output_path": "lyrics.txt",
        "steps": [
            { "name": "demucs", "version": "4.0.1" },
            { "name": "whisper", "version": "3.0.0" }
        ]
    })
    .to_string();

    let response = wait_for_http_with_auth(&env, "POST", port, "/v1/models/chains/plan", &body);

    assert!(
        response.starts_with("HTTP/1.1 200 OK"),
        "model chain plan should return 200, got: {response}"
    );
    let plan: serde_json::Value =
        serde_json::from_str(response_body(&response)).expect("model chain plan JSON");
    assert_eq!(plan["status"], "planned");
    assert_eq!(plan["input"], "audio");
    assert_eq!(plan["output"], "text");
    assert_eq!(plan["steps"][0]["package_id"], "demucs@4.0.1");
    assert_eq!(plan["steps"][1]["package_id"], "whisper@3.0.0");
    assert_eq!(plan["edges"][0]["from_output"], "stems");
    assert_eq!(plan["edges"][0]["to_input"], "audio");
    assert_eq!(plan["edges"][0]["binding"], "stem_selection_required");
    assert_eq!(plan["execution"]["status"], "blocked");
    assert_eq!(plan["execution"]["blocker"], "chain_runner_unavailable");
}

fn write_cached_model_manifest(env: &CliTestEnv) {
    let store = env.data_home.path().join(".apm");
    let manifest_path = store.join("manifests/demucs/4.0.1.toml");
    let manifest = include_str!("../../../examples/models/demucs.toml")
        .replace(EXAMPLE_DEMUCS_SHA256, TEST_WEIGHT_SHA256);
    fs::create_dir_all(manifest_path.parent().expect("manifest parent"))
        .expect("create manifest cache");
    fs::create_dir_all(store.join("weights")).expect("create weights cache");
    fs::write(manifest_path, manifest).expect("write cached model manifest");
    fs::write(
        store.join(format!("weights/{TEST_WEIGHT_SHA256}")),
        TEST_WEIGHT_BYTES,
    )
    .expect("write cached model weight");
}

fn write_prepared_chain_model(
    env: &CliTestEnv,
    name: &str,
    version: &str,
    input: &str,
    output: &str,
    sha256: &str,
) {
    let store = ModelStore::new(env.data_home.path().join(".apm"));
    let manifest_toml = test_model_manifest_for_io(name, version, input, output, sha256);
    let manifest = ModelManifest::from_toml_str(&manifest_toml).expect("model manifest");
    store
        .cache_manifest(&manifest, &manifest_toml)
        .expect("cache model manifest");
    fs::create_dir_all(store.weights_dir()).expect("create weights cache");
    let weights_path = store.weight_path(sha256);
    fs::write(&weights_path, TEST_WEIGHT_BYTES).expect("write cached model weight");
    provision_runtime_adapter(
        &store,
        &manifest,
        &ModelWeightPullResult {
            package_id: manifest.package_id(),
            source: manifest.weights.source.clone(),
            resolved_url: manifest.weights.source.clone(),
            sha256: sha256.to_string(),
            path: weights_path.display().to_string(),
            bytes: TEST_WEIGHT_BYTES.len() as u64,
            status: ModelWeightPullStatus::Cached,
        },
    )
    .expect("provision runtime adapter metadata");
}

fn write_test_config(env: &CliTestEnv, registry_path: &std::path::Path) {
    let config_dir = env.config_home.path().join("apm");
    fs::create_dir_all(&config_dir).expect("create config dir");
    fs::write(
        config_dir.join("config.toml"),
        format!("default_registry_url = \"{}\"\n", registry_path.display()),
    )
    .expect("write config");
}

fn write_registry_model(registry_path: &std::path::Path, source: &str, sha256: &str) {
    let manifest_path = registry_path.join("models/test-model/1.0.0.toml");
    fs::create_dir_all(manifest_path.parent().expect("manifest parent"))
        .expect("create registry model dir");
    fs::write(manifest_path, test_model_manifest(source, sha256))
        .expect("write registry model manifest");
}

fn write_cached_model_manifest_with_weights(env: &CliTestEnv, source: &str, sha256: &str) {
    let store = env.data_home.path().join(".apm");
    let manifest_path = store.join("manifests/test-model/1.0.0.toml");
    fs::create_dir_all(manifest_path.parent().expect("manifest parent"))
        .expect("create manifest cache");
    fs::write(manifest_path, test_model_manifest(source, sha256))
        .expect("write cached model manifest");
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

struct SlowTestServer {
    url: String,
    accepted: mpsc::Receiver<()>,
    handle: thread::JoinHandle<()>,
}

impl SlowTestServer {
    fn wait_for_request(&self) {
        self.accepted
            .recv_timeout(Duration::from_secs(5))
            .expect("slow test server should receive request");
    }

    fn join(self) {
        self.handle.join().expect("server thread should finish");
    }
}

fn serve_slowly(body: Vec<u8>, delay: Duration) -> SlowTestServer {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind test server");
    let url = format!(
        "http://{}/model.safetensors",
        listener.local_addr().expect("local addr")
    );
    let (accepted_tx, accepted) = mpsc::channel();
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept request");
        let _ = accepted_tx.send(());
        let mut buffer = [0_u8; 1024];
        let _ = stream.read(&mut buffer);
        let header = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        if stream.write_all(header.as_bytes()).is_err() {
            return;
        }
        for chunk in body.chunks(64 * 1024) {
            if stream.write_all(chunk).is_err() {
                break;
            }
            thread::sleep(delay);
        }
    });
    SlowTestServer {
        url,
        accepted,
        handle,
    }
}

fn test_model_manifest(source: &str, sha256: &str) -> String {
    test_model_manifest_for_io("test-model", "1.0.0", "audio", "stems", sha256)
        .replace("https://example.test/test-model.safetensors", source)
        .replace("test-model.Model", "test.Model")
}

fn test_model_manifest_for_io(
    name: &str,
    version: &str,
    input: &str,
    output: &str,
    sha256: &str,
) -> String {
    format!(
        r#"
[package]
name = "{name}"
version = "{version}"
description = "{name} model"
publisher = "apm-core"

[runtime]
mode = "native-mlx"
entry = "{name}.Model"

[weights]
source = "https://example.test/{name}.safetensors"
sha256 = "{sha256}"
format = "safetensors"

[io]
input = "{input}"
output = "{output}"

[license]
spdx = "MIT"
commercial = true

[hardware]
min_memory_gb = 8
"#
    )
}
