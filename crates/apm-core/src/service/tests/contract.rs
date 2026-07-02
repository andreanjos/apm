use std::collections::HashSet;

use crate::engine::{EngineEvent, InstallHandoffKind};

use super::*;

#[test]
fn contract_is_localhost_only_and_not_a_daemon_claim() {
    let contract = local_service_contract();

    assert_eq!(
        contract.daemon_status,
        ServiceDaemonStatus::ForegroundPreview
    );
    assert!(contract.security.localhost_only);
    assert_eq!(contract.bind.host, "127.0.0.1");
    assert!(!contract.pending_runtime_work.is_empty());
}

#[test]
fn contract_exposes_privileged_install_policy() {
    let contract = local_service_contract();
    let policy = &contract.security.privileged_install_policy;

    assert_eq!(
        policy.execution,
        PrivilegedInstallExecution::ExternalHandoffOnly
    );
    assert_eq!(policy.handoff_kind, InstallHandoffKind::PrivilegedInstaller);
    assert!(policy.requires_user_confirmation);
    assert!(!policy.runs_pkg_installers);
    assert_eq!(
        policy.design.helper_strategy,
        PrivilegedInstallHelperStrategy::SignedHelperDeferred
    );
    assert_eq!(
        policy.design.helper.status,
        PrivilegedInstallDesignStatus::Designed
    );
    assert_eq!(policy.design.helper.bundle_identifier, "com.apm.pkg-helper");
    assert_eq!(policy.design.helper.mach_service_name, "com.apm.pkg-helper");
    assert_eq!(
        policy.design.helper.install_path,
        "/Library/PrivilegedHelperTools/com.apm.pkg-helper"
    );
    assert_eq!(
        policy.design.helper.launchd_plist_path,
        "/Library/LaunchDaemons/com.apm.pkg-helper.plist"
    );
    assert_eq!(
        policy.design.helper.required_signing_identity,
        "Developer ID Application"
    );
    assert!(policy.design.helper.requires_authorization);
    assert_eq!(
        policy.design.rollback_strategy,
        PrivilegedInstallRollbackStrategy::ReceiptBackedUninstallDeferred
    );
    assert_eq!(
        policy.design.rollback.status,
        PrivilegedInstallDesignStatus::Designed
    );
    assert_eq!(
        policy.design.rollback.receipt_store_relative_path,
        "service/privileged-install-receipts.json"
    );
    assert!(policy.design.rollback.receipt_required_before_mutation);
    assert!(policy.design.rollback.preflight_snapshot_required);
    assert!(policy.design.rollback.uninstall_requires_receipt);
    assert!(policy
        .design
        .rollback
        .message
        .contains("preflight snapshot"));
    assert!(policy
        .design
        .execution_gate
        .contains("runs_pkg_installers false"));
    assert_eq!(policy.prerequisites.len(), 5);
    assert_eq!(
        privileged_prerequisite(
            policy,
            PrivilegedInstallPrerequisiteId::HelperOrEscalationDesign
        )
        .status,
        PrivilegedInstallPrerequisiteStatus::Designed
    );
    assert_eq!(
        privileged_prerequisite(policy, PrivilegedInstallPrerequisiteId::ExplicitUserConsent)
            .status,
        PrivilegedInstallPrerequisiteStatus::Required
    );
    assert_eq!(
        privileged_prerequisite(policy, PrivilegedInstallPrerequisiteId::PackageVerification)
            .status,
        PrivilegedInstallPrerequisiteStatus::Required
    );
    assert_eq!(
        privileged_prerequisite(policy, PrivilegedInstallPrerequisiteId::AuditTrail).status,
        PrivilegedInstallPrerequisiteStatus::Required
    );
    assert_eq!(
        privileged_prerequisite(policy, PrivilegedInstallPrerequisiteId::RollbackPlan).status,
        PrivilegedInstallPrerequisiteStatus::Designed
    );
    assert!(privileged_prerequisite(
        policy,
        PrivilegedInstallPrerequisiteId::HelperOrEscalationDesign
    )
    .message
    .contains("future PKG execution boundary"));
    assert!(policy.message.contains("does not run installer packages"));
}

fn privileged_prerequisite(
    policy: &PrivilegedInstallPolicy,
    id: PrivilegedInstallPrerequisiteId,
) -> &PrivilegedInstallPrerequisite {
    policy
        .prerequisites
        .iter()
        .find(|prerequisite| prerequisite.id == id)
        .expect("privileged install prerequisite should exist")
}

#[test]
fn contract_exposes_operation_recovery_policy() {
    let contract = local_service_contract();
    let policy = &contract.operation_recovery_policy;

    assert_eq!(policy.automatic_resume, AutomaticResumePolicy::Disabled);
    assert_eq!(
        policy.explicit_retry,
        ExplicitRetryPolicy::RequestMetadataRequired
    );
    assert!(policy.retry_all_ready_recovery_candidates);
    assert!(policy.consumes_request_metadata_after_recovery_retry);
    assert!(policy.message.contains("does not automatically resume"));
}

#[test]
fn contract_exposes_operation_control_policy() {
    let contract = local_service_contract();
    let policy = &contract.operation_control_policy;

    assert_eq!(policy.cancel_endpoint_id, "operation.cancel");
    assert_eq!(policy.retry_endpoint_id, "operation.retry");
    assert_eq!(
        policy.recovery_retry_endpoint_id,
        "operation.recovery.retry"
    );
    assert_eq!(policy.progress_event_stream_id, "operation.events");
    assert!(policy.queued_cancellation);
    assert_eq!(
        policy.running_cancellation_state,
        OperationState::CancelRequested
    );
    assert_eq!(
        policy.cooperative_cancellation_kinds,
        OperationKind::ALL.to_vec()
    );
    assert_eq!(
        policy.progress_event_kinds,
        policy.cooperative_cancellation_kinds
    );
    assert!(policy.message.contains("server-sent progress"));
}

#[test]
fn contract_pending_work_tracks_release_and_runtime_gaps() {
    let contract = local_service_contract();
    let pending = contract.pending_runtime_work.join("\n");

    assert!(pending.contains("release-channel artifact acceptance"));
    assert!(pending.contains("signed privileged helper"));
    assert!(pending.contains("runtime-session checkpoints"));
    assert!(pending.contains("native MLX/Core ML"));
    assert!(!pending.contains("automatic resume policy"));
}

#[test]
fn contract_operation_stream_declares_scan_and_model_events() {
    let contract = local_service_contract();
    let stream = contract
        .event_streams
        .iter()
        .find(|stream| stream.id == "operation.events")
        .expect("operation event stream");

    assert!(stream.events.contains(&"ScanEvent".to_string()));
    assert!(stream.events.contains(&"ModelOperationEvent".to_string()));
    assert_eq!(
        stream.event_names,
        EngineEvent::SERIALIZED_NAMES
            .iter()
            .map(|event_name| event_name.to_string())
            .collect::<Vec<_>>()
    );
}

#[test]
fn contract_paths_are_versioned_and_unique() {
    let contract = local_service_contract();
    let mut ids = HashSet::new();
    let mut paths = HashSet::new();

    for endpoint in &contract.endpoints {
        assert!(ids.insert(&endpoint.id), "duplicate endpoint id");
        assert!(paths.insert(&endpoint.path), "duplicate endpoint path");
        assert!(
            endpoint.path.starts_with("/v1/"),
            "endpoint path is not versioned: {}",
            endpoint.path
        );
    }
}

#[test]
fn contract_covers_desktop_foundation_surfaces() {
    let contract = local_service_contract();
    let ids: HashSet<&str> = contract
        .endpoints
        .iter()
        .map(|endpoint| endpoint.id.as_str())
        .collect();

    for expected in [
        "health",
        "service.contract",
        "catalog.search",
        "catalog.details",
        "library.list",
        "library.updates",
        "library.scan",
        "install.plan",
        "install.handoff",
        "install.url",
        "install.archive",
        "package.pin",
        "package.update",
        "package.remove",
        "model.list",
        "model.catalog",
        "model.catalog.cache",
        "model.store",
        "model.store.init",
        "model.manifest.validate",
        "model.manifest.cache",
        "model.weights.pull",
        "model.install",
        "model.run.plan",
        "model.run",
        "model.chain.plan",
        "model.remove",
        "operation.cancel",
        "operation.retry",
        "operation.recovery",
    ] {
        assert!(ids.contains(expected), "missing endpoint: {expected}");
    }
}

#[test]
fn contract_marks_library_scan_as_available_operation() {
    let contract = local_service_contract();
    let endpoint = contract
        .endpoints
        .iter()
        .find(|endpoint| endpoint.id == "library.scan")
        .expect("library.scan endpoint should exist");

    assert_eq!(endpoint.runtime, ServiceEndpointRuntime::Available);
    assert_eq!(endpoint.request.as_deref(), None);
    assert_eq!(endpoint.response, "OperationAccepted");
}

#[test]
fn contract_marks_seeded_routes_available() {
    let contract = local_service_contract();

    for endpoint in &contract.endpoints {
        assert_eq!(
            endpoint.runtime,
            ServiceEndpointRuntime::Available,
            "seeded endpoint should be available: {}",
            endpoint.id
        );
    }
}

#[test]
fn contract_marks_install_handoff_as_available() {
    let contract = local_service_contract();
    let endpoint = contract
        .endpoints
        .iter()
        .find(|endpoint| endpoint.id == "install.handoff")
        .expect("install.handoff endpoint should exist");

    assert_eq!(endpoint.runtime, ServiceEndpointRuntime::Available);
    assert_eq!(endpoint.request.as_deref(), Some("InstallPlanRequest"));
}

#[test]
fn contract_marks_package_pin_as_available() {
    let contract = local_service_contract();
    let endpoint = contract
        .endpoints
        .iter()
        .find(|endpoint| endpoint.id == "package.pin")
        .expect("package.pin endpoint should exist");

    assert_eq!(endpoint.runtime, ServiceEndpointRuntime::Available);
    assert_eq!(endpoint.request.as_deref(), Some("PackagePinBody"));
}

#[test]
fn contract_marks_package_update_as_available_operation() {
    let contract = local_service_contract();
    let endpoint = contract
        .endpoints
        .iter()
        .find(|endpoint| endpoint.id == "package.update")
        .expect("package.update endpoint should exist");

    assert_eq!(endpoint.runtime, ServiceEndpointRuntime::Available);
    assert_eq!(endpoint.request.as_deref(), Some("PackageUpdateBody"));
    assert_eq!(endpoint.response, "OperationAccepted");
}

#[test]
fn contract_marks_archive_install_as_available_operation() {
    let contract = local_service_contract();
    let endpoint = contract
        .endpoints
        .iter()
        .find(|endpoint| endpoint.id == "install.archive")
        .expect("install.archive endpoint should exist");

    assert_eq!(endpoint.runtime, ServiceEndpointRuntime::Available);
    assert_eq!(endpoint.request.as_deref(), Some("InstallPackageRequest"));
    assert_eq!(endpoint.response, "OperationAccepted");
}

#[test]
fn contract_marks_url_install_as_available_operation() {
    let contract = local_service_contract();
    let endpoint = contract
        .endpoints
        .iter()
        .find(|endpoint| endpoint.id == "install.url")
        .expect("install.url endpoint should exist");

    assert_eq!(endpoint.runtime, ServiceEndpointRuntime::Available);
    assert_eq!(endpoint.request.as_deref(), Some("InstallPackageRequest"));
    assert_eq!(endpoint.response, "OperationAccepted");
}

#[test]
fn contract_marks_package_remove_as_available_operation() {
    let contract = local_service_contract();
    let endpoint = contract
        .endpoints
        .iter()
        .find(|endpoint| endpoint.id == "package.remove")
        .expect("package.remove endpoint should exist");

    assert_eq!(endpoint.runtime, ServiceEndpointRuntime::Available);
    assert_eq!(endpoint.request.as_deref(), Some("PackageRemoveBody"));
    assert_eq!(endpoint.response, "OperationAccepted");
}

#[test]
fn contract_marks_operation_cancel_as_available_operation_control() {
    let contract = local_service_contract();
    let endpoint = contract
        .endpoints
        .iter()
        .find(|endpoint| endpoint.id == "operation.cancel")
        .expect("operation.cancel endpoint should exist");

    assert_eq!(endpoint.runtime, ServiceEndpointRuntime::Available);
    assert_eq!(endpoint.method, HttpMethod::Post);
    assert_eq!(endpoint.request, None);
    assert_eq!(endpoint.response, "OperationCancelResult");
}

#[test]
fn contract_marks_operation_retry_as_available_operation_control() {
    let contract = local_service_contract();
    let endpoint = contract
        .endpoints
        .iter()
        .find(|endpoint| endpoint.id == "operation.retry")
        .expect("operation.retry endpoint should exist");

    assert_eq!(endpoint.runtime, ServiceEndpointRuntime::Available);
    assert_eq!(endpoint.method, HttpMethod::Post);
    assert_eq!(endpoint.request, None);
    assert_eq!(endpoint.response, "OperationRetryResult");
}

#[test]
fn contract_marks_operation_recovery_as_available_operation_control() {
    let contract = local_service_contract();
    let endpoint = contract
        .endpoints
        .iter()
        .find(|endpoint| endpoint.id == "operation.recovery")
        .expect("operation.recovery endpoint should exist");

    assert_eq!(endpoint.runtime, ServiceEndpointRuntime::Available);
    assert_eq!(endpoint.method, HttpMethod::Get);
    assert_eq!(endpoint.request, None);
    assert_eq!(endpoint.response, "OperationRecoverySummary");
}

#[test]
fn contract_marks_model_listing_as_available() {
    let contract = local_service_contract();
    let endpoint = contract
        .endpoints
        .iter()
        .find(|endpoint| endpoint.id == "model.list")
        .expect("model listing endpoint should exist");

    assert_eq!(endpoint.runtime, ServiceEndpointRuntime::Available);
    assert_eq!(endpoint.method, HttpMethod::Get);
    assert_eq!(endpoint.request, None);
    assert_eq!(endpoint.response, "ModelListResult");
}

#[test]
fn contract_marks_model_catalog_as_available() {
    let contract = local_service_contract();
    let endpoint = contract
        .endpoints
        .iter()
        .find(|endpoint| endpoint.id == "model.catalog")
        .expect("model catalog endpoint should exist");

    assert_eq!(endpoint.runtime, ServiceEndpointRuntime::Available);
    assert_eq!(endpoint.method, HttpMethod::Get);
    assert_eq!(
        endpoint.request.as_deref(),
        Some("ModelCatalogListRequest as query parameters")
    );
    assert_eq!(endpoint.response, "ModelCatalogListResult");
}

#[test]
fn contract_marks_model_catalog_cache_as_available() {
    let contract = local_service_contract();
    let endpoint = contract
        .endpoints
        .iter()
        .find(|endpoint| endpoint.id == "model.catalog.cache")
        .expect("model catalog cache endpoint should exist");

    assert_eq!(endpoint.runtime, ServiceEndpointRuntime::Available);
    assert_eq!(endpoint.method, HttpMethod::Post);
    assert_eq!(endpoint.request, None);
    assert_eq!(endpoint.response, "ModelManifestCacheResult");
}

#[test]
fn contract_marks_model_manifest_validation_as_available() {
    let contract = local_service_contract();
    let endpoint = contract
        .endpoints
        .iter()
        .find(|endpoint| endpoint.id == "model.manifest.validate")
        .expect("model manifest validation endpoint should exist");

    assert_eq!(endpoint.runtime, ServiceEndpointRuntime::Available);
    assert_eq!(endpoint.method, HttpMethod::Post);
    assert_eq!(
        endpoint.request.as_deref(),
        Some("ModelManifestValidationRequest")
    );
    assert_eq!(endpoint.response, "ModelManifestValidationResult");
}

#[test]
fn contract_marks_model_manifest_cache_as_available() {
    let contract = local_service_contract();
    let endpoint = contract
        .endpoints
        .iter()
        .find(|endpoint| endpoint.id == "model.manifest.cache")
        .expect("model manifest cache endpoint should exist");

    assert_eq!(endpoint.runtime, ServiceEndpointRuntime::Available);
    assert_eq!(endpoint.method, HttpMethod::Post);
    assert_eq!(
        endpoint.request.as_deref(),
        Some("ModelManifestCacheRequest")
    );
    assert_eq!(endpoint.response, "ModelManifestCacheResult");
}

#[test]
fn contract_marks_model_weight_pull_as_available_operation() {
    let contract = local_service_contract();
    let endpoint = contract
        .endpoints
        .iter()
        .find(|endpoint| endpoint.id == "model.weights.pull")
        .expect("model weight pull endpoint should exist");

    assert_eq!(endpoint.runtime, ServiceEndpointRuntime::Available);
    assert_eq!(endpoint.method, HttpMethod::Post);
    assert_eq!(endpoint.request, None);
    assert_eq!(endpoint.response, "OperationAccepted");
}

#[test]
fn contract_marks_model_install_as_available_operation() {
    let contract = local_service_contract();
    let endpoint = contract
        .endpoints
        .iter()
        .find(|endpoint| endpoint.id == "model.install")
        .expect("model install endpoint should exist");

    assert_eq!(endpoint.runtime, ServiceEndpointRuntime::Available);
    assert_eq!(endpoint.method, HttpMethod::Post);
    assert_eq!(endpoint.request, None);
    assert_eq!(endpoint.response, "OperationAccepted");
}

#[test]
fn contract_marks_model_run_plan_as_available() {
    let contract = local_service_contract();
    let endpoint = contract
        .endpoints
        .iter()
        .find(|endpoint| endpoint.id == "model.run.plan")
        .expect("model run plan endpoint should exist");

    assert_eq!(endpoint.runtime, ServiceEndpointRuntime::Available);
    assert_eq!(endpoint.method, HttpMethod::Post);
    assert_eq!(endpoint.request.as_deref(), Some("ModelRunPlanRequest"));
    assert_eq!(endpoint.response, "ModelRunPlan");
}

#[test]
fn contract_marks_model_run_as_available_operation() {
    let contract = local_service_contract();
    let endpoint = contract
        .endpoints
        .iter()
        .find(|endpoint| endpoint.id == "model.run")
        .expect("model run endpoint should exist");

    assert_eq!(endpoint.runtime, ServiceEndpointRuntime::Available);
    assert_eq!(endpoint.method, HttpMethod::Post);
    assert_eq!(endpoint.request.as_deref(), Some("ModelRunPlanRequest"));
    assert_eq!(endpoint.response, "OperationAccepted");
}

#[test]
fn contract_marks_model_remove_as_available() {
    let contract = local_service_contract();
    let endpoint = contract
        .endpoints
        .iter()
        .find(|endpoint| endpoint.id == "model.remove")
        .expect("model remove endpoint should exist");

    assert_eq!(endpoint.runtime, ServiceEndpointRuntime::Available);
    assert_eq!(endpoint.method, HttpMethod::Delete);
    assert_eq!(endpoint.request, None);
    assert_eq!(endpoint.response, "ModelRemoveResult");
}

#[test]
fn contract_marks_event_streams_as_available() {
    let contract = local_service_contract();

    assert!(contract
        .event_streams
        .iter()
        .all(|stream| stream.runtime == ServiceEndpointRuntime::Available));
}
