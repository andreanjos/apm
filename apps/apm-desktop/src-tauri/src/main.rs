#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::{fs, process::Command};

use apm_core::config;
use apm_core::diagnostics::DiagnosticsReport;
use apm_core::engine::{
    AvailableUpdatesResult, EngineEvent, InstallHandoffResult, InstallHandoffTarget,
    InstallPackageRequest, InstallPackageResult, InstallPlanRequest, InstallPlanResult,
    InstalledPackageSummary, PackageDetailsResult, PackageSearchResult, RegistrySyncResult,
    RemovePackageResult,
    ScanPackagesResult, SetPackagePinResult, UpdatePackageResult,
};
use apm_core::model::{
    ModelInstallResult, ModelRemoveResult, ModelRunResult, ModelRunStatus, ModelWeightPullResult,
};
use apm_core::registry::PluginFormat;
use apm_core::service::{
    ModelCatalogListResult, ModelChainPlan, ModelChainPlanRequest, ModelListResult,
    ModelManifestCacheResult, ModelRunPlan, ModelRunPlanRequest, ModelStoreInitResult,
    ModelStoreLayout, OperationCancelResult, OperationKind, OperationRecoverySummary,
    OperationResult, OperationState, OperationStatus, PackageRemoveBody, PackageUpdateBody,
};
use serde::Serialize;
use tauri::Emitter;

mod service;
mod service_client;
mod service_events;
mod service_http;

use service::{DesktopServiceSession, DesktopServiceSupervisor};
use service_client::{DesktopServiceClient, ServiceOperationOutcome};

const OPERATION_PROGRESS_EVENT: &str = "apm-operation-progress";

#[derive(Debug, Serialize)]
struct DesktopSnapshot {
    service: DesktopServiceSession,
    distribution: DesktopDistribution,
    source_count: usize,
    catalog: PackageSearchResult,
    installed: Vec<InstalledPackageSummary>,
    updates: AvailableUpdatesResult,
    models: ModelListResult,
    model_catalog: ModelCatalogListResult,
    model_store: ModelStoreLayout,
    diagnostics: DiagnosticsReport,
    recovery: OperationRecoverySummary,
    operations: Vec<OperationStatus>,
}

#[derive(Debug, Clone, Serialize)]
struct DesktopDistribution {
    channel: DesktopDistributionChannel,
    app_version: &'static str,
    build_profile: DesktopBuildProfile,
    sidecar_policy: DesktopSidecarPolicy,
    release_gate: DesktopReleaseGate,
    signing: DesktopSigningState,
    notarization: DesktopNotarizationState,
    message: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum DesktopDistributionChannel {
    Development,
    PreviewBundle,
    PublicRelease,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum DesktopBuildProfile {
    Debug,
    Release,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum DesktopSidecarPolicy {
    ExternalOrBundledCli,
    BundledCli,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum DesktopReleaseGate {
    NotApplicable,
    Required,
    Selected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum DesktopSigningState {
    NotChecked,
    DeveloperIdRequired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum DesktopNotarizationState {
    NotChecked,
    Required,
}

#[derive(Debug, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum DesktopInstallResult {
    Completed {
        result: InstallPackageResult,
        events: Vec<EngineEvent>,
    },
    Failed {
        error: String,
        events: Vec<EngineEvent>,
    },
}

#[derive(Debug, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum DesktopRemoveResult {
    Completed {
        result: RemovePackageResult,
        events: Vec<EngineEvent>,
    },
    Failed {
        error: String,
        events: Vec<EngineEvent>,
    },
}

#[derive(Debug, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum DesktopUpdateResult {
    Completed {
        result: Box<UpdatePackageResult>,
        events: Vec<EngineEvent>,
    },
    Failed {
        error: String,
        events: Vec<EngineEvent>,
    },
}

#[derive(Debug, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum DesktopScanResult {
    Completed {
        result: ScanPackagesResult,
        events: Vec<EngineEvent>,
    },
    Failed {
        error: String,
        events: Vec<EngineEvent>,
    },
}

#[derive(Debug, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum DesktopModelWeightPullResult {
    Completed { result: ModelWeightPullResult },
    Failed { error: String },
}

#[derive(Debug, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum DesktopModelInstallResult {
    Completed { result: Box<ModelInstallResult> },
    Failed { error: String },
}

#[derive(Debug, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum DesktopModelRunResult {
    Completed { result: Box<ModelRunResult> },
    Blocked { result: Box<ModelRunResult> },
    Failed { error: String },
}

#[derive(Debug, Clone, Serialize)]
struct DesktopOperationProgress {
    progress_id: Option<String>,
    operation_id: String,
    kind: OperationKind,
    event: EngineEvent,
}

#[tauri::command]
fn desktop_snapshot(
    service: tauri::State<'_, DesktopServiceSupervisor>,
) -> Result<DesktopSnapshot, String> {
    let config = config::init().map_err(|error| error.to_string())?;
    let source_count = config.sources().len();
    let session = service.ensure_started()?;
    let client = DesktopServiceClient::from_session(&session)?;
    let catalog = client.catalog_snapshot(24)?;
    let installed = client.installed_library()?;
    let updates = client.available_updates()?;
    let models = client.model_packages()?;
    let model_catalog = client.model_catalog()?;
    let model_store = client.model_store()?;
    let diagnostics = client.diagnostics_report()?;
    let recovery = client.operation_recovery()?;
    let operations = client.operation_history()?;

    Ok(DesktopSnapshot {
        service: session,
        distribution: desktop_distribution(),
        source_count,
        catalog,
        installed,
        updates,
        models,
        model_catalog,
        model_store,
        diagnostics,
        recovery,
        operations,
    })
}

#[tauri::command]
fn package_details(
    service: tauri::State<'_, DesktopServiceSupervisor>,
    slug: String,
) -> Result<PackageDetailsResult, String> {
    let session = service.ensure_started()?;
    let client = DesktopServiceClient::from_session(&session)?;
    client.package_details(slug)
}

#[tauri::command]
fn initialize_model_store(
    service: tauri::State<'_, DesktopServiceSupervisor>,
) -> Result<ModelStoreInitResult, String> {
    let session = service.ensure_started()?;
    let client = DesktopServiceClient::from_session(&session)?;
    client.initialize_model_store()
}

fn desktop_distribution() -> DesktopDistribution {
    let channel = if cfg!(debug_assertions) {
        DesktopDistributionChannel::Development
    } else if option_env!("APM_DESKTOP_DISTRIBUTION_CHANNEL") == Some("public") {
        DesktopDistributionChannel::PublicRelease
    } else {
        DesktopDistributionChannel::PreviewBundle
    };

    match channel {
        DesktopDistributionChannel::Development => DesktopDistribution {
            channel,
            app_version: env!("CARGO_PKG_VERSION"),
            build_profile: DesktopBuildProfile::Debug,
            sidecar_policy: DesktopSidecarPolicy::ExternalOrBundledCli,
            release_gate: DesktopReleaseGate::NotApplicable,
            signing: DesktopSigningState::NotChecked,
            notarization: DesktopNotarizationState::NotChecked,
            message: "Development build; public release checks are not expected.",
        },
        DesktopDistributionChannel::PublicRelease => DesktopDistribution {
            channel,
            app_version: env!("CARGO_PKG_VERSION"),
            build_profile: DesktopBuildProfile::Release,
            sidecar_policy: DesktopSidecarPolicy::BundledCli,
            release_gate: DesktopReleaseGate::Selected,
            signing: DesktopSigningState::DeveloperIdRequired,
            notarization: DesktopNotarizationState::Required,
            message: "Built through the public macOS release gate; verifier must pass before distribution.",
        },
        DesktopDistributionChannel::PreviewBundle => DesktopDistribution {
            channel,
            app_version: env!("CARGO_PKG_VERSION"),
            build_profile: DesktopBuildProfile::Release,
            sidecar_policy: DesktopSidecarPolicy::BundledCli,
            release_gate: DesktopReleaseGate::Required,
            signing: DesktopSigningState::NotChecked,
            notarization: DesktopNotarizationState::NotChecked,
            message: "Preview bundle; run the public release gate before distributing.",
        },
    }
}

#[tauri::command]
fn sync_registries(
    app: tauri::AppHandle,
    service: tauri::State<'_, DesktopServiceSupervisor>,
    progress_id: Option<String>,
) -> Result<RegistrySyncResult, String> {
    let session = service.ensure_started()?;
    let client = DesktopServiceClient::from_session(&session)?;
    client.registry_sync_with_events(|operation_id, kind, event| {
        emit_operation_progress(&app, progress_id.as_deref(), operation_id, kind, event);
    })
}

#[tauri::command]
fn plan_install(
    service: tauri::State<'_, DesktopServiceSupervisor>,
    slug: String,
    scope: Option<String>,
) -> Result<InstallPlanResult, String> {
    let session = service.ensure_started()?;
    let client = DesktopServiceClient::from_session(&session)?;
    client.install_plan(InstallPlanRequest {
        slug,
        scope: parse_install_scope(scope)?,
        ..InstallPlanRequest::default()
    })
}

#[tauri::command]
fn open_install_handoff(
    service: tauri::State<'_, DesktopServiceSupervisor>,
    slug: String,
) -> Result<InstallHandoffResult, String> {
    let session = service.ensure_started()?;
    let client = DesktopServiceClient::from_session(&session)?;
    let result = client.install_handoff(slug)?;

    if let InstallHandoffResult::Open { handoff, .. } = &result {
        open_handoff_target(&handoff.target)?;
    }

    Ok(result)
}

#[tauri::command]
fn import_model_manifest(
    service: tauri::State<'_, DesktopServiceSupervisor>,
    manifest_path: String,
) -> Result<ModelManifestCacheResult, String> {
    let manifest_toml = fs::read_to_string(&manifest_path)
        .map_err(|error| format!("Failed to read model manifest: {error}"))?;
    let session = service.ensure_started()?;
    let client = DesktopServiceClient::from_session(&session)?;
    client.cache_model_manifest(manifest_toml)
}

#[tauri::command]
fn import_model_catalog_package(
    service: tauri::State<'_, DesktopServiceSupervisor>,
    name: String,
    version: String,
) -> Result<ModelManifestCacheResult, String> {
    let session = service.ensure_started()?;
    let client = DesktopServiceClient::from_session(&session)?;
    client.cache_model_catalog_manifest(name, version)
}

#[tauri::command]
fn pull_model_weights(
    app: tauri::AppHandle,
    service: tauri::State<'_, DesktopServiceSupervisor>,
    name: String,
    version: String,
    progress_id: Option<String>,
) -> Result<DesktopModelWeightPullResult, String> {
    let session = service.ensure_started()?;
    let client = DesktopServiceClient::from_session(&session)?;
    let result =
        client.pull_model_weights_with_events(name, version, |operation_id, kind, event| {
            emit_operation_progress(&app, progress_id.as_deref(), operation_id, kind, event);
        })?;

    Ok(match result {
        ServiceOperationOutcome::Completed { result, .. } => {
            DesktopModelWeightPullResult::Completed { result }
        }
        ServiceOperationOutcome::Failed { error, .. } => {
            DesktopModelWeightPullResult::Failed { error }
        }
    })
}

#[tauri::command]
fn install_model_package(
    app: tauri::AppHandle,
    service: tauri::State<'_, DesktopServiceSupervisor>,
    name: String,
    version: String,
    progress_id: Option<String>,
) -> Result<DesktopModelInstallResult, String> {
    let session = service.ensure_started()?;
    let client = DesktopServiceClient::from_session(&session)?;
    let result =
        client.install_model_package_with_events(name, version, |operation_id, kind, event| {
            emit_operation_progress(&app, progress_id.as_deref(), operation_id, kind, event);
        })?;

    Ok(match result {
        ServiceOperationOutcome::Completed { result, .. } => DesktopModelInstallResult::Completed {
            result: Box::new(result),
        },
        ServiceOperationOutcome::Failed { error, .. } => {
            DesktopModelInstallResult::Failed { error }
        }
    })
}

#[tauri::command]
fn remove_model_package(
    service: tauri::State<'_, DesktopServiceSupervisor>,
    name: String,
    version: String,
) -> Result<ModelRemoveResult, String> {
    let session = service.ensure_started()?;
    let client = DesktopServiceClient::from_session(&session)?;
    client.remove_model_package(name, version)
}

#[tauri::command]
fn plan_model_run(
    service: tauri::State<'_, DesktopServiceSupervisor>,
    name: String,
    version: String,
    input_path: String,
    output_path: String,
) -> Result<ModelRunPlan, String> {
    let session = service.ensure_started()?;
    let client = DesktopServiceClient::from_session(&session)?;
    client.plan_model_run(
        name,
        version,
        ModelRunPlanRequest::new(input_path, output_path),
    )
}

#[tauri::command]
fn run_model(
    app: tauri::AppHandle,
    service: tauri::State<'_, DesktopServiceSupervisor>,
    name: String,
    version: String,
    input_path: String,
    output_path: String,
    progress_id: Option<String>,
) -> Result<DesktopModelRunResult, String> {
    let session = service.ensure_started()?;
    let client = DesktopServiceClient::from_session(&session)?;
    let status = client.run_model_with_events(
        name,
        version,
        ModelRunPlanRequest::new(input_path, output_path),
        |operation_id, kind, event| {
            emit_operation_progress(&app, progress_id.as_deref(), operation_id, kind, event);
        },
    )?;
    Ok(desktop_model_run_result(status))
}

#[tauri::command]
fn plan_model_chain(
    service: tauri::State<'_, DesktopServiceSupervisor>,
    request: ModelChainPlanRequest,
) -> Result<ModelChainPlan, String> {
    let session = service.ensure_started()?;
    let client = DesktopServiceClient::from_session(&session)?;
    client.plan_model_chain(request)
}

#[tauri::command]
fn install_from_archive(
    app: tauri::AppHandle,
    service: tauri::State<'_, DesktopServiceSupervisor>,
    slug: String,
    archive_path: String,
    format: Option<String>,
    scope: Option<String>,
    progress_id: Option<String>,
) -> Result<DesktopInstallResult, String> {
    let session = service.ensure_started()?;
    let client = DesktopServiceClient::from_session(&session)?;
    let result = client.install_from_archive_with_events(
        InstallPackageRequest {
            slug,
            format: parse_plugin_format(format)?,
            scope: parse_install_scope(scope)?,
            archive_path: Some(archive_path.into()),
            ..InstallPackageRequest::default()
        },
        |operation_id, kind, event| {
            emit_operation_progress(&app, progress_id.as_deref(), operation_id, kind, event);
        },
    )?;

    Ok(desktop_install_result(result))
}

#[tauri::command]
fn install_from_url(
    app: tauri::AppHandle,
    service: tauri::State<'_, DesktopServiceSupervisor>,
    slug: String,
    format: String,
    scope: Option<String>,
    progress_id: Option<String>,
) -> Result<DesktopInstallResult, String> {
    let session = service.ensure_started()?;
    let client = DesktopServiceClient::from_session(&session)?;
    let result = client.install_from_url_with_events(
        InstallPackageRequest {
            slug,
            format: parse_plugin_format(Some(format))?,
            scope: parse_install_scope(scope)?,
            ..InstallPackageRequest::default()
        },
        |operation_id, kind, event| {
            emit_operation_progress(&app, progress_id.as_deref(), operation_id, kind, event);
        },
    )?;

    Ok(desktop_install_result(result))
}

#[tauri::command]
fn remove_package(
    app: tauri::AppHandle,
    service: tauri::State<'_, DesktopServiceSupervisor>,
    slug: String,
    progress_id: Option<String>,
) -> Result<DesktopRemoveResult, String> {
    let session = service.ensure_started()?;
    let client = DesktopServiceClient::from_session(&session)?;
    let result = client.remove_package_with_events(
        slug,
        PackageRemoveBody { dry_run: false },
        |operation_id, kind, event| {
            emit_operation_progress(&app, progress_id.as_deref(), operation_id, kind, event);
        },
    )?;

    Ok(match result {
        ServiceOperationOutcome::Completed { result, events } => {
            DesktopRemoveResult::Completed { result, events }
        }
        ServiceOperationOutcome::Failed { error, events } => {
            DesktopRemoveResult::Failed { error, events }
        }
    })
}

#[tauri::command]
fn update_package(
    app: tauri::AppHandle,
    service: tauri::State<'_, DesktopServiceSupervisor>,
    slug: String,
    format: Option<String>,
    progress_id: Option<String>,
) -> Result<DesktopUpdateResult, String> {
    let session = service.ensure_started()?;
    let client = DesktopServiceClient::from_session(&session)?;
    let result = client.update_package_with_events(
        slug,
        PackageUpdateBody {
            format: parse_plugin_format(format)?,
            scope: None,
        },
        |operation_id, kind, event| {
            emit_operation_progress(&app, progress_id.as_deref(), operation_id, kind, event);
        },
    )?;

    Ok(match result {
        ServiceOperationOutcome::Completed { result, events } => DesktopUpdateResult::Completed {
            result: Box::new(result),
            events,
        },
        ServiceOperationOutcome::Failed { error, events } => {
            DesktopUpdateResult::Failed { error, events }
        }
    })
}

#[tauri::command]
fn scan_library(
    app: tauri::AppHandle,
    service: tauri::State<'_, DesktopServiceSupervisor>,
    progress_id: Option<String>,
) -> Result<DesktopScanResult, String> {
    let session = service.ensure_started()?;
    let client = DesktopServiceClient::from_session(&session)?;
    let result = client.scan_library_with_events(|operation_id, kind, event| {
        emit_operation_progress(&app, progress_id.as_deref(), operation_id, kind, event);
    })?;

    Ok(match result {
        ServiceOperationOutcome::Completed { result, events } => {
            DesktopScanResult::Completed { result, events }
        }
        ServiceOperationOutcome::Failed { error, events } => {
            DesktopScanResult::Failed { error, events }
        }
    })
}

#[tauri::command]
fn set_package_pin(
    service: tauri::State<'_, DesktopServiceSupervisor>,
    slug: String,
    pinned: bool,
) -> Result<SetPackagePinResult, String> {
    let session = service.ensure_started()?;
    let client = DesktopServiceClient::from_session(&session)?;
    client.set_package_pin(slug, pinned)
}

#[tauri::command]
fn cancel_operation(
    service: tauri::State<'_, DesktopServiceSupervisor>,
    operation_id: String,
) -> Result<OperationCancelResult, String> {
    let session = service.ensure_started()?;
    let client = DesktopServiceClient::from_session(&session)?;
    client.cancel_operation(operation_id)
}

#[tauri::command]
fn retry_operation(
    app: tauri::AppHandle,
    service: tauri::State<'_, DesktopServiceSupervisor>,
    operation_id: String,
    progress_id: Option<String>,
) -> Result<OperationStatus, String> {
    let session = service.ensure_started()?;
    let client = DesktopServiceClient::from_session(&session)?;
    client.retry_operation_with_events(operation_id, |operation_id, kind, event| {
        emit_operation_progress(&app, progress_id.as_deref(), operation_id, kind, event);
    })
}

#[tauri::command]
fn retry_recovery_operations(
    app: tauri::AppHandle,
    service: tauri::State<'_, DesktopServiceSupervisor>,
    progress_id: Option<String>,
) -> Result<Vec<OperationStatus>, String> {
    let session = service.ensure_started()?;
    let client = DesktopServiceClient::from_session(&session)?;
    client.retry_recovery_operations_with_events(|operation_id, kind, event| {
        emit_operation_progress(&app, progress_id.as_deref(), operation_id, kind, event);
    })
}

#[tauri::command]
fn local_service_status(
    service: tauri::State<'_, DesktopServiceSupervisor>,
) -> Result<DesktopServiceSession, String> {
    service.status()
}

#[tauri::command]
fn ensure_local_service(
    service: tauri::State<'_, DesktopServiceSupervisor>,
) -> Result<DesktopServiceSession, String> {
    service.ensure_started()
}

fn open_handoff_target(target: &InstallHandoffTarget) -> Result<(), String> {
    let mut command = Command::new("open");
    match target {
        InstallHandoffTarget::App { path } => {
            command.arg(path);
        }
        InstallHandoffTarget::Url { url } => {
            command.arg(url);
        }
    }

    command
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("Failed to open install handoff: {error}"))
}

fn parse_plugin_format(value: Option<String>) -> Result<Option<PluginFormat>, String> {
    let Some(value) = value else {
        return Ok(None);
    };

    match value.trim().to_lowercase().as_str() {
        "au" | "component" => Ok(Some(PluginFormat::Au)),
        "vst3" => Ok(Some(PluginFormat::Vst3)),
        "app" | "standalone" => Ok(Some(PluginFormat::App)),
        other => Err(format!("Unsupported plugin format: {other}")),
    }
}

fn parse_install_scope(value: Option<String>) -> Result<Option<config::InstallScope>, String> {
    let Some(value) = value else {
        return Ok(None);
    };

    match value.trim().to_lowercase().as_str() {
        "user" => Ok(Some(config::InstallScope::User)),
        "system" => Ok(Some(config::InstallScope::System)),
        other => Err(format!("Unsupported install scope: {other}")),
    }
}

fn desktop_install_result(
    result: ServiceOperationOutcome<InstallPackageResult>,
) -> DesktopInstallResult {
    match result {
        ServiceOperationOutcome::Completed { result, events } => {
            DesktopInstallResult::Completed { result, events }
        }
        ServiceOperationOutcome::Failed { error, events } => {
            DesktopInstallResult::Failed { error, events }
        }
    }
}

fn desktop_model_run_result(status: OperationStatus) -> DesktopModelRunResult {
    match (status.state, status.result) {
        (OperationState::Succeeded, Some(OperationResult::ModelRun(result)))
            if result.status() == ModelRunStatus::Completed =>
        {
            DesktopModelRunResult::Completed {
                result: Box::new(result),
            }
        }
        (OperationState::Failed, Some(OperationResult::ModelRun(result)))
            if result.status() == ModelRunStatus::Blocked =>
        {
            DesktopModelRunResult::Blocked {
                result: Box::new(result),
            }
        }
        (_, Some(OperationResult::ModelRun(result))) => DesktopModelRunResult::Failed {
            error: status.error.unwrap_or_else(|| result.message().to_string()),
        },
        _ => DesktopModelRunResult::Failed {
            error: status.error.unwrap_or_else(|| {
                format!(
                    "model run operation did not finish with a model result: {}",
                    status.operation_id
                )
            }),
        },
    }
}

fn emit_operation_progress(
    app: &tauri::AppHandle,
    progress_id: Option<&str>,
    operation_id: &str,
    kind: OperationKind,
    event: EngineEvent,
) {
    let _ = app.emit(
        OPERATION_PROGRESS_EVENT,
        DesktopOperationProgress {
            progress_id: progress_id.map(str::to_string),
            operation_id: operation_id.to_string(),
            kind,
            event,
        },
    );
}

fn main() {
    tauri::Builder::default()
        .manage(DesktopServiceSupervisor::default())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            local_service_status,
            ensure_local_service,
            desktop_snapshot,
            package_details,
            initialize_model_store,
            sync_registries,
            plan_install,
            open_install_handoff,
            import_model_manifest,
            import_model_catalog_package,
            pull_model_weights,
            install_model_package,
            remove_model_package,
            plan_model_run,
            run_model,
            plan_model_chain,
            install_from_archive,
            install_from_url,
            remove_package,
            update_package,
            scan_library,
            set_package_pin,
            cancel_operation,
            retry_operation,
            retry_recovery_operations
        ])
        .run(tauri::generate_context!())
        .expect("failed to run apm desktop app")
}

#[cfg(test)]
mod tests {
    use super::{
        desktop_distribution, parse_install_scope, DesktopDistributionChannel, DesktopReleaseGate,
    };
    use apm_core::config::InstallScope;

    #[test]
    fn distribution_reports_compile_time_channel() {
        let distribution = desktop_distribution();

        assert_eq!(distribution.app_version, env!("CARGO_PKG_VERSION"));
        if cfg!(debug_assertions) {
            assert_eq!(
                distribution.channel,
                DesktopDistributionChannel::Development
            );
            assert_eq!(distribution.release_gate, DesktopReleaseGate::NotApplicable);
        } else {
            assert!(matches!(
                distribution.channel,
                DesktopDistributionChannel::PreviewBundle
                    | DesktopDistributionChannel::PublicRelease
            ));
        }
    }

    #[test]
    fn parse_install_scope_accepts_user_and_system() {
        assert_eq!(
            parse_install_scope(Some("user".to_string())),
            Ok(Some(InstallScope::User))
        );
        assert_eq!(
            parse_install_scope(Some("system".to_string())),
            Ok(Some(InstallScope::System))
        );
        assert_eq!(parse_install_scope(None), Ok(None));
        assert!(parse_install_scope(Some("global".to_string())).is_err());
    }
}
