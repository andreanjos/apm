use std::fs;

use apm_core::engine::{EngineEvent, InstallPackageRequest, OPERATION_CANCELED_BY_REQUEST};
use apm_core::service::{
    OperationKind, OperationRequest, OperationState, OperationStatus, PackageRemoveBody,
};
use chrono::{DateTime, TimeZone, Utc};

use super::*;

fn test_store() -> (tempfile::TempDir, OperationStore) {
    let temp = tempfile::tempdir().expect("temp dir");
    let store = OperationStore::new(temp.path().join("operations.json")).expect("store");
    (temp, store)
}

fn test_operation_status(operation_id: &str, created_at: DateTime<Utc>) -> OperationStatus {
    OperationStatus {
        operation_id: operation_id.to_string(),
        kind: OperationKind::RegistrySync,
        request: None,
        state: OperationState::Succeeded,
        created_at,
        started_at: None,
        finished_at: Some(created_at),
        result: None,
        error: None,
        events: Vec::new(),
    }
}

fn timestamp(seconds: i64) -> DateTime<Utc> {
    Utc.timestamp_opt(seconds, 0).single().expect("timestamp")
}

fn sync_event(source: &str) -> EngineEvent {
    EngineEvent::RegistrySourceSyncStarted {
        source: source.to_string(),
    }
}

#[test]
fn cancel_queued_operation_finishes_without_starting() {
    let (_temp, store) = test_store();
    let accepted = store.accept(OperationRequest::RegistrySync);

    let result = store
        .cancel(&accepted.operation_id)
        .expect("operation should exist");

    assert!(result.accepted);
    assert_eq!(result.state, OperationState::Canceled);
    assert!(!store.mark_running(&accepted.operation_id));

    let status = store.get(&accepted.operation_id).expect("status");
    assert_eq!(status.state, OperationState::Canceled);
    assert!(status.finished_at.is_some());
    assert_eq!(
        status.error.as_deref(),
        Some(OPERATION_CANCELED_BEFORE_START)
    );
}

#[test]
fn cancel_running_operation_records_cancel_request() {
    let (_temp, store) = test_store();
    let accepted = store.accept(OperationRequest::RegistrySync);
    assert!(store.mark_running(&accepted.operation_id));

    let result = store
        .cancel(&accepted.operation_id)
        .expect("operation should exist");

    assert!(result.accepted);
    assert_eq!(result.state, OperationState::CancelRequested);

    let status = store.get(&accepted.operation_id).expect("status");
    assert_eq!(status.state, OperationState::CancelRequested);
    assert!(status.started_at.is_some());
    assert!(status.finished_at.is_none());
}

#[test]
fn canceled_engine_result_finishes_cancel_requested_operation() {
    let (_temp, store) = test_store();
    let accepted = store.accept(OperationRequest::PackageRemove {
        slug: "surge-xt".to_string(),
        body: PackageRemoveBody { dry_run: false },
    });
    assert!(store.mark_running(&accepted.operation_id));
    store
        .cancel(&accepted.operation_id)
        .expect("cancel request");

    store.finish_package_remove(
        &accepted.operation_id,
        Err(apm_core::ApmError::OperationCanceled.into()),
    );

    let status = store.get(&accepted.operation_id).expect("status");
    assert_eq!(status.state, OperationState::Canceled);
    assert_eq!(status.error.as_deref(), Some(OPERATION_CANCELED_BY_REQUEST));
    assert!(status.finished_at.is_some());
}

#[test]
fn cancel_terminal_operation_is_not_accepted() {
    let (_temp, store) = test_store();
    let accepted = store.accept(OperationRequest::RegistrySync);
    store
        .cancel(&accepted.operation_id)
        .expect("initial cancel");

    let result = store
        .cancel(&accepted.operation_id)
        .expect("operation should exist");

    assert!(!result.accepted);
    assert_eq!(result.state, OperationState::Canceled);
    assert_eq!(result.message, OPERATION_ALREADY_TERMINAL);
}

#[test]
fn accepted_operation_persists_request_metadata() {
    let (temp, store) = test_store();
    let request = OperationRequest::InstallArchive {
        request: InstallPackageRequest {
            slug: "surge-xt".to_string(),
            version: Some("1.3.3".to_string()),
            ..InstallPackageRequest::default()
        },
    };
    let accepted = store.accept(request.clone());

    let status = store.get(&accepted.operation_id).expect("status");
    assert_eq!(status.kind, OperationKind::InstallArchive);
    assert_eq!(status.request.as_ref(), Some(&request));

    let history_content =
        fs::read_to_string(temp.path().join("operations.json")).expect("history file");
    let history: OperationHistoryFile =
        serde_json::from_str(&history_content).expect("history json");
    assert_eq!(history.schema_version, OPERATION_HISTORY_SCHEMA_VERSION);

    let restored =
        OperationStore::new(temp.path().join("operations.json")).expect("restored store");
    let restored_status = restored
        .get(&accepted.operation_id)
        .expect("restored status");
    assert_eq!(restored_status.request.as_ref(), Some(&request));
}

#[test]
fn write_operation_history_replaces_file_without_temp_leftover() {
    let temp = tempfile::tempdir().expect("temp dir");
    let history_path = temp.path().join("operations.json");
    let temp_path = history_path.with_extension("json.tmp");
    let status = test_operation_status("op-persisted", timestamp(42));

    write_operation_history(&history_path, vec![status]).expect("write history");

    let history_content = fs::read_to_string(&history_path).expect("history file");
    assert!(history_content.contains("op-persisted"));
    assert!(!temp_path.exists());
}

#[test]
fn write_operation_history_failure_keeps_existing_file() {
    let temp = tempfile::tempdir().expect("temp dir");
    let history_path = temp.path().join("operations.json");
    let temp_path = history_path.with_extension("json.tmp");
    let original = b"existing history";
    fs::write(&history_path, original).expect("seed history file");
    fs::create_dir(&temp_path).expect("block temp write path");

    let result = write_operation_history(
        &history_path,
        vec![test_operation_status("op-new", timestamp(43))],
    );

    assert!(result.is_err());
    assert_eq!(
        fs::read(&history_path).expect("history file"),
        original.to_vec()
    );
}

#[test]
fn retry_request_returns_saved_request_for_failed_operation() {
    let (_temp, store) = test_store();
    let request = OperationRequest::PackageRemove {
        slug: "surge-xt".to_string(),
        body: PackageRemoveBody { dry_run: false },
    };
    let accepted = store.accept(request.clone());
    assert!(store.mark_running(&accepted.operation_id));
    store.finish_package_remove(
        &accepted.operation_id,
        Err(anyhow::anyhow!("delete failed")),
    );

    let retry_request = store
        .retry_request(&accepted.operation_id)
        .expect("retry request");

    assert_eq!(retry_request, request);
}

#[test]
fn retry_request_rejects_running_succeeded_and_legacy_operations() {
    let (_temp, store) = test_store();
    let running = store.accept(OperationRequest::RegistrySync);
    assert!(store.mark_running(&running.operation_id));
    assert!(matches!(
        store.retry_request(&running.operation_id),
        Err(OperationRetryError::NotTerminal)
    ));

    let succeeded = store.accept(OperationRequest::RegistrySync);
    store.finish_registry_sync(
        &succeeded.operation_id,
        Ok(apm_core::engine::RegistrySyncResult {
            sources: Vec::new(),
        }),
    );
    assert!(matches!(
        store.retry_request(&succeeded.operation_id),
        Err(OperationRetryError::AlreadySucceeded)
    ));

    store.mutate_operations(|operations| {
        let mut status = test_operation_status("op-legacy", timestamp(42));
        status.state = OperationState::Failed;
        operations.insert(
            "op-legacy".to_string(),
            OperationRecord {
                status,
                sender: None,
            },
        );
    });
    assert!(matches!(
        store.retry_request("op-legacy"),
        Err(OperationRetryError::RequestUnavailable)
    ));
}

#[test]
fn recovery_summary_reports_restart_interrupted_retry_candidates() {
    let (temp, store) = test_store();
    let retryable = store.accept(OperationRequest::RegistrySync);
    assert!(store.mark_running(&retryable.operation_id));

    let mut legacy = test_operation_status("op-legacy", timestamp(42));
    legacy.state = OperationState::Running;
    legacy.finished_at = None;
    legacy.error = None;
    store.mutate_operations(|operations| {
        operations.insert(
            "op-legacy".to_string(),
            OperationRecord {
                status: legacy,
                sender: None,
            },
        );
    });

    let restored =
        OperationStore::new(temp.path().join("operations.json")).expect("restored store");
    let recovery = restored.recovery_summary();

    assert_eq!(recovery.interrupted_count, 2);
    assert_eq!(recovery.retryable_count, 1);
    assert!(recovery.candidates.iter().any(|candidate| {
        candidate.operation_id == retryable.operation_id && candidate.retryable
    }));
    assert!(recovery
        .candidates
        .iter()
        .any(|candidate| { candidate.operation_id == "op-legacy" && !candidate.retryable }));
}

#[test]
fn recovery_retry_requests_returns_only_retryable_restart_interruptions() {
    let (temp, store) = test_store();
    let first = store.accept(OperationRequest::RegistrySync);
    assert!(store.mark_running(&first.operation_id));

    let second = store.accept(OperationRequest::PackageRemove {
        slug: "surge-xt".to_string(),
        body: PackageRemoveBody { dry_run: false },
    });
    assert!(store.mark_running(&second.operation_id));

    let mut legacy = test_operation_status("op-legacy", timestamp(42));
    legacy.state = OperationState::Running;
    legacy.finished_at = None;
    legacy.error = None;
    store.mutate_operations(|operations| {
        operations.insert(
            "op-legacy".to_string(),
            OperationRecord {
                status: legacy,
                sender: None,
            },
        );
    });

    let restored =
        OperationStore::new(temp.path().join("operations.json")).expect("restored store");
    let requests = restored.recovery_retry_requests();
    let operation_ids = requests
        .iter()
        .map(|(operation_id, _)| operation_id.as_str())
        .collect::<Vec<_>>();

    assert_eq!(
        operation_ids,
        vec![first.operation_id.as_str(), second.operation_id.as_str()]
    );
    assert_eq!(requests[0].1, OperationRequest::RegistrySync);
    assert!(matches!(
        requests[1].1,
        OperationRequest::PackageRemove { .. }
    ));
}

#[test]
fn mark_recovery_retry_submitted_removes_restart_interrupted_recovery_candidate() {
    let (temp, store) = test_store();
    let retryable = store.accept(OperationRequest::RegistrySync);
    assert!(store.mark_running(&retryable.operation_id));

    let restored =
        OperationStore::new(temp.path().join("operations.json")).expect("restored store");
    assert_eq!(restored.recovery_summary().interrupted_count, 1);

    assert!(restored.mark_recovery_retry_submitted(&retryable.operation_id, "op-retry"));

    let status = restored.get(&retryable.operation_id).expect("status");
    assert_eq!(
        status.error.as_deref(),
        Some("Retry submitted as op-retry.")
    );
    assert_eq!(status.request, None);
    assert!(matches!(
        restored.retry_request(&retryable.operation_id),
        Err(OperationRetryError::RequestUnavailable)
    ));
    assert_eq!(restored.recovery_summary().interrupted_count, 0);

    let restored_again =
        OperationStore::new(temp.path().join("operations.json")).expect("restored store");
    assert_eq!(restored_again.recovery_summary().interrupted_count, 0);
    assert_eq!(
        restored_again
            .get(&retryable.operation_id)
            .expect("restored status")
            .request,
        None
    );
}

#[test]
fn retained_operation_history_keeps_latest_records_in_stable_order() {
    let retained = retained_operation_history(
        vec![
            test_operation_status("op-newer-b", timestamp(30)),
            test_operation_status("op-oldest", timestamp(10)),
            test_operation_status("op-newest", timestamp(40)),
            test_operation_status("op-newer-a", timestamp(30)),
        ],
        3,
    );

    let operation_ids: Vec<&str> = retained
        .iter()
        .map(|status| status.operation_id.as_str())
        .collect();

    assert_eq!(operation_ids, vec!["op-newer-a", "op-newer-b", "op-newest"]);
}

#[test]
fn retained_operation_history_can_drop_all_records() {
    let retained =
        retained_operation_history(vec![test_operation_status("op-1", timestamp(10))], 0);

    assert!(retained.is_empty());
}

#[test]
fn retain_recent_events_keeps_latest_event_tail() {
    let mut events = vec![
        sync_event("oldest"),
        sync_event("newer-a"),
        sync_event("newer-b"),
    ];

    retain_recent_events(&mut events, 2);

    assert_eq!(events, vec![sync_event("newer-a"), sync_event("newer-b")]);
}

#[test]
fn retain_recent_events_can_drop_all_events() {
    let mut events = vec![sync_event("oldest")];

    retain_recent_events(&mut events, 0);

    assert!(events.is_empty());
}
