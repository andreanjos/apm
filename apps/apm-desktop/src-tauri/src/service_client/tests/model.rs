use super::support::*;
use super::*;

use apm_core::engine::PackageSearchResult;

#[test]
fn catalog_snapshot_sends_loopback_token() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind fake service");
    let port = listener.local_addr().expect("fake service addr").port();
    let token_path = write_test_token("catalog");
    let server = thread::spawn(move || {
        let request = accept_request(&listener);
        assert!(request.contains("GET /v1/packages?limit=24 HTTP/1.1"));
        assert!(request.contains("x-apm-token: secret\r\n"));
        write_json_response(request.stream, "200 OK", r#"{"status":"catalog_empty"}"#);
    });

    let session = test_session(port, &token_path);
    let client = DesktopServiceClient::from_session(&session).expect("create client");
    assert_eq!(
        client.catalog_snapshot(24).expect("catalog response"),
        PackageSearchResult::CatalogEmpty
    );
    server.join().expect("fake service should finish");
    let _ = fs::remove_file(token_path);
}

#[test]
fn model_catalog_sends_loopback_token() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind fake service");
    let port = listener.local_addr().expect("fake service addr").port();
    let token_path = write_test_token("model-catalog");
    let server = thread::spawn(move || {
        let request = accept_request(&listener);
        assert!(request.contains("GET /v1/models/catalog HTTP/1.1"));
        assert!(request.contains("x-apm-token: secret\r\n"));
        write_json_response(request.stream, "200 OK", r#"{"packages":[]}"#);
    });

    let session = test_session(port, &token_path);
    let client = DesktopServiceClient::from_session(&session).expect("create client");
    let result = client.model_catalog().expect("model catalog response");

    assert!(result.packages.is_empty());
    server.join().expect("fake service should finish");
    let _ = fs::remove_file(token_path);
}

#[test]
fn model_store_sends_loopback_token() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind fake service");
    let port = listener.local_addr().expect("fake service addr").port();
    let token_path = write_test_token("model-store");
    let server = thread::spawn(move || {
        let request = accept_request(&listener);
        assert!(request.contains("GET /v1/models/store HTTP/1.1"));
        assert!(request.contains("x-apm-token: secret\r\n"));
        write_json_response(
            request.stream,
            "200 OK",
            r#"{"root":"/tmp/.apm","manifests":"/tmp/.apm/manifests","weights":"/tmp/.apm/weights","runtimes":"/tmp/.apm/runtimes","cache":"/tmp/.apm/cache","logs":"/tmp/.apm/logs","config":"/tmp/.apm/config.toml"}"#,
        );
    });

    let session = test_session(port, &token_path);
    let client = DesktopServiceClient::from_session(&session).expect("create client");
    let result = client.model_store().expect("model store response");

    assert_eq!(result.root, "/tmp/.apm");
    assert_eq!(result.weights, "/tmp/.apm/weights");
    server.join().expect("fake service should finish");
    let _ = fs::remove_file(token_path);
}

#[test]
fn initialize_model_store_posts_loopback_token() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind fake service");
    let port = listener.local_addr().expect("fake service addr").port();
    let token_path = write_test_token("model-store-init");
    let server = thread::spawn(move || {
        let request = accept_request(&listener);
        assert!(request.contains("POST /v1/models/store/init HTTP/1.1"));
        assert!(request.contains("x-apm-token: secret\r\n"));
        write_json_response(
            request.stream,
            "200 OK",
            r#"{"layout":{"root":"/tmp/.apm","manifests":"/tmp/.apm/manifests","weights":"/tmp/.apm/weights","runtimes":"/tmp/.apm/runtimes","cache":"/tmp/.apm/cache","logs":"/tmp/.apm/logs","config":"/tmp/.apm/config.toml"}}"#,
        );
    });

    let session = test_session(port, &token_path);
    let client = DesktopServiceClient::from_session(&session).expect("create client");
    let result = client
        .initialize_model_store()
        .expect("model store init response");

    assert_eq!(result.layout.root, "/tmp/.apm");
    assert_eq!(result.layout.manifests, "/tmp/.apm/manifests");
    server.join().expect("fake service should finish");
    let _ = fs::remove_file(token_path);
}

#[test]
fn cache_model_catalog_manifest_posts_safe_package_path() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind fake service");
    let port = listener.local_addr().expect("fake service addr").port();
    let token_path = write_test_token("model-catalog-cache");
    let server = thread::spawn(move || {
        let request = accept_request(&listener);
        assert!(request.contains("POST /v1/models/catalog/demucs/4.0.1/cache HTTP/1.1"));
        assert!(request.contains("x-apm-token: secret\r\n"));
        write_json_response(
            request.stream,
            "200 OK",
            r#"{"model":{"package":{"package_id":"demucs@4.0.1","name":"demucs","version":"4.0.1","description":"Music source separation","publisher":"apm-core","runtime_mode":"native-mlx","input":"audio","output":"stems","parameter_count":0,"min_memory_gb":8,"commercial_license":true},"runtime_entry":"demucs.Model","weights":{"source":"https://example.test/model.safetensors","sha256":"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef","format":"safetensors","cached":false},"params":[]},"manifest_path":"/tmp/demucs.toml","replaced":false}"#,
        );
    });

    let session = test_session(port, &token_path);
    let client = DesktopServiceClient::from_session(&session).expect("create client");
    let result = client
        .cache_model_catalog_manifest("demucs".to_string(), "4.0.1".to_string())
        .expect("cache catalog model response");

    assert_eq!(result.model.package.package_id, "demucs@4.0.1");
    assert!(!result.replaced);
    server.join().expect("fake service should finish");
    let _ = fs::remove_file(token_path);
}

#[test]
fn model_run_plan_posts_request_body() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind fake service");
    let port = listener.local_addr().expect("fake service addr").port();
    let token_path = write_test_token("model-run-plan");
    let server = thread::spawn(move || {
        let request = accept_request(&listener);
        assert!(request.contains("POST /v1/models/demucs/4.0.1/run/plan HTTP/1.1"));
        assert!(request.contains("x-apm-token: secret\r\n"));
        assert!(request.contains(r#""input_path":"mix.wav""#));
        assert!(request.contains(r#""output_path":"stems/""#));
        write_json_response(
            request.stream,
            "200 OK",
            r#"{"package_id":"demucs@4.0.1","status":"planned","runtime_mode":"native-mlx","runtime_entry":"demucs.Model","adapter":"native-mlx","runtime_dir":"/tmp/.apm/runtimes/native-mlx/demucs/4.0.1","adapter_manifest_path":"/tmp/.apm/runtimes/native-mlx/demucs/4.0.1/adapter.toml","weights_path":"/tmp/.apm/weights/0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef","input_path":"mix.wav","output_path":"stems/","params":[{"name":"stems","value":"4","source":"default"}],"execution":{"status":"blocked","blocker":"adapter_runner_unavailable","message":"native-mlx execution for demucs@4.0.1 is not implemented yet; this plan is review-only."},"message":"Runtime execution is pending."}"#,
        );
    });

    let session = test_session(port, &token_path);
    let client = DesktopServiceClient::from_session(&session).expect("create client");
    let result = client
        .plan_model_run(
            "demucs".to_string(),
            "4.0.1".to_string(),
            ModelRunPlanRequest::new("mix.wav", "stems/"),
        )
        .expect("model run plan response");

    assert_eq!(result.package_id, "demucs@4.0.1");
    assert_eq!(result.input_path, "mix.wav");
    assert_eq!(result.output_path, "stems/");
    assert_eq!(result.params[0].name, "stems");
    server.join().expect("fake service should finish");
    let _ = fs::remove_file(token_path);
}
