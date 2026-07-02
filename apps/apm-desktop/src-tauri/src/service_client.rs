use apm_core::{
    diagnostics::DiagnosticsReport,
    engine::{
        AvailableUpdatesResult, EngineEvent, InstallHandoffResult, InstallPackageRequest,
        InstallPackageResult, InstallPlanRequest, InstallPlanResult, InstalledPackageSummary,
        PackageDetailsResult, PackageSearchResult, RegistrySyncResult, RemovePackageResult,
        ScanPackagesResult, SetPackagePinResult, UpdatePackageResult,
    },
    model::{ModelInstallResult, ModelRemoveResult, ModelWeightPullResult},
    service::{
        ModelCatalogListResult, ModelChainPlan, ModelChainPlanRequest, ModelListResult,
        ModelManifestCacheRequest, ModelManifestCacheResult, ModelRunPlan, ModelRunPlanRequest,
        ModelStoreInitResult, ModelStoreLayout, OperationAccepted, OperationCancelResult,
        OperationKind, OperationRecoveryRetryResult, OperationRecoverySummary, OperationResult,
        OperationRetryResult, OperationState, OperationStatus, PackagePinBody, PackageRemoveBody,
        PackageUpdateBody,
    },
};
use serde::Serialize;

use crate::service::DesktopServiceSession;
use crate::service_events::stream_operation_events;
use crate::service_http::{read_loopback_token, service_port, ServiceHttpClient};

pub struct DesktopServiceClient {
    http: ServiceHttpClient,
}

pub enum ServiceOperationOutcome<T> {
    Completed {
        result: T,
        events: Vec<EngineEvent>,
    },
    Failed {
        error: String,
        events: Vec<EngineEvent>,
    },
}

impl DesktopServiceClient {
    pub fn from_session(session: &DesktopServiceSession) -> Result<Self, String> {
        let port = service_port(&session.url)?;
        let token = read_loopback_token(std::path::Path::new(&session.token_file))?;
        Ok(Self {
            http: ServiceHttpClient::new(port, token),
        })
    }

    pub fn catalog_snapshot(&self, limit: usize) -> Result<PackageSearchResult, String> {
        self.http.get_json(&format!("/v1/packages?limit={limit}"))
    }

    pub fn package_details(&self, slug: String) -> Result<PackageDetailsResult, String> {
        let slug = service_path_segment(&slug)?;
        self.http
            .get_json(&format!("/v1/packages/{slug}?include_versions=true"))
    }

    pub fn installed_library(&self) -> Result<Vec<InstalledPackageSummary>, String> {
        self.http.get_json("/v1/library")
    }

    pub fn available_updates(&self) -> Result<AvailableUpdatesResult, String> {
        self.http.get_json("/v1/library/updates")
    }

    pub fn diagnostics_report(&self) -> Result<DiagnosticsReport, String> {
        self.http.get_json("/v1/diagnostics")
    }

    pub fn model_packages(&self) -> Result<ModelListResult, String> {
        self.http.get_json("/v1/models")
    }

    pub fn model_catalog(&self) -> Result<ModelCatalogListResult, String> {
        self.http.get_json("/v1/models/catalog")
    }

    pub fn model_store(&self) -> Result<ModelStoreLayout, String> {
        self.http.get_json("/v1/models/store")
    }

    pub fn initialize_model_store(&self) -> Result<ModelStoreInitResult, String> {
        self.http.post_json_ok("/v1/models/store/init", "")
    }

    pub fn cache_model_manifest(
        &self,
        manifest_toml: String,
    ) -> Result<ModelManifestCacheResult, String> {
        let body = encode_json(
            "model manifest cache request",
            &ModelManifestCacheRequest { manifest_toml },
        )?;
        self.http.post_json_ok("/v1/models/manifest/cache", &body)
    }

    pub fn cache_model_catalog_manifest(
        &self,
        name: String,
        version: String,
    ) -> Result<ModelManifestCacheResult, String> {
        let name = service_path_segment(&name)?;
        let version = service_path_segment(&version)?;
        self.http
            .post_json_ok(&format!("/v1/models/catalog/{name}/{version}/cache"), "")
    }

    pub fn pull_model_weights_with_events(
        &self,
        name: String,
        version: String,
        mut on_event: impl FnMut(&str, OperationKind, EngineEvent),
    ) -> Result<ServiceOperationOutcome<ModelWeightPullResult>, String> {
        let name = service_path_segment(&name)?;
        let version = service_path_segment(&version)?;
        let status = self.submit_operation_with_events(
            &format!("/v1/models/{name}/{version}/weights/pull"),
            "",
            OperationKind::ModelWeightPull,
            &mut on_event,
        )?;
        operation_outcome(status, "model weight pull", |result| match result {
            OperationResult::ModelWeightPull(result) => Ok(result),
            other => Err(unexpected_operation_result("model weight pull", &other)),
        })
    }

    pub fn install_model_package_with_events(
        &self,
        name: String,
        version: String,
        mut on_event: impl FnMut(&str, OperationKind, EngineEvent),
    ) -> Result<ServiceOperationOutcome<ModelInstallResult>, String> {
        let name = service_path_segment(&name)?;
        let version = service_path_segment(&version)?;
        let status = self.submit_operation_with_events(
            &format!("/v1/models/{name}/{version}/install"),
            "",
            OperationKind::ModelInstall,
            &mut on_event,
        )?;
        operation_outcome(status, "model install", |result| match result {
            OperationResult::ModelInstall(result) => Ok(result),
            other => Err(unexpected_operation_result("model install", &other)),
        })
    }

    pub fn remove_model_package(
        &self,
        name: String,
        version: String,
    ) -> Result<ModelRemoveResult, String> {
        let name = service_path_segment(&name)?;
        let version = service_path_segment(&version)?;
        self.http
            .delete_json_ok(&format!("/v1/models/{name}/{version}"))
    }

    pub fn plan_model_run(
        &self,
        name: String,
        version: String,
        request: ModelRunPlanRequest,
    ) -> Result<ModelRunPlan, String> {
        let name = service_path_segment(&name)?;
        let version = service_path_segment(&version)?;
        let body = encode_json("model run plan request", &request)?;
        self.http
            .post_json_ok(&format!("/v1/models/{name}/{version}/run/plan"), &body)
    }

    pub fn run_model_with_events(
        &self,
        name: String,
        version: String,
        request: ModelRunPlanRequest,
        mut on_event: impl FnMut(&str, OperationKind, EngineEvent),
    ) -> Result<OperationStatus, String> {
        let name = service_path_segment(&name)?;
        let version = service_path_segment(&version)?;
        let body = encode_json("model run request", &request)?;
        self.submit_operation_with_events(
            &format!("/v1/models/{name}/{version}/run"),
            &body,
            OperationKind::ModelRun,
            &mut on_event,
        )
    }

    pub fn plan_model_chain(
        &self,
        request: ModelChainPlanRequest,
    ) -> Result<ModelChainPlan, String> {
        let body = encode_json("model chain plan request", &request)?;
        self.http.post_json_ok("/v1/models/chains/plan", &body)
    }

    pub fn operation_history(&self) -> Result<Vec<OperationStatus>, String> {
        self.http.get_json("/v1/operations")
    }

    pub fn operation_recovery(&self) -> Result<OperationRecoverySummary, String> {
        self.http.get_json("/v1/operations/recovery")
    }

    pub fn install_plan(&self, request: InstallPlanRequest) -> Result<InstallPlanResult, String> {
        self.http
            .post_json_ok("/v1/install/plan", &install_plan_body(request)?)
    }

    pub fn install_handoff(&self, slug: String) -> Result<InstallHandoffResult, String> {
        self.http
            .post_json_ok(
                "/v1/install/handoff",
                &install_plan_body(InstallPlanRequest {
                    slug,
                    ..InstallPlanRequest::default()
                })?,
            )
    }

    pub fn install_from_archive_with_events(
        &self,
        request: InstallPackageRequest,
        mut on_event: impl FnMut(&str, OperationKind, EngineEvent),
    ) -> Result<ServiceOperationOutcome<InstallPackageResult>, String> {
        self.submit_typed_operation_with_events(
            "/v1/install/archive",
            "archive install",
            OperationKind::InstallArchive,
            &request,
            |result| install_package_operation_result("archive install", result),
            &mut on_event,
        )
    }

    pub fn install_from_url_with_events(
        &self,
        request: InstallPackageRequest,
        mut on_event: impl FnMut(&str, OperationKind, EngineEvent),
    ) -> Result<ServiceOperationOutcome<InstallPackageResult>, String> {
        self.submit_typed_operation_with_events(
            "/v1/install/url",
            "install",
            OperationKind::InstallUrl,
            &request,
            |result| install_package_operation_result("install", result),
            &mut on_event,
        )
    }

    pub fn update_package_with_events(
        &self,
        slug: String,
        request: PackageUpdateBody,
        mut on_event: impl FnMut(&str, OperationKind, EngineEvent),
    ) -> Result<ServiceOperationOutcome<UpdatePackageResult>, String> {
        let slug = service_path_segment(&slug)?;
        self.submit_typed_operation_with_events(
            &format!("/v1/packages/{slug}/update"),
            "update",
            OperationKind::PackageUpdate,
            &request,
            |result| update_package_operation_result("update", result),
            &mut on_event,
        )
    }

    pub fn remove_package_with_events(
        &self,
        slug: String,
        request: PackageRemoveBody,
        mut on_event: impl FnMut(&str, OperationKind, EngineEvent),
    ) -> Result<ServiceOperationOutcome<RemovePackageResult>, String> {
        let slug = service_path_segment(&slug)?;
        self.submit_typed_operation_with_events(
            &format!("/v1/packages/{slug}/remove"),
            "remove",
            OperationKind::PackageRemove,
            &request,
            |result| remove_package_operation_result("remove", result),
            &mut on_event,
        )
    }

    pub fn scan_library_with_events(
        &self,
        mut on_event: impl FnMut(&str, OperationKind, EngineEvent),
    ) -> Result<ServiceOperationOutcome<ScanPackagesResult>, String> {
        let status = self.submit_operation_with_events(
            "/v1/library/scan",
            "",
            OperationKind::LibraryScan,
            &mut on_event,
        )?;
        operation_outcome(status, "library scan", |result| match result {
            OperationResult::LibraryScan(result) => Ok(result),
            other => Err(unexpected_operation_result("library scan", &other)),
        })
    }

    pub fn set_package_pin(
        &self,
        slug: String,
        pinned: bool,
    ) -> Result<SetPackagePinResult, String> {
        let slug = service_path_segment(&slug)?;
        let body = encode_json("pin request", &PackagePinBody { pinned })?;
        self.http
            .post_json_ok(&format!("/v1/packages/{slug}/pin"), &body)
    }

    pub fn cancel_operation(&self, operation_id: String) -> Result<OperationCancelResult, String> {
        let operation_id = service_path_segment(&operation_id)?;
        self.http
            .post_json_ok(&format!("/v1/operations/{operation_id}/cancel"), "")
    }

    pub fn retry_operation(&self, operation_id: String) -> Result<OperationRetryResult, String> {
        let operation_id = service_path_segment(&operation_id)?;
        self.http
            .post_json_accepted(&format!("/v1/operations/{operation_id}/retry"), "")
    }

    pub fn retry_recovery_operations(&self) -> Result<OperationRecoveryRetryResult, String> {
        self.http
            .post_json_accepted("/v1/operations/recovery/retry", "")
    }

    pub fn retry_operation_with_events(
        &self,
        operation_id: String,
        mut on_event: impl FnMut(&str, OperationKind, EngineEvent),
    ) -> Result<OperationStatus, String> {
        let retry = self.retry_operation(operation_id)?;
        self.observe_accepted_operation(&retry.operation, &mut on_event)
    }

    pub fn retry_recovery_operations_with_events(
        &self,
        mut on_event: impl FnMut(&str, OperationKind, EngineEvent),
    ) -> Result<Vec<OperationStatus>, String> {
        let retry = self.retry_recovery_operations()?;
        retry
            .operations
            .iter()
            .map(|retry| self.observe_accepted_operation(&retry.operation, &mut on_event))
            .collect()
    }

    pub fn registry_sync_with_events(
        &self,
        mut on_event: impl FnMut(&str, OperationKind, EngineEvent),
    ) -> Result<RegistrySyncResult, String> {
        let status = self.submit_operation_with_events(
            "/v1/registry/sync",
            "",
            OperationKind::RegistrySync,
            &mut on_event,
        )?;
        match operation_outcome(status, "registry sync", |result| match result {
            OperationResult::RegistrySync(result) => Ok(result),
            other => Err(unexpected_operation_result("registry sync", &other)),
        })? {
            ServiceOperationOutcome::Completed { result, .. } => Ok(result),
            ServiceOperationOutcome::Failed { error, .. } => Err(error),
        }
    }

    fn submit_operation_with_events(
        &self,
        path: &str,
        body: &str,
        expected_kind: OperationKind,
        on_event: &mut impl FnMut(&str, OperationKind, EngineEvent),
    ) -> Result<OperationStatus, String> {
        let accepted: OperationAccepted = self.http.post_json_accepted(path, body)?;
        if accepted.kind != expected_kind {
            return Err(format!(
                "service accepted unexpected operation kind: expected {expected_kind:?}, got {:?}",
                accepted.kind
            ));
        }
        self.observe_accepted_operation(&accepted, on_event)
    }

    fn observe_accepted_operation(
        &self,
        accepted: &OperationAccepted,
        on_event: &mut impl FnMut(&str, OperationKind, EngineEvent),
    ) -> Result<OperationStatus, String> {
        let kind = accepted.kind;
        stream_operation_events(
            self.http.port(),
            self.http.token(),
            &accepted.operation_id,
            &accepted.status_url,
            &mut |operation_id, event| on_event(operation_id, kind, event),
        )?;
        let status: OperationStatus = self.http.get_json(&accepted.status_url)?;
        if status.kind != accepted.kind {
            return Err(format!(
                "service returned operation kind mismatch for {}: accepted {:?}, status {:?}",
                accepted.operation_id, accepted.kind, status.kind
            ));
        }
        Ok(status)
    }

    fn submit_typed_operation_with_events<T, B>(
        &self,
        path: &str,
        action: &str,
        expected_kind: OperationKind,
        request: &B,
        extract: impl FnOnce(OperationResult) -> Result<T, String>,
        on_event: &mut impl FnMut(&str, OperationKind, EngineEvent),
    ) -> Result<ServiceOperationOutcome<T>, String>
    where
        B: Serialize,
    {
        let body = encode_json(&format!("{action} request"), request)?;
        let status = self.submit_operation_with_events(path, &body, expected_kind, on_event)?;
        operation_outcome(status, action, extract)
    }
}

fn install_plan_body(request: InstallPlanRequest) -> Result<String, String> {
    encode_json("install plan request", &request)
}

fn encode_json<T: Serialize>(label: &str, value: &T) -> Result<String, String> {
    serde_json::to_string(value).map_err(|error| format!("failed to encode {label}: {error}"))
}

fn operation_outcome<T>(
    status: OperationStatus,
    action: &str,
    extract: impl FnOnce(OperationResult) -> Result<T, String>,
) -> Result<ServiceOperationOutcome<T>, String> {
    let events = status.events;
    match status.state {
        OperationState::Succeeded => {
            let result = status
                .result
                .ok_or_else(|| format!("{action} finished without a result"))?;
            Ok(ServiceOperationOutcome::Completed {
                result: extract(result)?,
                events,
            })
        }
        OperationState::Failed => Ok(ServiceOperationOutcome::Failed {
            error: status
                .error
                .unwrap_or_else(|| format!("{action} operation failed")),
            events,
        }),
        OperationState::Canceled => Ok(ServiceOperationOutcome::Failed {
            error: status
                .error
                .unwrap_or_else(|| format!("{action} operation was canceled")),
            events,
        }),
        OperationState::Queued | OperationState::Running | OperationState::CancelRequested => {
            Err(format!(
                "{action} operation was not terminal: {}",
                status.operation_id
            ))
        }
    }
}

fn service_path_segment(value: &str) -> Result<&str, String> {
    let safe = value
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'+'));
    if safe && !value.is_empty() {
        Ok(value)
    } else {
        Err(format!("unsupported service path segment: {value}"))
    }
}

fn operation_result_name(result: &OperationResult) -> &'static str {
    match result {
        OperationResult::RegistrySync(_) => "registry_sync",
        OperationResult::LibraryScan(_) => "library_scan",
        OperationResult::InstallPackage(_) => "install_package",
        OperationResult::UpdatePackage(_) => "update_package",
        OperationResult::RemovePackage(_) => "remove_package",
        OperationResult::ModelWeightPull(_) => "model_weight_pull",
        OperationResult::ModelInstall(_) => "model_install",
        OperationResult::ModelRun(_) => "model_run",
    }
}

fn install_package_operation_result(
    action: &str,
    result: OperationResult,
) -> Result<InstallPackageResult, String> {
    match result {
        OperationResult::InstallPackage(result) => Ok(result),
        other => Err(unexpected_operation_result(action, &other)),
    }
}

fn update_package_operation_result(
    action: &str,
    result: OperationResult,
) -> Result<UpdatePackageResult, String> {
    match result {
        OperationResult::UpdatePackage(result) => Ok(result),
        other => Err(unexpected_operation_result(action, &other)),
    }
}

fn remove_package_operation_result(
    action: &str,
    result: OperationResult,
) -> Result<RemovePackageResult, String> {
    match result {
        OperationResult::RemovePackage(result) => Ok(result),
        other => Err(unexpected_operation_result(action, &other)),
    }
}

fn unexpected_operation_result(action: &str, result: &OperationResult) -> String {
    format!(
        "{action} returned unexpected operation result: {}",
        operation_result_name(result)
    )
}

#[cfg(test)]
mod tests;
