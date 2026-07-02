use super::*;
use std::{fs, net::TcpListener, thread};

use apm_core::{config::InstallScope, model::ModelChainStepRequest};

mod model;
mod support;

use support::*;

#[test]
fn model_run_submits_operation_and_preserves_blocked_result() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind fake service");
    let port = listener.local_addr().expect("fake service addr").port();
    let token_path = write_test_token("model-run");
    let server = thread::spawn(move || {
        let post = accept_request(&listener);
        assert!(post.contains("POST /v1/models/demucs/4.0.1/run HTTP/1.1"));
        assert!(post.contains("x-apm-token: secret\r\n"));
        assert!(post.contains(r#""input_path":"mix.wav""#));
        assert!(post.contains(r#""output_path":"stems/""#));
        write_json_response(
            post.stream,
            "202 Accepted",
            r#"{"operation_id":"op-model-run","kind":"model_run","status_url":"/v1/operations/op-model-run"}"#,
        );

        let events = accept_request(&listener);
        assert!(events.contains("GET /v1/operations/op-model-run/events HTTP/1.1"));
        write_event_stream_response(
            events.stream,
            "event: engine_event\ndata: {\"event\":\"model_run_blocked\",\"package_id\":\"demucs@4.0.1\",\"blocker\":\"adapter_runner_unavailable\",\"message\":\"native-mlx execution is not implemented yet\"}\n\n",
        );

        let get = accept_request(&listener);
        assert!(get.contains("GET /v1/operations/op-model-run HTTP/1.1"));
        write_json_response(
            get.stream,
            "200 OK",
            r#"{"operation_id":"op-model-run","kind":"model_run","state":"failed","created_at":"2026-06-30T00:00:00Z","started_at":"2026-06-30T00:00:00Z","finished_at":"2026-06-30T00:00:01Z","result":{"kind":"model_run","result":{"package_id":"demucs@4.0.1","status":"blocked","plan":{"package_id":"demucs@4.0.1","status":"planned","runtime_mode":"native-mlx","runtime_entry":"demucs.Model","adapter":"native-mlx","runtime_dir":"/tmp/.apm/runtimes/native-mlx/demucs/4.0.1","adapter_manifest_path":"/tmp/.apm/runtimes/native-mlx/demucs/4.0.1/adapter.toml","weights_path":"/tmp/.apm/weights/0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef","input_path":"mix.wav","output_path":"stems/","params":[],"execution":{"status":"blocked","blocker":"adapter_runner_unavailable","message":"native-mlx execution is not implemented yet"},"message":"Runtime execution is pending."},"message":"native-mlx execution is not implemented yet"}},"error":"native-mlx execution is not implemented yet","events":[{"event":"model_run_blocked","package_id":"demucs@4.0.1","blocker":"adapter_runner_unavailable","message":"native-mlx execution is not implemented yet"}]}"#,
        );
    });

    let session = test_session(port, &token_path);
    let client = DesktopServiceClient::from_session(&session).expect("create client");
    let mut streamed_events = Vec::new();
    let status = client
        .run_model_with_events(
            "demucs".to_string(),
            "4.0.1".to_string(),
            ModelRunPlanRequest::new("mix.wav", "stems/"),
            |operation_id, kind, event| {
                assert_eq!(operation_id, "op-model-run");
                streamed_events.push((kind, event));
            },
        )
        .expect("model run status");

    assert_eq!(status.state, OperationState::Failed);
    assert_eq!(streamed_events.len(), 1);
    let (kind, event) = &streamed_events[0];
    assert_eq!(kind, &OperationKind::ModelRun);
    match event {
        EngineEvent::ModelRunBlocked { package_id, .. } => {
            assert_eq!(package_id, "demucs@4.0.1");
        }
        other => panic!("expected model run blocked event, got {other:?}"),
    }
    match status.result {
        Some(OperationResult::ModelRun(result)) => {
            assert_eq!(result.package_id(), "demucs@4.0.1");
            assert_eq!(
                result.message(),
                "native-mlx execution is not implemented yet"
            );
        }
        other => panic!("expected structured model run result, got {other:?}"),
    }

    server.join().expect("fake service should finish");
    let _ = fs::remove_file(token_path);
}

#[test]
fn model_chain_plan_posts_request_body() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind fake service");
    let port = listener.local_addr().expect("fake service addr").port();
    let token_path = write_test_token("model-chain-plan");
    let server = thread::spawn(move || {
        let request = accept_request(&listener);
        assert!(request.contains("POST /v1/models/chains/plan HTTP/1.1"));
        assert!(request.contains("x-apm-token: secret\r\n"));
        assert!(request.contains(r#""input_path":"mix.wav""#));
        assert!(request.contains(r#""output_path":"stems/""#));
        assert!(request.contains(r#""name":"demucs""#));
        assert!(request.contains(r#""version":"4.0.1""#));
        write_json_response(
            request.stream,
            "200 OK",
            r#"{"status":"planned","input_path":"mix.wav","output_path":"stems/","input":"audio","output":"stems","steps":[{"step_index":0,"package_id":"demucs@4.0.1","runtime_mode":"native-mlx","runtime_entry":"demucs.Model","adapter":"native-mlx","input":"audio","output":"stems","weights_path":"/tmp/.apm/weights/0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef","runtime_dir":"/tmp/.apm/runtimes/native-mlx/demucs/4.0.1","adapter_manifest_path":"/tmp/.apm/runtimes/native-mlx/demucs/4.0.1/adapter.toml","params":[{"name":"stems","value":"4","source":"default"}],"execution":{"status":"blocked","blocker":"adapter_runner_unavailable","message":"native-mlx execution for demucs@4.0.1 is not implemented yet; this plan is review-only."}}],"edges":[],"execution":{"status":"blocked","blocker":"chain_runner_unavailable","message":"Chain execution for 1 prepared step is not implemented yet; this plan is review-only."},"message":"Runtime chain execution is pending; this plan validates 1 prepared step and 0 IO edges."}"#,
        );
    });

    let session = test_session(port, &token_path);
    let client = DesktopServiceClient::from_session(&session).expect("create client");
    let result = client
        .plan_model_chain(ModelChainPlanRequest {
            input_path: "mix.wav".to_string(),
            output_path: "stems/".to_string(),
            steps: vec![ModelChainStepRequest::new("demucs", "4.0.1")],
        })
        .expect("model chain plan response");

    assert_eq!(result.input_path, "mix.wav");
    assert_eq!(result.output_path, "stems/");
    assert_eq!(result.steps[0].package_id, "demucs@4.0.1");
    assert!(result.edges.is_empty());
    server.join().expect("fake service should finish");
    let _ = fs::remove_file(token_path);
}

#[test]
fn registry_sync_submits_operation_and_reads_result() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind fake service");
    let port = listener.local_addr().expect("fake service addr").port();
    let token_path = write_test_token("sync");
    let server = thread::spawn(move || {
        let post = accept_request(&listener);
        assert!(post.contains("POST /v1/registry/sync HTTP/1.1"));
        assert!(post.contains("x-apm-token: secret\r\n"));
        write_json_response(
            post.stream,
            "202 Accepted",
            r#"{"operation_id":"op-1","kind":"registry_sync","status_url":"/v1/operations/op-1"}"#,
        );

        let events = accept_request(&listener);
        assert!(events.contains("GET /v1/operations/op-1/events HTTP/1.1"));
        assert!(events.contains("x-apm-token: secret\r\n"));
        write_event_stream_response(events.stream, "");

        let get = accept_request(&listener);
        assert!(get.contains("GET /v1/operations/op-1 HTTP/1.1"));
        assert!(get.contains("x-apm-token: secret\r\n"));
        write_json_response(
            get.stream,
            "200 OK",
            r#"{"operation_id":"op-1","kind":"registry_sync","state":"succeeded","created_at":"2026-06-30T00:00:00Z","started_at":"2026-06-30T00:00:00Z","finished_at":"2026-06-30T00:00:01Z","result":{"kind":"registry_sync","result":{"sources":[{"status":"ok","name":"official","catalog_item_count":1,"installable_product_count":1}]}},"error":null,"events":[]}"#,
        );
    });

    let session = test_session(port, &token_path);
    let client = DesktopServiceClient::from_session(&session).expect("create client");
    let result = client
        .registry_sync_with_events(|_, _, _| {})
        .expect("registry sync result");

    assert_eq!(result.sources.len(), 1);
    server.join().expect("fake service should finish");
    let _ = fs::remove_file(token_path);
}

#[test]
fn library_scan_submits_operation_and_reads_result() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind fake service");
    let port = listener.local_addr().expect("fake service addr").port();
    let token_path = write_test_token("library-scan");
    let server = thread::spawn(move || {
        let post = accept_request(&listener);
        assert!(post.contains("POST /v1/library/scan HTTP/1.1"));
        assert!(post.contains("x-apm-token: secret\r\n"));
        write_json_response(
            post.stream,
            "202 Accepted",
            r#"{"operation_id":"op-scan","kind":"library_scan","status_url":"/v1/operations/op-scan"}"#,
        );

        let events = accept_request(&listener);
        assert!(events.contains("GET /v1/operations/op-scan/events HTTP/1.1"));
        write_event_stream_response(
            events.stream,
            "event: engine_event\ndata: {\"event\":\"scan_finished\",\"scanned_count\":2,\"matched_count\":1,\"adopted_count\":1}\n\n",
        );

        let get = accept_request(&listener);
        assert!(get.contains("GET /v1/operations/op-scan HTTP/1.1"));
        write_json_response(
            get.stream,
            "200 OK",
            r#"{"operation_id":"op-scan","kind":"library_scan","state":"succeeded","created_at":"2026-07-01T00:00:00Z","started_at":"2026-07-01T00:00:00Z","finished_at":"2026-07-01T00:00:01Z","result":{"kind":"library_scan","result":{"scanned_count":2,"visible_count":2,"matched_count":1,"tracked_count":1,"adopted_count":1,"learned_bundle_id_count":1,"au_count":0,"vst3_count":2,"plugins":[]}},"error":null,"events":[{"event":"scan_finished","scanned_count":2,"matched_count":1,"adopted_count":1}]}"#,
        );
    });

    let session = test_session(port, &token_path);
    let client = DesktopServiceClient::from_session(&session).expect("create client");
    let mut streamed_events = Vec::new();
    let outcome = client
        .scan_library_with_events(|operation_id, kind, event| {
            assert_eq!(operation_id, "op-scan");
            streamed_events.push((kind, event));
        })
        .expect("library scan outcome");

    assert_eq!(streamed_events.len(), 1);
    assert_eq!(streamed_events[0].0, OperationKind::LibraryScan);
    match outcome {
        ServiceOperationOutcome::Completed { result, events } => {
            assert_eq!(result.adopted_count, 1);
            assert_eq!(events.len(), 1);
        }
        ServiceOperationOutcome::Failed { error, .. } => {
            panic!("library scan should complete: {error}");
        }
    }

    server.join().expect("fake service should finish");
    let _ = fs::remove_file(token_path);
}

#[test]
fn install_from_archive_submits_operation_and_preserves_events() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind fake service");
    let port = listener.local_addr().expect("fake service addr").port();
    let token_path = write_test_token("archive");
    let server = thread::spawn(move || {
        let post = accept_request(&listener);
        assert!(post.contains("POST /v1/install/archive HTTP/1.1"));
        assert!(post.contains("x-apm-token: secret\r\n"));
        assert!(post.contains(r#""slug":"surge-xt""#));
        assert!(post.contains(r#""archive_path":"/tmp/surge.zip""#));
        assert!(post.contains(r#""scope":"system""#));
        write_json_response(
            post.stream,
            "202 Accepted",
            r#"{"operation_id":"op-archive","kind":"install_archive","status_url":"/v1/operations/op-archive"}"#,
        );

        let events = accept_request(&listener);
        assert!(events.contains("GET /v1/operations/op-archive/events HTTP/1.1"));
        write_event_stream_response(events.stream, "");

        let get = accept_request(&listener);
        assert!(get.contains("GET /v1/operations/op-archive HTTP/1.1"));
        write_json_response(
            get.stream,
            "200 OK",
            r#"{"operation_id":"op-archive","kind":"install_archive","state":"succeeded","created_at":"2026-06-30T00:00:00Z","started_at":"2026-06-30T00:00:00Z","finished_at":"2026-06-30T00:00:01Z","result":{"kind":"install_package","result":{"status":"plan_unavailable","plan":{"status":"catalog_empty"}}},"error":null,"events":[{"event":"install_failed","slug":"surge-xt","error":"test event"}]}"#,
        );
    });

    let session = test_session(port, &token_path);
    let client = DesktopServiceClient::from_session(&session).expect("create client");
    let outcome = client
        .install_from_archive_with_events(
            InstallPackageRequest {
                slug: "surge-xt".to_string(),
                archive_path: Some("/tmp/surge.zip".into()),
                scope: Some(InstallScope::System),
                ..InstallPackageRequest::default()
            },
            |_, _, _| {},
        )
        .expect("archive install outcome");

    match outcome {
        ServiceOperationOutcome::Completed { result, events } => {
            assert!(matches!(
                result,
                InstallPackageResult::PlanUnavailable { .. }
            ));
            assert_eq!(events.len(), 1);
        }
        ServiceOperationOutcome::Failed { error, .. } => {
            panic!("archive install should complete: {error}");
        }
    }

    server.join().expect("fake service should finish");
    let _ = fs::remove_file(token_path);
}

#[test]
fn install_from_archive_with_events_reads_operation_event_stream() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind fake service");
    let port = listener.local_addr().expect("fake service addr").port();
    let token_path = write_test_token("archive-events");
    let server = thread::spawn(move || {
        let post = accept_request(&listener);
        assert!(post.contains("POST /v1/install/archive HTTP/1.1"));
        write_json_response(
            post.stream,
            "202 Accepted",
            r#"{"operation_id":"op-archive","kind":"install_archive","status_url":"/v1/operations/op-archive"}"#,
        );

        let events = accept_request(&listener);
        assert!(events.contains("GET /v1/operations/op-archive/events HTTP/1.1"));
        write_event_stream_response(
                events.stream,
                "event: engine_event\ndata: {\"event\":\"install_state_recorded\",\"slug\":\"surge-xt\"}\n\n",
            );

        let status = accept_request(&listener);
        assert!(status.contains("GET /v1/operations/op-archive HTTP/1.1"));
        write_json_response(
            status.stream,
            "200 OK",
            r#"{"operation_id":"op-archive","kind":"install_archive","state":"succeeded","created_at":"2026-06-30T00:00:00Z","started_at":"2026-06-30T00:00:00Z","finished_at":"2026-06-30T00:00:01Z","result":{"kind":"install_package","result":{"status":"plan_unavailable","plan":{"status":"catalog_empty"}}},"error":null,"events":[]}"#,
        );
    });

    let session = test_session(port, &token_path);
    let client = DesktopServiceClient::from_session(&session).expect("create client");
    let mut streamed_events = Vec::new();
    let outcome = client
        .install_from_archive_with_events(
            InstallPackageRequest {
                slug: "surge-xt".to_string(),
                archive_path: Some("/tmp/surge.zip".into()),
                ..InstallPackageRequest::default()
            },
            |operation_id, kind, event| {
                assert_eq!(operation_id, "op-archive");
                streamed_events.push((kind, event));
            },
        )
        .expect("archive install outcome");

    assert_eq!(
        streamed_events,
        vec![(
            OperationKind::InstallArchive,
            EngineEvent::InstallStateRecorded {
                slug: "surge-xt".to_string()
            }
        )]
    );
    assert!(matches!(
        outcome,
        ServiceOperationOutcome::Completed {
            result: InstallPackageResult::PlanUnavailable { .. },
            ..
        }
    ));

    server.join().expect("fake service should finish");
    let _ = fs::remove_file(token_path);
}

#[test]
fn operation_event_observer_rejects_status_kind_mismatch() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind fake service");
    let port = listener.local_addr().expect("fake service addr").port();
    let token_path = write_test_token("kind-mismatch");
    let server = thread::spawn(move || {
        let post = accept_request(&listener);
        assert!(post.contains("POST /v1/install/archive HTTP/1.1"));
        write_json_response(
            post.stream,
            "202 Accepted",
            r#"{"operation_id":"op-archive","kind":"install_archive","status_url":"/v1/operations/op-archive"}"#,
        );

        let events = accept_request(&listener);
        assert!(events.contains("GET /v1/operations/op-archive/events HTTP/1.1"));
        write_event_stream_response(events.stream, "");

        let status = accept_request(&listener);
        assert!(status.contains("GET /v1/operations/op-archive HTTP/1.1"));
        write_json_response(
            status.stream,
            "200 OK",
            r#"{"operation_id":"op-archive","kind":"package_remove","state":"succeeded","created_at":"2026-06-30T00:00:00Z","started_at":"2026-06-30T00:00:00Z","finished_at":"2026-06-30T00:00:01Z","result":{"kind":"remove_package","result":{"status":"not_installed","slug":"surge-xt"}},"error":null,"events":[]}"#,
        );
    });

    let session = test_session(port, &token_path);
    let client = DesktopServiceClient::from_session(&session).expect("create client");
    let result = client.install_from_archive_with_events(
        InstallPackageRequest {
            slug: "surge-xt".to_string(),
            archive_path: Some("/tmp/surge.zip".into()),
            ..InstallPackageRequest::default()
        },
        |_, _, _| {},
    );

    match result {
        Ok(_) => panic!("operation kind mismatch should fail"),
        Err(error) => assert!(
            error.contains("operation kind mismatch"),
            "unexpected error: {error}"
        ),
    }
    server.join().expect("fake service should finish");
    let _ = fs::remove_file(token_path);
}

#[test]
fn remove_package_returns_failed_operation_events() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind fake service");
    let port = listener.local_addr().expect("fake service addr").port();
    let token_path = write_test_token("remove");
    let server = thread::spawn(move || {
        let post = accept_request(&listener);
        assert!(post.contains("POST /v1/packages/surge-xt/remove HTTP/1.1"));
        assert!(post.contains("x-apm-token: secret\r\n"));
        assert!(post.contains(r#""dry_run":false"#));
        write_json_response(
            post.stream,
            "202 Accepted",
            r#"{"operation_id":"op-remove","kind":"package_remove","status_url":"/v1/operations/op-remove"}"#,
        );

        let events = accept_request(&listener);
        assert!(events.contains("GET /v1/operations/op-remove/events HTTP/1.1"));
        write_event_stream_response(events.stream, "");

        let get = accept_request(&listener);
        assert!(get.contains("GET /v1/operations/op-remove HTTP/1.1"));
        write_json_response(
            get.stream,
            "200 OK",
            r#"{"operation_id":"op-remove","kind":"package_remove","state":"failed","created_at":"2026-06-30T00:00:00Z","started_at":"2026-06-30T00:00:00Z","finished_at":"2026-06-30T00:00:01Z","result":null,"error":"delete failed","events":[{"event":"remove_started","slug":"surge-xt","version":"1.0.0","format_count":1}]}"#,
        );
    });

    let session = test_session(port, &token_path);
    let client = DesktopServiceClient::from_session(&session).expect("create client");
    let outcome = client
        .remove_package_with_events(
            "surge-xt".to_string(),
            PackageRemoveBody { dry_run: false },
            |_, _, _| {},
        )
        .expect("remove outcome");

    match outcome {
        ServiceOperationOutcome::Failed { error, events } => {
            assert_eq!(error, "delete failed");
            assert_eq!(events.len(), 1);
        }
        ServiceOperationOutcome::Completed { .. } => {
            panic!("remove should return a failed operation");
        }
    }

    server.join().expect("fake service should finish");
    let _ = fs::remove_file(token_path);
}

#[test]
fn install_plan_posts_typed_request() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind fake service");
    let port = listener.local_addr().expect("fake service addr").port();
    let token_path = write_test_token("plan");
    let server = thread::spawn(move || {
        let request = accept_request(&listener);
        assert!(request.contains("POST /v1/install/plan HTTP/1.1"));
        assert!(request.contains("x-apm-token: secret\r\n"));
        assert!(request.contains(r#""slug":"surge-xt""#));
        assert!(request.contains(r#""scope":"system""#));
        write_json_response(request.stream, "200 OK", r#"{"status":"catalog_empty"}"#);
    });

    let session = test_session(port, &token_path);
    let client = DesktopServiceClient::from_session(&session).expect("create client");
    assert!(matches!(
        client.install_plan(InstallPlanRequest {
            slug: "surge-xt".to_string(),
            scope: Some(InstallScope::System),
            ..InstallPlanRequest::default()
        }),
        Ok(InstallPlanResult::CatalogEmpty)
    ));
    server.join().expect("fake service should finish");
    let _ = fs::remove_file(token_path);
}

#[test]
fn package_details_reads_safe_slug_with_versions() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind fake service");
    let port = listener.local_addr().expect("fake service addr").port();
    let token_path = write_test_token("package-details");
    let server = thread::spawn(move || {
        let request = accept_request(&listener);
        assert!(request.contains("GET /v1/packages/surge-xt?include_versions=true HTTP/1.1"));
        assert!(request.contains("x-apm-token: secret\r\n"));
        write_json_response(request.stream, "200 OK", r#"{"status":"not_found"}"#);
    });

    let session = test_session(port, &token_path);
    let client = DesktopServiceClient::from_session(&session).expect("create client");
    assert!(matches!(
        client.package_details("surge-xt".to_string()),
        Ok(PackageDetailsResult::NotFound)
    ));
    server.join().expect("fake service should finish");
    let _ = fs::remove_file(token_path);
}

#[test]
fn package_details_rejects_unsafe_path_slug() {
    let client = DesktopServiceClient {
        http: ServiceHttpClient::new(4767, "secret".to_string()),
    };
    let error = client
        .package_details("../surge-xt".to_string())
        .expect_err("unsafe slug should be rejected");

    assert!(error.contains("unsupported service path segment"));
}

#[test]
fn install_handoff_posts_typed_request() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind fake service");
    let port = listener.local_addr().expect("fake service addr").port();
    let token_path = write_test_token("handoff");
    let server = thread::spawn(move || {
        let request = accept_request(&listener);
        assert!(request.contains("POST /v1/install/handoff HTTP/1.1"));
        assert!(request.contains("x-apm-token: secret\r\n"));
        assert!(request.contains(r#""slug":"manual-effect""#));
        write_json_response(
            request.stream,
            "200 OK",
            r#"{"status":"plan_unavailable","plan":{"status":"catalog_empty"}}"#,
        );
    });

    let session = test_session(port, &token_path);
    let client = DesktopServiceClient::from_session(&session).expect("create client");
    assert!(matches!(
        client.install_handoff("manual-effect".to_string()),
        Ok(InstallHandoffResult::PlanUnavailable { .. })
    ));
    server.join().expect("fake service should finish");
    let _ = fs::remove_file(token_path);
}

#[test]
fn set_package_pin_posts_safe_slug_and_body() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind fake service");
    let port = listener.local_addr().expect("fake service addr").port();
    let token_path = write_test_token("pin");
    let server = thread::spawn(move || {
        let request = accept_request(&listener);
        assert!(request.contains("POST /v1/packages/surge-xt/pin HTTP/1.1"));
        assert!(request.contains("x-apm-token: secret\r\n"));
        assert!(request.contains(r#""pinned":true"#));
        write_json_response(
            request.stream,
            "200 OK",
            r#"{"status":"not_installed","slug":"surge-xt"}"#,
        );
    });

    let session = test_session(port, &token_path);
    let client = DesktopServiceClient::from_session(&session).expect("create client");
    assert!(matches!(
        client.set_package_pin("surge-xt".to_string(), true),
        Ok(SetPackagePinResult::NotInstalled { .. })
    ));
    server.join().expect("fake service should finish");
    let _ = fs::remove_file(token_path);
}

#[test]
fn set_package_pin_rejects_unsafe_path_slug() {
    let client = DesktopServiceClient {
        http: ServiceHttpClient::new(4767, "secret".to_string()),
    };
    let error = client
        .set_package_pin("../surge-xt".to_string(), true)
        .expect_err("unsafe slug should be rejected");

    assert!(error.contains("unsupported service path segment"));
}

#[test]
fn cancel_operation_posts_operation_control_request() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind fake service");
    let port = listener.local_addr().expect("fake service addr").port();
    let token_path = write_test_token("cancel");
    let server = thread::spawn(move || {
        let request = accept_request(&listener);
        assert!(request.contains("POST /v1/operations/op-1/cancel HTTP/1.1"));
        assert!(request.contains("x-apm-token: secret\r\n"));
        write_json_response(
            request.stream,
            "200 OK",
            r#"{"operation_id":"op-1","state":"cancel_requested","accepted":true,"message":"Cancellation requested"}"#,
        );
    });

    let session = test_session(port, &token_path);
    let client = DesktopServiceClient::from_session(&session).expect("create client");
    let result = client
        .cancel_operation("op-1".to_string())
        .expect("cancel result");

    assert!(result.accepted);
    assert_eq!(result.state, OperationState::CancelRequested);
    server.join().expect("fake service should finish");
    let _ = fs::remove_file(token_path);
}

#[test]
fn retry_operation_posts_operation_control_request() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind fake service");
    let port = listener.local_addr().expect("fake service addr").port();
    let token_path = write_test_token("retry");
    let server = thread::spawn(move || {
        let request = accept_request(&listener);
        assert!(request.contains("POST /v1/operations/op-1/retry HTTP/1.1"));
        assert!(request.contains("x-apm-token: secret\r\n"));
        write_json_response(
            request.stream,
            "202 Accepted",
            r#"{"original_operation_id":"op-1","operation":{"operation_id":"op-2","kind":"registry_sync","status_url":"/v1/operations/op-2"},"message":"Retry operation accepted."}"#,
        );
    });

    let session = test_session(port, &token_path);
    let client = DesktopServiceClient::from_session(&session).expect("create client");
    let result = client
        .retry_operation("op-1".to_string())
        .expect("retry result");

    assert_eq!(result.original_operation_id, "op-1");
    assert_eq!(result.operation.operation_id, "op-2");
    server.join().expect("fake service should finish");
    let _ = fs::remove_file(token_path);
}

#[test]
fn retry_operation_with_events_streams_retried_operation_to_terminal_status() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind fake service");
    let port = listener.local_addr().expect("fake service addr").port();
    let token_path = write_test_token("retry-events");
    let server = thread::spawn(move || {
        let retry = accept_request(&listener);
        assert!(retry.contains("POST /v1/operations/op-1/retry HTTP/1.1"));
        write_json_response(
            retry.stream,
            "202 Accepted",
            r#"{"original_operation_id":"op-1","operation":{"operation_id":"op-2","kind":"registry_sync","status_url":"/v1/operations/op-2"},"message":"Retry operation accepted."}"#,
        );

        let events = accept_request(&listener);
        assert!(events.contains("GET /v1/operations/op-2/events HTTP/1.1"));
        write_event_stream_response(
                events.stream,
                "event: engine_event\ndata: {\"event\":\"registry_sync_started\",\"source_count\":1}\n\n",
            );

        let status = accept_request(&listener);
        assert!(status.contains("GET /v1/operations/op-2 HTTP/1.1"));
        write_json_response(
            status.stream,
            "200 OK",
            r#"{"operation_id":"op-2","kind":"registry_sync","request":{"kind":"registry_sync"},"state":"succeeded","created_at":"2026-06-30T00:00:00Z","started_at":"2026-06-30T00:00:00Z","finished_at":"2026-06-30T00:00:01Z","result":{"kind":"registry_sync","result":{"sources":[]}},"error":null,"events":[{"event":"registry_sync_started","source_count":1}]}"#,
        );
    });

    let session = test_session(port, &token_path);
    let client = DesktopServiceClient::from_session(&session).expect("create client");
    let mut streamed_events = Vec::new();
    let status = client
        .retry_operation_with_events("op-1".to_string(), |operation_id, kind, event| {
            assert_eq!(operation_id, "op-2");
            streamed_events.push((kind, event));
        })
        .expect("retry status");

    assert_eq!(status.operation_id, "op-2");
    assert_eq!(status.state, OperationState::Succeeded);
    assert_eq!(
        streamed_events,
        vec![(
            OperationKind::RegistrySync,
            EngineEvent::RegistrySyncStarted { source_count: 1 }
        )]
    );
    server.join().expect("fake service should finish");
    let _ = fs::remove_file(token_path);
}

#[test]
fn retry_recovery_operations_with_events_streams_accepted_operations() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind fake service");
    let port = listener.local_addr().expect("fake service addr").port();
    let token_path = write_test_token("retry-recovery-events");
    let server = thread::spawn(move || {
        let retry = accept_request(&listener);
        assert!(retry.contains("POST /v1/operations/recovery/retry HTTP/1.1"));
        assert!(retry.contains("x-apm-token: secret\r\n"));
        write_json_response(
            retry.stream,
            "202 Accepted",
            r#"{"retried_count":1,"operations":[{"original_operation_id":"op-1","operation":{"operation_id":"op-2","kind":"registry_sync","status_url":"/v1/operations/op-2"},"message":"Retry operation accepted."}],"message":"Retry operation accepted."}"#,
        );

        let events = accept_request(&listener);
        assert!(events.contains("GET /v1/operations/op-2/events HTTP/1.1"));
        write_event_stream_response(
            events.stream,
            "event: engine_event\ndata: {\"event\":\"registry_sync_started\",\"source_count\":1}\n\n",
        );

        let status = accept_request(&listener);
        assert!(status.contains("GET /v1/operations/op-2 HTTP/1.1"));
        write_json_response(
            status.stream,
            "200 OK",
            r#"{"operation_id":"op-2","kind":"registry_sync","request":{"kind":"registry_sync"},"state":"succeeded","created_at":"2026-06-30T00:00:00Z","started_at":"2026-06-30T00:00:00Z","finished_at":"2026-06-30T00:00:01Z","result":{"kind":"registry_sync","result":{"sources":[]}},"error":null,"events":[{"event":"registry_sync_started","source_count":1}]}"#,
        );
    });

    let session = test_session(port, &token_path);
    let client = DesktopServiceClient::from_session(&session).expect("create client");
    let mut streamed_events = Vec::new();
    let statuses = client
        .retry_recovery_operations_with_events(|operation_id, kind, event| {
            assert_eq!(operation_id, "op-2");
            streamed_events.push((kind, event));
        })
        .expect("recovery retry statuses");

    assert_eq!(statuses.len(), 1);
    assert_eq!(statuses[0].operation_id, "op-2");
    assert_eq!(statuses[0].state, OperationState::Succeeded);
    assert_eq!(
        streamed_events,
        vec![(
            OperationKind::RegistrySync,
            EngineEvent::RegistrySyncStarted { source_count: 1 }
        )]
    );
    server.join().expect("fake service should finish");
    let _ = fs::remove_file(token_path);
}

#[test]
fn operation_history_reads_recent_operations() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind fake service");
    let port = listener.local_addr().expect("fake service addr").port();
    let token_path = write_test_token("operation-history");
    let server = thread::spawn(move || {
        let request = accept_request(&listener);
        assert!(request.contains("GET /v1/operations HTTP/1.1"));
        assert!(request.contains("x-apm-token: secret\r\n"));
        write_json_response(
            request.stream,
            "200 OK",
            r#"[{"operation_id":"op-1","kind":"registry_sync","state":"succeeded","created_at":"2026-06-30T00:00:00Z","started_at":"2026-06-30T00:00:00Z","finished_at":"2026-06-30T00:00:01Z","result":null,"error":null,"events":[]}]"#,
        );
    });

    let session = test_session(port, &token_path);
    let client = DesktopServiceClient::from_session(&session).expect("create client");
    let history = client.operation_history().expect("operation history");

    assert_eq!(history[0].operation_id, "op-1");
    assert_eq!(history[0].state, OperationState::Succeeded);
    server.join().expect("fake service should finish");
    let _ = fs::remove_file(token_path);
}

#[test]
fn operation_recovery_reads_restart_interrupted_summary() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind fake service");
    let port = listener.local_addr().expect("fake service addr").port();
    let token_path = write_test_token("operation-recovery");
    let server = thread::spawn(move || {
        let request = accept_request(&listener);
        assert!(request.contains("GET /v1/operations/recovery HTTP/1.1"));
        assert!(request.contains("x-apm-token: secret\r\n"));
        write_json_response(
            request.stream,
            "200 OK",
            r#"{"interrupted_count":1,"retryable_count":1,"candidates":[{"operation_id":"op-1","kind":"registry_sync","created_at":"2026-06-30T00:00:00Z","finished_at":"2026-06-30T00:00:01Z","retryable":true,"reason":"Operation did not finish before the service restarted."}]}"#,
        );
    });

    let session = test_session(port, &token_path);
    let client = DesktopServiceClient::from_session(&session).expect("create client");
    let recovery = client.operation_recovery().expect("operation recovery");

    assert_eq!(recovery.interrupted_count, 1);
    assert_eq!(recovery.retryable_count, 1);
    assert_eq!(recovery.candidates[0].operation_id, "op-1");
    server.join().expect("fake service should finish");
    let _ = fs::remove_file(token_path);
}

#[test]
fn diagnostics_report_reads_doctor_checks() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind fake service");
    let port = listener.local_addr().expect("fake service addr").port();
    let token_path = write_test_token("diagnostics");
    let server = thread::spawn(move || {
        let request = accept_request(&listener);
        assert!(request.contains("GET /v1/diagnostics HTTP/1.1"));
        assert!(request.contains("x-apm-token: secret\r\n"));
        write_json_response(
            request.stream,
            "200 OK",
            r#"{"checks":[{"name":"State file","status":"ok","detail":"ready"}],"summary":{"ok":1,"warnings":0,"failures":0}}"#,
        );
    });

    let session = test_session(port, &token_path);
    let client = DesktopServiceClient::from_session(&session).expect("create client");
    let report = client.diagnostics_report().expect("diagnostics report");

    assert_eq!(report.summary.ok, 1);
    assert_eq!(report.checks[0].name, "State file");
    server.join().expect("fake service should finish");
    let _ = fs::remove_file(token_path);
}
