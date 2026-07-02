use std::fmt;
use std::path::Path;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::config::{Config, InstallScope};
use crate::engine::{
    EngineEvent, InstallHandoffKind, InstallPackageRequest, InstallPackageResult,
    RegistrySyncResult, RemovePackageResult, ScanPackagesResult, UpdatePackageResult,
};
pub use crate::model::{ModelChainPlan, ModelChainPlanRequest, ModelRunPlan, ModelRunPlanRequest};

use crate::model::{ModelInstallResult, ModelRunResult, ModelWeightPullResult};
use crate::registry::PluginFormat;

mod contract;
mod models;
mod privileged;

pub use contract::{local_service_contract, operation_recovery_policy, privileged_install_policy};
pub use models::{
    cache_model_catalog_manifest, cache_model_catalog_manifest_in_store, cache_model_manifest,
    cache_model_manifest_in_store, initialize_model_store, initialize_model_store_in_store,
    install_cached_model_package, install_cached_model_package_in_store,
    install_cached_model_package_in_store_with_cancellation,
    install_cached_model_package_with_cancellation, list_cached_models,
    list_cached_models_in_store, list_cached_models_matching, list_cached_models_matching_in_store,
    list_model_catalog, list_model_catalog_in_store, model_store_layout, plan_cached_model_chain,
    plan_cached_model_chain_in_store, plan_cached_model_run, plan_cached_model_run_in_store,
    pull_cached_model_weights, pull_cached_model_weights_in_store,
    pull_cached_model_weights_in_store_with_cancellation,
    pull_cached_model_weights_in_store_with_observer, pull_cached_model_weights_with_cancellation,
    pull_cached_model_weights_with_observer, remove_cached_model_package,
    remove_cached_model_package_in_store, run_cached_model, run_cached_model_in_store,
    run_cached_model_in_store_with_observer, run_cached_model_with_observer,
    validate_model_manifest, AvailableModelPackage, CachedModelPackage, ModelCatalogListRequest,
    ModelCatalogListResult, ModelListRequest, ModelListResult, ModelManifestCacheError,
    ModelManifestCacheRequest, ModelManifestCacheResult, ModelManifestSummary,
    ModelManifestValidationRequest, ModelManifestValidationResult, ModelParameterSummary,
    ModelStoreInitResult, ModelStoreLayout, ModelWeightsSummary,
};
pub use privileged::{
    privileged_install_receipt_store_path, PrivilegedInstallPreflightSnapshot,
    PrivilegedInstallReceipt, PrivilegedInstallReceiptStore, PRIVILEGED_HELPER_BUNDLE_IDENTIFIER,
    PRIVILEGED_HELPER_INSTALL_PATH, PRIVILEGED_HELPER_LABEL, PRIVILEGED_HELPER_LAUNCHD_PLIST_PATH,
    PRIVILEGED_HELPER_MACH_SERVICE_NAME, PRIVILEGED_HELPER_REQUIRED_SIGNING_IDENTITY,
    PRIVILEGED_INSTALL_RECEIPT_STORE_RELATIVE_PATH,
    PRIVILEGED_INSTALL_RECEIPT_STORE_SCHEMA_VERSION,
};

pub const LOOPBACK_TOKEN_HEADER: &str = "x-apm-token";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalServiceContract {
    pub schema_version: String,
    pub service_name: String,
    pub api_version: String,
    pub daemon_status: ServiceDaemonStatus,
    pub bind: ServiceBind,
    pub security: ServiceSecurity,
    pub operation_recovery_policy: OperationRecoveryPolicy,
    pub operation_control_policy: OperationControlPolicy,
    pub endpoints: Vec<ServiceEndpoint>,
    pub event_streams: Vec<ServiceEventStream>,
    pub pending_runtime_work: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceDaemonStatus {
    ForegroundPreview,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceBind {
    pub host: String,
    pub default_port: u16,
    pub port_env: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceSecurity {
    pub localhost_only: bool,
    pub browser_cors: String,
    pub privileged_install_policy: PrivilegedInstallPolicy,
    pub auth: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrivilegedInstallPolicy {
    pub execution: PrivilegedInstallExecution,
    pub handoff_kind: InstallHandoffKind,
    pub requires_user_confirmation: bool,
    pub runs_pkg_installers: bool,
    pub design: PrivilegedInstallDesign,
    pub prerequisites: Vec<PrivilegedInstallPrerequisite>,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrivilegedInstallExecution {
    ExternalHandoffOnly,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrivilegedInstallDesign {
    pub helper_strategy: PrivilegedInstallHelperStrategy,
    pub helper: PrivilegedInstallHelperDesign,
    pub rollback_strategy: PrivilegedInstallRollbackStrategy,
    pub rollback: PrivilegedInstallRollbackDesign,
    pub execution_gate: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrivilegedInstallHelperStrategy {
    SignedHelperDeferred,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrivilegedInstallHelperDesign {
    pub status: PrivilegedInstallDesignStatus,
    pub label: String,
    pub bundle_identifier: String,
    pub mach_service_name: String,
    pub install_path: String,
    pub launchd_plist_path: String,
    pub required_signing_identity: String,
    pub requires_authorization: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrivilegedInstallRollbackStrategy {
    ReceiptBackedUninstallDeferred,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrivilegedInstallRollbackDesign {
    pub status: PrivilegedInstallDesignStatus,
    pub receipt_store_relative_path: String,
    pub receipt_required_before_mutation: bool,
    pub preflight_snapshot_required: bool,
    pub uninstall_requires_receipt: bool,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrivilegedInstallDesignStatus {
    Designed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrivilegedInstallPrerequisite {
    pub id: PrivilegedInstallPrerequisiteId,
    pub status: PrivilegedInstallPrerequisiteStatus,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrivilegedInstallPrerequisiteId {
    HelperOrEscalationDesign,
    ExplicitUserConsent,
    PackageVerification,
    AuditTrail,
    RollbackPlan,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrivilegedInstallPrerequisiteStatus {
    Missing,
    Designed,
    Required,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperationRecoveryPolicy {
    pub automatic_resume: AutomaticResumePolicy,
    pub explicit_retry: ExplicitRetryPolicy,
    pub retry_all_ready_recovery_candidates: bool,
    pub consumes_request_metadata_after_recovery_retry: bool,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomaticResumePolicy {
    Disabled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExplicitRetryPolicy {
    RequestMetadataRequired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperationControlPolicy {
    pub cancel_endpoint_id: String,
    pub retry_endpoint_id: String,
    pub recovery_retry_endpoint_id: String,
    pub progress_event_stream_id: String,
    pub queued_cancellation: bool,
    pub running_cancellation_state: OperationState,
    pub cooperative_cancellation_kinds: Vec<OperationKind>,
    pub progress_event_kinds: Vec<OperationKind>,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceEndpoint {
    pub id: String,
    pub method: HttpMethod,
    pub path: String,
    pub summary: String,
    pub request: Option<String>,
    pub response: String,
    pub runtime: ServiceEndpointRuntime,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceEndpointRuntime {
    Available,
    Planned,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceEventStream {
    pub id: String,
    pub path: String,
    pub transport: String,
    pub events: Vec<String>,
    pub event_names: Vec<String>,
    pub runtime: ServiceEndpointRuntime,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum HttpMethod {
    Get,
    Post,
    Delete,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeBind {
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceHealth {
    pub status: String,
    pub service_name: String,
    pub api_version: String,
    pub daemon_status: ServiceDaemonStatus,
    pub bind: RuntimeBind,
    pub config_dir: String,
    pub data_dir: String,
    pub cache_dir: String,
    pub model_store: ModelStoreLayout,
    pub auth: ServiceAuthStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceAuthStatus {
    pub required: bool,
    pub header: String,
    pub token_file: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperationAccepted {
    pub operation_id: String,
    pub kind: OperationKind,
    pub status_url: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OperationStatus {
    pub operation_id: String,
    pub kind: OperationKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request: Option<OperationRequest>,
    pub state: OperationState,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    pub result: Option<OperationResult>,
    pub error: Option<String>,
    pub events: Vec<EngineEvent>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OperationRequest {
    RegistrySync,
    LibraryScan,
    InstallUrl {
        request: InstallPackageRequest,
    },
    InstallArchive {
        request: InstallPackageRequest,
    },
    PackageUpdate {
        slug: String,
        body: PackageUpdateBody,
    },
    PackageRemove {
        slug: String,
        body: PackageRemoveBody,
    },
    ModelWeightPull {
        name: String,
        version: String,
    },
    ModelInstall {
        name: String,
        version: String,
    },
    ModelRun {
        name: String,
        version: String,
        request: ModelRunPlanRequest,
    },
}

impl OperationRequest {
    pub fn kind(&self) -> OperationKind {
        match self {
            Self::RegistrySync => OperationKind::RegistrySync,
            Self::LibraryScan => OperationKind::LibraryScan,
            Self::InstallUrl { .. } => OperationKind::InstallUrl,
            Self::InstallArchive { .. } => OperationKind::InstallArchive,
            Self::PackageUpdate { .. } => OperationKind::PackageUpdate,
            Self::PackageRemove { .. } => OperationKind::PackageRemove,
            Self::ModelWeightPull { .. } => OperationKind::ModelWeightPull,
            Self::ModelInstall { .. } => OperationKind::ModelInstall,
            Self::ModelRun { .. } => OperationKind::ModelRun,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperationCancelResult {
    pub operation_id: String,
    pub state: OperationState,
    pub accepted: bool,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperationRetryResult {
    pub original_operation_id: String,
    pub operation: OperationAccepted,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperationRecoveryRetryResult {
    pub retried_count: usize,
    pub operations: Vec<OperationRetryResult>,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperationRecoverySummary {
    pub interrupted_count: usize,
    pub retryable_count: usize,
    pub candidates: Vec<OperationRecoveryCandidate>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperationRecoveryCandidate {
    pub operation_id: String,
    pub kind: OperationKind,
    pub created_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub retryable: bool,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackagePinBody {
    pub pinned: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageUpdateBody {
    #[serde(default)]
    pub format: Option<PluginFormat>,
    #[serde(default)]
    pub scope: Option<InstallScope>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageRemoveBody {
    #[serde(default)]
    pub dry_run: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationKind {
    RegistrySync,
    LibraryScan,
    InstallUrl,
    InstallArchive,
    PackageUpdate,
    PackageRemove,
    ModelWeightPull,
    ModelInstall,
    ModelRun,
}

impl OperationKind {
    pub const ALL: [Self; 9] = [
        Self::RegistrySync,
        Self::LibraryScan,
        Self::InstallUrl,
        Self::InstallArchive,
        Self::PackageUpdate,
        Self::PackageRemove,
        Self::ModelWeightPull,
        Self::ModelInstall,
        Self::ModelRun,
    ];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationState {
    Queued,
    Running,
    CancelRequested,
    Canceled,
    Succeeded,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "result", rename_all = "snake_case")]
pub enum OperationResult {
    RegistrySync(RegistrySyncResult),
    LibraryScan(ScanPackagesResult),
    InstallPackage(InstallPackageResult),
    UpdatePackage(UpdatePackageResult),
    RemovePackage(RemovePackageResult),
    ModelWeightPull(ModelWeightPullResult),
    ModelInstall(ModelInstallResult),
    ModelRun(ModelRunResult),
}

impl fmt::Display for HttpMethod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Get => f.write_str("GET"),
            Self::Post => f.write_str("POST"),
            Self::Delete => f.write_str("DELETE"),
        }
    }
}

pub fn service_health(config: &Config, bind: RuntimeBind) -> ServiceHealth {
    let contract = local_service_contract();
    ServiceHealth {
        status: "ok".to_string(),
        service_name: contract.service_name,
        api_version: contract.api_version,
        daemon_status: contract.daemon_status,
        bind,
        config_dir: path_string(&crate::config::config_dir()),
        data_dir: path_string(&config.resolved_data_dir()),
        cache_dir: path_string(&config.resolved_cache_dir()),
        model_store: model_store_layout(),
        auth: service_auth_status(config),
    }
}

pub fn service_auth_status(config: &Config) -> ServiceAuthStatus {
    ServiceAuthStatus {
        required: true,
        header: LOOPBACK_TOKEN_HEADER.to_string(),
        token_file: path_string(&loopback_token_file(config)),
    }
}

pub fn loopback_token_file(config: &Config) -> std::path::PathBuf {
    config.resolved_data_dir().join("service/token.json")
}

pub fn operation_accepted(operation_id: String, kind: OperationKind) -> OperationAccepted {
    OperationAccepted {
        status_url: format!("/v1/operations/{operation_id}"),
        operation_id,
        kind,
    }
}

fn path_string(path: &Path) -> String {
    path.display().to_string()
}

#[cfg(test)]
mod tests;
