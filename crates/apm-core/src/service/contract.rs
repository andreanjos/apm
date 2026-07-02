use crate::engine::{EngineEvent, InstallHandoffKind};

use super::{
    AutomaticResumePolicy, ExplicitRetryPolicy, HttpMethod, LocalServiceContract,
    OperationControlPolicy, OperationKind, OperationRecoveryPolicy, OperationState,
    PrivilegedInstallDesign, PrivilegedInstallDesignStatus, PrivilegedInstallExecution,
    PrivilegedInstallHelperDesign, PrivilegedInstallHelperStrategy, PrivilegedInstallPolicy,
    PrivilegedInstallPrerequisite, PrivilegedInstallPrerequisiteId,
    PrivilegedInstallPrerequisiteStatus, PrivilegedInstallRollbackDesign,
    PrivilegedInstallRollbackStrategy, ServiceBind, ServiceDaemonStatus, ServiceEndpoint,
    ServiceEndpointRuntime, ServiceEventStream, ServiceSecurity, LOOPBACK_TOKEN_HEADER,
    PRIVILEGED_HELPER_BUNDLE_IDENTIFIER, PRIVILEGED_HELPER_INSTALL_PATH, PRIVILEGED_HELPER_LABEL,
    PRIVILEGED_HELPER_LAUNCHD_PLIST_PATH, PRIVILEGED_HELPER_MACH_SERVICE_NAME,
    PRIVILEGED_HELPER_REQUIRED_SIGNING_IDENTITY, PRIVILEGED_INSTALL_RECEIPT_STORE_RELATIVE_PATH,
};

pub fn local_service_contract() -> LocalServiceContract {
    LocalServiceContract {
        schema_version: "2026-07-01-operation-controls".to_string(),
        service_name: "apm local service".to_string(),
        api_version: "v1alpha1".to_string(),
        daemon_status: ServiceDaemonStatus::ForegroundPreview,
        bind: ServiceBind {
            host: "127.0.0.1".to_string(),
            default_port: 4767,
            port_env: "APM_SERVE_PORT".to_string(),
        },
        security: ServiceSecurity {
            localhost_only: true,
            browser_cors: "desktop app origin only; deny arbitrary browsers by default".to_string(),
            privileged_install_policy: privileged_install_policy(),
            auth: format!("required for non-public endpoints via the {LOOPBACK_TOKEN_HEADER} header"),
        },
        operation_recovery_policy: operation_recovery_policy(),
        operation_control_policy: operation_control_policy(),
        endpoints: vec![
            available_endpoint(
                "health",
                HttpMethod::Get,
                "/v1/health",
                "Report service version, store paths, and daemon readiness.",
                None,
                "ServiceHealth",
            ),
            available_endpoint(
                "service.contract",
                HttpMethod::Get,
                "/v1/service/contract",
                "Return the versioned local service contract.",
                None,
                "LocalServiceContract",
            ),
            available_endpoint(
                "diagnostics.report",
                HttpMethod::Get,
                "/v1/diagnostics",
                "Run local doctor checks and return a typed diagnostics report.",
                None,
                "DiagnosticsReport",
            ),
            available_endpoint(
                "catalog.search",
                HttpMethod::Get,
                "/v1/packages",
                "Search the synced package catalog with the shared engine filters.",
                Some("PackageSearchRequest as query parameters"),
                "PackageSearchResult",
            ),
            available_endpoint(
                "catalog.details",
                HttpMethod::Get,
                "/v1/packages/{slug}",
                "Return one package detail record from the synced catalog.",
                None,
                "PackageDetailsResult",
            ),
            available_endpoint(
                "library.list",
                HttpMethod::Get,
                "/v1/library",
                "List packages tracked in local install state.",
                Some("InstalledPackagesRequest as query parameters"),
                "InstalledPackageSummary[]",
            ),
            available_endpoint(
                "library.updates",
                HttpMethod::Get,
                "/v1/library/updates",
                "Classify available updates using the shared update policy.",
                Some("AvailableUpdatesRequest as query parameters"),
                "AvailableUpdatesResult",
            ),
            available_endpoint(
                "library.scan",
                HttpMethod::Post,
                "/v1/library/scan",
                "Submit a plugin-directory scan and external-install reconciliation operation.",
                None,
                "OperationAccepted",
            ),
            available_endpoint(
                "registry.sync",
                HttpMethod::Post,
                "/v1/registry/sync",
                "Submit a registry sync operation.",
                None,
                "OperationAccepted",
            ),
            available_endpoint(
                "install.plan",
                HttpMethod::Post,
                "/v1/install/plan",
                "Build a non-mutating install plan for direct, manual, vendor, PKG, and App Store packages.",
                Some("InstallPlanRequest"),
                "InstallPlanResult",
            ),
            available_endpoint(
                "install.handoff",
                HttpMethod::Post,
                "/v1/install/handoff",
                "Resolve a manual, vendor-managed, PKG, or App Store install handoff target for desktop confirmation.",
                Some("InstallPlanRequest"),
                "InstallHandoffResult",
            ),
            available_endpoint(
                "install.url",
                HttpMethod::Post,
                "/v1/install/url",
                "Submit a direct download install operation.",
                Some("InstallPackageRequest"),
                "OperationAccepted",
            ),
            available_endpoint(
                "install.archive",
                HttpMethod::Post,
                "/v1/install/archive",
                "Submit an explicit local archive install operation.",
                Some("InstallPackageRequest"),
                "OperationAccepted",
            ),
            available_endpoint(
                "package.pin",
                HttpMethod::Post,
                "/v1/packages/{slug}/pin",
                "Set or clear package pin state.",
                Some("PackagePinBody"),
                "SetPackagePinResult",
            ),
            available_endpoint(
                "package.update",
                HttpMethod::Post,
                "/v1/packages/{slug}/update",
                "Submit a one-package update operation using shared update eligibility.",
                Some("PackageUpdateBody"),
                "OperationAccepted",
            ),
            available_endpoint(
                "package.remove",
                HttpMethod::Post,
                "/v1/packages/{slug}/remove",
                "Submit an apm-managed removal operation.",
                Some("PackageRemoveBody"),
                "OperationAccepted",
            ),
            available_endpoint(
                "model.list",
                HttpMethod::Get,
                "/v1/models",
                "List or search cached audio-AI model package manifests with GUI-renderable IO and parameter metadata.",
                None,
                "ModelListResult",
            ),
            available_endpoint(
                "model.catalog",
                HttpMethod::Get,
                "/v1/models/catalog",
                "List or search curated audio-AI model manifests from configured registry sources.",
                Some("ModelCatalogListRequest as query parameters"),
                "ModelCatalogListResult",
            ),
            available_endpoint(
                "model.catalog.cache",
                HttpMethod::Post,
                "/v1/models/catalog/{name}/{version}/cache",
                "Cache a curated audio-AI model manifest from configured registry sources into the local model store.",
                None,
                "ModelManifestCacheResult",
            ),
            available_endpoint(
                "model.store",
                HttpMethod::Get,
                "/v1/models/store",
                "Report the local audio-AI model store layout.",
                None,
                "ModelStoreLayout",
            ),
            available_endpoint(
                "model.store.init",
                HttpMethod::Post,
                "/v1/models/store/init",
                "Create missing local audio-AI model store directories.",
                None,
                "ModelStoreInitResult",
            ),
            available_endpoint(
                "model.manifest.validate",
                HttpMethod::Post,
                "/v1/models/manifest/validate",
                "Validate audio-AI model package manifest TOML and return GUI-safe summary metadata.",
                Some("ModelManifestValidationRequest"),
                "ModelManifestValidationResult",
            ),
            available_endpoint(
                "model.manifest.cache",
                HttpMethod::Post,
                "/v1/models/manifest/cache",
                "Validate and cache audio-AI model package manifest TOML in the local model store.",
                Some("ModelManifestCacheRequest"),
                "ModelManifestCacheResult",
            ),
            available_endpoint(
                "model.weights.pull",
                HttpMethod::Post,
                "/v1/models/{name}/{version}/weights/pull",
                "Submit a model weight pull operation for a cached model manifest.",
                None,
                "OperationAccepted",
            ),
            available_endpoint(
                "model.install",
                HttpMethod::Post,
                "/v1/models/{name}/{version}/install",
                "Submit a cached model install operation that verifies or pulls model weights and prepares runtime adapter metadata.",
                None,
                "OperationAccepted",
            ),
            available_endpoint(
                "model.run.plan",
                HttpMethod::Post,
                "/v1/models/{name}/{version}/run/plan",
                "Build a non-mutating run plan from prepared runtime adapter metadata without executing the model.",
                Some("ModelRunPlanRequest"),
                "ModelRunPlan",
            ),
            available_endpoint(
                "model.run",
                HttpMethod::Post,
                "/v1/models/{name}/{version}/run",
                "Submit a cached model run operation that validates the run plan and reports a typed execution blocker until an adapter runner exists.",
                Some("ModelRunPlanRequest"),
                "OperationAccepted",
            ),
            available_endpoint(
                "model.chain.plan",
                HttpMethod::Post,
                "/v1/models/chains/plan",
                "Build a non-mutating chain plan from prepared model runtime metadata and typed IO edges without executing the models.",
                Some("ModelChainPlanRequest"),
                "ModelChainPlan",
            ),
            available_endpoint(
                "model.remove",
                HttpMethod::Delete,
                "/v1/models/{name}/{version}",
                "Remove a cached audio-AI model package manifest and any unreferenced cached weights.",
                None,
                "ModelRemoveResult",
            ),
            available_endpoint(
                "operation.history",
                HttpMethod::Get,
                "/v1/operations",
                "List recent accepted operations from persisted service history, including request metadata for newly accepted operations.",
                None,
                "OperationStatus[]",
            ),
            available_endpoint(
                "operation.recovery",
                HttpMethod::Get,
                "/v1/operations/recovery",
                "Summarize restart-interrupted operations that can be retried from persisted request metadata.",
                None,
                "OperationRecoverySummary",
            ),
            available_endpoint(
                "operation.recovery.retry",
                HttpMethod::Post,
                "/v1/operations/recovery/retry",
                "Submit new operations for all currently retryable restart-interrupted records.",
                None,
                "OperationRecoveryRetryResult",
            ),
            available_endpoint(
                "operation.status",
                HttpMethod::Get,
                "/v1/operations/{operation_id}",
                "Inspect an accepted long-running operation and its persisted request metadata when available.",
                None,
                "OperationStatus",
            ),
            available_endpoint(
                "operation.cancel",
                HttpMethod::Post,
                "/v1/operations/{operation_id}/cancel",
                "Request cancellation for an accepted operation.",
                None,
                "OperationCancelResult",
            ),
            available_endpoint(
                "operation.retry",
                HttpMethod::Post,
                "/v1/operations/{operation_id}/retry",
                "Submit a new operation from a failed or canceled operation's persisted request metadata.",
                None,
                "OperationRetryResult",
            ),
        ],
        event_streams: vec![ServiceEventStream {
            id: "operation.events".to_string(),
            path: "/v1/operations/{operation_id}/events".to_string(),
            transport: "server-sent events".to_string(),
            events: vec![
                "RegistryEvent".to_string(),
                "ScanEvent".to_string(),
                "InstallEvent".to_string(),
                "RemoveEvent".to_string(),
                "ModelOperationEvent".to_string(),
            ],
            event_names: EngineEvent::SERIALIZED_NAMES
                .iter()
                .map(|event_name| event_name.to_string())
                .collect(),
            runtime: ServiceEndpointRuntime::Available,
        }],
        pending_runtime_work: vec![
            "configure macos-desktop-release signing/notarization secrets, run the manual desktop workflow, and complete release-channel artifact acceptance"
                .to_string(),
            "implement the signed privileged helper and receipt-backed rollback path before enabling apm-run PKG installers"
                .to_string(),
            "extend the current model-run cancellation/progress checkpoints into executable native MLX/Core ML and managed Python runtime-session checkpoints"
                .to_string(),
            "turn blocked model run operations into executable native MLX/Core ML adapters and managed Python runtime sessions"
                .to_string(),
        ],
    }
}

pub fn privileged_install_policy() -> PrivilegedInstallPolicy {
    PrivilegedInstallPolicy {
        execution: PrivilegedInstallExecution::ExternalHandoffOnly,
        handoff_kind: InstallHandoffKind::PrivilegedInstaller,
        requires_user_confirmation: true,
        runs_pkg_installers: false,
        design: privileged_install_design(),
        prerequisites: privileged_install_prerequisites(),
        message:
            "PKG packages are exposed only as explicit external handoffs; apm opens the vendor target after confirmation and does not run installer packages itself."
                .to_string(),
    }
}

fn privileged_install_design() -> PrivilegedInstallDesign {
    PrivilegedInstallDesign {
        helper_strategy: PrivilegedInstallHelperStrategy::SignedHelperDeferred,
        helper: privileged_install_helper_design(),
        rollback_strategy: PrivilegedInstallRollbackStrategy::ReceiptBackedUninstallDeferred,
        rollback: privileged_install_rollback_design(),
        execution_gate:
            "Keep runs_pkg_installers false until a signed helper, explicit consent, package verification, audit trail, and receipt-backed rollback are implemented."
                .to_string(),
    }
}

fn privileged_install_helper_design() -> PrivilegedInstallHelperDesign {
    PrivilegedInstallHelperDesign {
        status: PrivilegedInstallDesignStatus::Designed,
        label: PRIVILEGED_HELPER_LABEL.to_string(),
        bundle_identifier: PRIVILEGED_HELPER_BUNDLE_IDENTIFIER.to_string(),
        mach_service_name: PRIVILEGED_HELPER_MACH_SERVICE_NAME.to_string(),
        install_path: PRIVILEGED_HELPER_INSTALL_PATH.to_string(),
        launchd_plist_path: PRIVILEGED_HELPER_LAUNCHD_PLIST_PATH.to_string(),
        required_signing_identity: PRIVILEGED_HELPER_REQUIRED_SIGNING_IDENTITY.to_string(),
        requires_authorization: true,
    }
}

fn privileged_install_rollback_design() -> PrivilegedInstallRollbackDesign {
    PrivilegedInstallRollbackDesign {
        status: PrivilegedInstallDesignStatus::Designed,
        receipt_store_relative_path: PRIVILEGED_INSTALL_RECEIPT_STORE_RELATIVE_PATH.to_string(),
        receipt_required_before_mutation: true,
        preflight_snapshot_required: true,
        uninstall_requires_receipt: true,
        message:
            "Before helper-run PKG execution can mutate disk, apm must persist a package receipt and preflight snapshot so failed installs and explicit uninstalls have a rollback target."
                .to_string(),
    }
}

fn privileged_install_prerequisites() -> Vec<PrivilegedInstallPrerequisite> {
    use PrivilegedInstallPrerequisiteId as Id;
    use PrivilegedInstallPrerequisiteStatus as Status;

    vec![
        PrivilegedInstallPrerequisite {
            id: Id::HelperOrEscalationDesign,
            status: Status::Designed,
            message:
                "Use a signed privileged helper as the future PKG execution boundary; keep current builds on external handoff until that helper is implemented and reviewed."
                    .to_string(),
        },
        PrivilegedInstallPrerequisite {
            id: Id::ExplicitUserConsent,
            status: Status::Required,
            message:
                "Require an explicit per-install confirmation before any privileged installer execution."
                    .to_string(),
        },
        PrivilegedInstallPrerequisite {
            id: Id::PackageVerification,
            status: Status::Required,
            message:
                "Verify the downloaded package against registry metadata before privileged execution."
                    .to_string(),
        },
        PrivilegedInstallPrerequisite {
            id: Id::AuditTrail,
            status: Status::Required,
            message:
                "Record the requested package, source, checksum, and privileged action outcome in operation history."
                    .to_string(),
        },
        PrivilegedInstallPrerequisite {
            id: Id::RollbackPlan,
            status: Status::Designed,
            message:
                "Record helper-installed package receipts before enabling execution so failed installs and explicit uninstalls have a rollback target."
                    .to_string(),
        },
    ]
}

pub fn operation_recovery_policy() -> OperationRecoveryPolicy {
    OperationRecoveryPolicy {
        automatic_resume: AutomaticResumePolicy::Disabled,
        explicit_retry: ExplicitRetryPolicy::RequestMetadataRequired,
        retry_all_ready_recovery_candidates: true,
        consumes_request_metadata_after_recovery_retry: true,
        message:
            "apm does not automatically resume interrupted operations; users must explicitly retry request-backed terminal or restart-interrupted operations."
                .to_string(),
    }
}

pub fn operation_control_policy() -> OperationControlPolicy {
    OperationControlPolicy {
        cancel_endpoint_id: "operation.cancel".to_string(),
        retry_endpoint_id: "operation.retry".to_string(),
        recovery_retry_endpoint_id: "operation.recovery.retry".to_string(),
        progress_event_stream_id: "operation.events".to_string(),
        queued_cancellation: true,
        running_cancellation_state: OperationState::CancelRequested,
        cooperative_cancellation_kinds: OperationKind::ALL.to_vec(),
        progress_event_kinds: OperationKind::ALL.to_vec(),
        message:
            "Operations expose cancel, retry, recovery retry, and server-sent progress controls; executable model adapters still need deeper runtime-session checkpoints."
                .to_string(),
    }
}

fn available_endpoint(
    id: &str,
    method: HttpMethod,
    path: &str,
    summary: &str,
    request: Option<&str>,
    response: &str,
) -> ServiceEndpoint {
    endpoint(
        id,
        method,
        path,
        summary,
        request,
        response,
        ServiceEndpointRuntime::Available,
    )
}

fn endpoint(
    id: &str,
    method: HttpMethod,
    path: &str,
    summary: &str,
    request: Option<&str>,
    response: &str,
    runtime: ServiceEndpointRuntime,
) -> ServiceEndpoint {
    ServiceEndpoint {
        id: id.to_string(),
        method,
        path: path.to_string(),
        summary: summary.to_string(),
        request: request.map(str::to_string),
        response: response.to_string(),
        runtime,
    }
}
