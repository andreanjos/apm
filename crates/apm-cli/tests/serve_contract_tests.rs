mod support;

use support::{command, CliTestEnv};

#[test]
fn serve_contract_help_exits_successfully() {
    let env = CliTestEnv::new();
    let output = command(&env)
        .args(["serve", "contract", "--help"])
        .output()
        .expect("run apm serve contract --help");

    assert!(
        output.status.success(),
        "apm serve contract --help should exit 0; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn serve_contract_json_outputs_versioned_localhost_contract() {
    let env = CliTestEnv::new();
    let output = command(&env)
        .args(["--json", "serve", "contract"])
        .output()
        .expect("run apm --json serve contract");

    assert!(
        output.status.success(),
        "apm --json serve contract should exit 0; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let value: serde_json::Value =
        serde_json::from_str(stdout.trim()).expect("contract should be JSON");

    assert_eq!(value["api_version"], "v1alpha1");
    assert_eq!(value["schema_version"], "2026-07-01-operation-controls");
    assert_eq!(value["daemon_status"], "foreground_preview");
    assert_eq!(value["bind"]["host"], "127.0.0.1");
    assert_eq!(value["security"]["localhost_only"], true);
    assert!(value["security"].get("privileged_installs").is_none());
    assert_eq!(
        value["security"]["privileged_install_policy"]["execution"],
        "external_handoff_only"
    );
    assert_eq!(
        value["security"]["privileged_install_policy"]["handoff_kind"],
        "privileged_installer"
    );
    assert_eq!(
        value["security"]["privileged_install_policy"]["requires_user_confirmation"],
        true
    );
    assert_eq!(
        value["security"]["privileged_install_policy"]["runs_pkg_installers"],
        false
    );
    assert_eq!(
        value["security"]["privileged_install_policy"]["design"]["helper_strategy"],
        "signed_helper_deferred"
    );
    assert_eq!(
        value["security"]["privileged_install_policy"]["design"]["helper"]["status"],
        "designed"
    );
    assert_eq!(
        value["security"]["privileged_install_policy"]["design"]["helper"]["bundle_identifier"],
        "com.apm.pkg-helper"
    );
    assert_eq!(
        value["security"]["privileged_install_policy"]["design"]["helper"]["mach_service_name"],
        "com.apm.pkg-helper"
    );
    assert_eq!(
        value["security"]["privileged_install_policy"]["design"]["helper"]["install_path"],
        "/Library/PrivilegedHelperTools/com.apm.pkg-helper"
    );
    assert_eq!(
        value["security"]["privileged_install_policy"]["design"]["helper"]["launchd_plist_path"],
        "/Library/LaunchDaemons/com.apm.pkg-helper.plist"
    );
    assert_eq!(
        value["security"]["privileged_install_policy"]["design"]["helper"]
            ["required_signing_identity"],
        "Developer ID Application"
    );
    assert_eq!(
        value["security"]["privileged_install_policy"]["design"]["helper"]
            ["requires_authorization"],
        true
    );
    assert_eq!(
        value["security"]["privileged_install_policy"]["design"]["rollback_strategy"],
        "receipt_backed_uninstall_deferred"
    );
    assert_eq!(
        value["security"]["privileged_install_policy"]["design"]["rollback"]["status"],
        "designed"
    );
    assert_eq!(
        value["security"]["privileged_install_policy"]["design"]["rollback"]
            ["receipt_store_relative_path"],
        "service/privileged-install-receipts.json"
    );
    assert_eq!(
        value["security"]["privileged_install_policy"]["design"]["rollback"]
            ["receipt_required_before_mutation"],
        true
    );
    assert_eq!(
        value["security"]["privileged_install_policy"]["design"]["rollback"]
            ["preflight_snapshot_required"],
        true
    );
    assert_eq!(
        value["security"]["privileged_install_policy"]["design"]["rollback"]
            ["uninstall_requires_receipt"],
        true
    );
    assert!(
        value["security"]["privileged_install_policy"]["design"]["rollback"]["message"]
            .as_str()
            .expect("rollback message should be a string")
            .contains("preflight snapshot")
    );
    assert!(
        value["security"]["privileged_install_policy"]["design"]["execution_gate"]
            .as_str()
            .expect("execution gate should be a string")
            .contains("runs_pkg_installers false")
    );
    assert_privileged_prerequisite(
        &value,
        "helper_or_escalation_design",
        "designed",
        "future PKG execution boundary",
    );
    assert_privileged_prerequisite(&value, "explicit_user_consent", "required", "confirmation");
    assert_privileged_prerequisite(&value, "package_verification", "required", "Verify");
    assert_privileged_prerequisite(&value, "audit_trail", "required", "outcome");
    assert_privileged_prerequisite(&value, "rollback_plan", "designed", "rollback");
    assert_eq!(
        value["operation_recovery_policy"]["automatic_resume"],
        "disabled"
    );
    assert_eq!(
        value["operation_recovery_policy"]["explicit_retry"],
        "request_metadata_required"
    );
    assert_eq!(
        value["operation_recovery_policy"]["retry_all_ready_recovery_candidates"],
        true
    );
    assert_eq!(
        value["operation_recovery_policy"]["consumes_request_metadata_after_recovery_retry"],
        true
    );
    assert_eq!(
        value["operation_control_policy"]["cancel_endpoint_id"],
        "operation.cancel"
    );
    assert_eq!(
        value["operation_control_policy"]["retry_endpoint_id"],
        "operation.retry"
    );
    assert_eq!(
        value["operation_control_policy"]["recovery_retry_endpoint_id"],
        "operation.recovery.retry"
    );
    assert_eq!(
        value["operation_control_policy"]["progress_event_stream_id"],
        "operation.events"
    );
    assert_eq!(
        value["operation_control_policy"]["queued_cancellation"],
        true
    );
    assert_eq!(
        value["operation_control_policy"]["running_cancellation_state"],
        "cancel_requested"
    );
    assert_operation_control_kind(&value, "cooperative_cancellation_kinds", "install_url");
    assert_operation_control_kind(&value, "cooperative_cancellation_kinds", "model_run");
    assert_operation_control_kind(&value, "progress_event_kinds", "model_weight_pull");
    assert_operation_control_kind(&value, "progress_event_kinds", "package_remove");

    let endpoints = value["endpoints"]
        .as_array()
        .expect("endpoints should be an array");
    assert!(endpoints.iter().any(|endpoint| {
        endpoint["id"] == "install.plan" && endpoint["path"] == "/v1/install/plan"
    }));
    assert!(endpoints.iter().any(|endpoint| {
        endpoint["id"] == "diagnostics.report"
            && endpoint["path"] == "/v1/diagnostics"
            && endpoint["response"] == "DiagnosticsReport"
            && endpoint["runtime"] == "available"
    }));
    assert!(endpoints.iter().any(|endpoint| {
        endpoint["id"] == "install.handoff"
            && endpoint["path"] == "/v1/install/handoff"
            && endpoint["request"] == "InstallPlanRequest"
            && endpoint["runtime"] == "available"
    }));
    assert!(endpoints.iter().any(|endpoint| {
        endpoint["id"] == "install.archive"
            && endpoint["path"] == "/v1/install/archive"
            && endpoint["request"] == "InstallPackageRequest"
            && endpoint["runtime"] == "available"
    }));
    assert!(endpoints.iter().any(|endpoint| {
        endpoint["id"] == "install.url"
            && endpoint["path"] == "/v1/install/url"
            && endpoint["request"] == "InstallPackageRequest"
            && endpoint["runtime"] == "available"
    }));
    assert!(endpoints.iter().any(|endpoint| {
        endpoint["id"] == "package.pin"
            && endpoint["path"] == "/v1/packages/{slug}/pin"
            && endpoint["request"] == "PackagePinBody"
            && endpoint["runtime"] == "available"
    }));
    assert!(endpoints.iter().any(|endpoint| {
        endpoint["id"] == "package.update"
            && endpoint["path"] == "/v1/packages/{slug}/update"
            && endpoint["request"] == "PackageUpdateBody"
            && endpoint["runtime"] == "available"
    }));
    assert!(endpoints.iter().any(|endpoint| {
        endpoint["id"] == "package.remove"
            && endpoint["path"] == "/v1/packages/{slug}/remove"
            && endpoint["request"] == "PackageRemoveBody"
            && endpoint["runtime"] == "available"
    }));
    assert!(endpoints.iter().any(|endpoint| {
        endpoint["id"] == "library.scan"
            && endpoint["path"] == "/v1/library/scan"
            && endpoint["request"].is_null()
            && endpoint["response"] == "OperationAccepted"
            && endpoint["runtime"] == "available"
    }));
    let streams = value["event_streams"]
        .as_array()
        .expect("event_streams should be an array");
    assert!(streams.iter().any(|stream| {
        stream["id"] == "operation.events"
            && stream["events"]
                .as_array()
                .expect("event stream events")
                .iter()
                .any(|event| event == "ScanEvent")
            && stream["event_names"]
                .as_array()
                .expect("event stream event names")
                .iter()
                .any(|event| event == "model_run_completed")
    }));
    assert!(endpoints.iter().any(|endpoint| {
        endpoint["id"] == "operation.cancel"
            && endpoint["path"] == "/v1/operations/{operation_id}/cancel"
            && endpoint["response"] == "OperationCancelResult"
            && endpoint["runtime"] == "available"
    }));
    assert!(endpoints.iter().any(|endpoint| {
        endpoint["id"] == "operation.retry"
            && endpoint["path"] == "/v1/operations/{operation_id}/retry"
            && endpoint["response"] == "OperationRetryResult"
            && endpoint["runtime"] == "available"
    }));
    assert!(endpoints.iter().any(|endpoint| {
        endpoint["id"] == "operation.history"
            && endpoint["path"] == "/v1/operations"
            && endpoint["response"] == "OperationStatus[]"
            && endpoint["runtime"] == "available"
    }));
    assert!(endpoints.iter().any(|endpoint| {
        endpoint["id"] == "operation.recovery"
            && endpoint["path"] == "/v1/operations/recovery"
            && endpoint["response"] == "OperationRecoverySummary"
            && endpoint["runtime"] == "available"
    }));
    assert!(endpoints.iter().any(|endpoint| {
        endpoint["id"] == "operation.recovery.retry"
            && endpoint["path"] == "/v1/operations/recovery/retry"
            && endpoint["response"] == "OperationRecoveryRetryResult"
            && endpoint["runtime"] == "available"
    }));
    assert!(endpoints.iter().any(|endpoint| {
        endpoint["id"] == "model.list"
            && endpoint["path"] == "/v1/models"
            && endpoint["response"] == "ModelListResult"
            && endpoint["runtime"] == "available"
    }));
    assert!(endpoints.iter().any(|endpoint| {
        endpoint["id"] == "model.catalog"
            && endpoint["path"] == "/v1/models/catalog"
            && endpoint["request"] == "ModelCatalogListRequest as query parameters"
            && endpoint["response"] == "ModelCatalogListResult"
            && endpoint["runtime"] == "available"
    }));
    assert!(endpoints.iter().any(|endpoint| {
        endpoint["id"] == "model.catalog.cache"
            && endpoint["path"] == "/v1/models/catalog/{name}/{version}/cache"
            && endpoint["request"].is_null()
            && endpoint["response"] == "ModelManifestCacheResult"
            && endpoint["runtime"] == "available"
    }));
    assert!(endpoints.iter().any(|endpoint| {
        endpoint["id"] == "model.store.init"
            && endpoint["path"] == "/v1/models/store/init"
            && endpoint["request"].is_null()
            && endpoint["response"] == "ModelStoreInitResult"
            && endpoint["runtime"] == "available"
    }));
    assert!(endpoints.iter().any(|endpoint| {
        endpoint["id"] == "model.manifest.validate"
            && endpoint["path"] == "/v1/models/manifest/validate"
            && endpoint["request"] == "ModelManifestValidationRequest"
            && endpoint["response"] == "ModelManifestValidationResult"
            && endpoint["runtime"] == "available"
    }));
    assert!(endpoints.iter().any(|endpoint| {
        endpoint["id"] == "model.manifest.cache"
            && endpoint["path"] == "/v1/models/manifest/cache"
            && endpoint["request"] == "ModelManifestCacheRequest"
            && endpoint["response"] == "ModelManifestCacheResult"
            && endpoint["runtime"] == "available"
    }));
    assert!(endpoints.iter().any(|endpoint| {
        endpoint["id"] == "model.weights.pull"
            && endpoint["path"] == "/v1/models/{name}/{version}/weights/pull"
            && endpoint["request"].is_null()
            && endpoint["response"] == "OperationAccepted"
            && endpoint["runtime"] == "available"
    }));
    assert!(endpoints.iter().any(|endpoint| {
        endpoint["id"] == "model.install"
            && endpoint["path"] == "/v1/models/{name}/{version}/install"
            && endpoint["request"].is_null()
            && endpoint["response"] == "OperationAccepted"
            && endpoint["runtime"] == "available"
    }));
    assert!(endpoints.iter().any(|endpoint| {
        endpoint["id"] == "model.run.plan"
            && endpoint["path"] == "/v1/models/{name}/{version}/run/plan"
            && endpoint["request"] == "ModelRunPlanRequest"
            && endpoint["response"] == "ModelRunPlan"
            && endpoint["runtime"] == "available"
    }));
    assert!(endpoints.iter().any(|endpoint| {
        endpoint["id"] == "model.run"
            && endpoint["path"] == "/v1/models/{name}/{version}/run"
            && endpoint["request"] == "ModelRunPlanRequest"
            && endpoint["response"] == "OperationAccepted"
            && endpoint["runtime"] == "available"
    }));
    assert!(endpoints.iter().any(|endpoint| {
        endpoint["id"] == "model.chain.plan"
            && endpoint["path"] == "/v1/models/chains/plan"
            && endpoint["request"] == "ModelChainPlanRequest"
            && endpoint["response"] == "ModelChainPlan"
            && endpoint["runtime"] == "available"
    }));
    assert!(endpoints.iter().any(|endpoint| {
        endpoint["id"] == "model.remove"
            && endpoint["method"] == "DELETE"
            && endpoint["path"] == "/v1/models/{name}/{version}"
            && endpoint["request"].is_null()
            && endpoint["response"] == "ModelRemoveResult"
            && endpoint["runtime"] == "available"
    }));
}

#[test]
fn serve_contract_human_output_shows_preview_status() {
    let env = CliTestEnv::new();
    let output = command(&env)
        .args(["serve", "contract"])
        .output()
        .expect("run apm serve contract");

    assert!(
        output.status.success(),
        "apm serve contract should exit 0; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("foreground preview"),
        "serve contract should show preview status, got: {stdout}"
    );
    assert!(
        stdout.contains("127.0.0.1:4767"),
        "serve contract should show the planned localhost bind, got: {stdout}"
    );
    assert!(
        stdout.contains("Operation recovery:")
            && stdout.contains("automatic resume disabled")
            && stdout.contains("request metadata required"),
        "serve contract should show the operation recovery policy, got: {stdout}"
    );
    assert!(
        stdout.contains("Privileged installs:")
            && stdout.contains("designed gates: helper/escalation design, rollback plan"),
        "serve contract should show privileged installer gates, got: {stdout}"
    );
    assert!(
        stdout.contains("Helper design:")
            && stdout.contains("com.apm.pkg-helper")
            && stdout.contains("service/privileged-install-receipts.json"),
        "serve contract should show privileged helper design, got: {stdout}"
    );
}

fn assert_operation_control_kind(value: &serde_json::Value, field: &str, kind: &str) {
    assert!(
        value["operation_control_policy"][field]
            .as_array()
            .expect("operation control kind list")
            .iter()
            .any(|candidate| candidate == kind),
        "operation_control_policy.{field} should include {kind}"
    );
}

fn assert_privileged_prerequisite(
    value: &serde_json::Value,
    id: &str,
    status: &str,
    message_fragment: &str,
) {
    let prerequisites = value["security"]["privileged_install_policy"]["prerequisites"]
        .as_array()
        .expect("privileged install prerequisites should be an array");
    let prerequisite = prerequisites
        .iter()
        .find(|candidate| candidate["id"] == id)
        .expect("privileged install prerequisite should exist");

    assert_eq!(prerequisite["status"], status);
    assert!(
        prerequisite["message"]
            .as_str()
            .expect("prerequisite message should be a string")
            .contains(message_fragment),
        "prerequisite {id} should describe {message_fragment}"
    );
}
