use std::path::PathBuf;

use anyhow::Error;
use serde::{Deserialize, Serialize};

use crate::cancel::CancellationToken;
use crate::error::ApmError;
use crate::registry::{InstallType, PluginFormat};

pub const OPERATION_CANCELED_BY_REQUEST: &str = "Operation canceled by request.";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum EngineEvent {
    ScanStarted,
    ScanFinished {
        scanned_count: usize,
        matched_count: usize,
        adopted_count: usize,
    },
    RegistrySyncStarted {
        source_count: usize,
    },
    RegistrySourceSyncStarted {
        source: String,
    },
    RegistrySourceSyncFinished {
        source: String,
        catalog_item_count: usize,
        installable_product_count: usize,
    },
    RegistrySourceSyncFailed {
        source: String,
        error: String,
    },
    RegistrySyncFinished {
        source_count: usize,
        failed_count: usize,
    },
    InstallStarted {
        slug: String,
        version: String,
        format_count: usize,
    },
    InstallFormatStarted {
        slug: String,
        format: PluginFormat,
    },
    InstallDownloadStarted {
        slug: String,
        format: PluginFormat,
        url: String,
    },
    InstallDownloadProgress {
        slug: String,
        format: PluginFormat,
        bytes: u64,
        total_bytes: Option<u64>,
    },
    InstallDownloadFinished {
        slug: String,
        format: PluginFormat,
        path: PathBuf,
        bytes: u64,
    },
    InstallArchiveInstallStarted {
        slug: String,
        format: PluginFormat,
        install_type: InstallType,
        path: PathBuf,
    },
    InstallArchiveVerified {
        slug: String,
        format: PluginFormat,
        path: PathBuf,
        sha256: String,
    },
    InstallQuarantineRemovalStarted {
        slug: String,
        format: PluginFormat,
        path: PathBuf,
    },
    InstallFormatPlaced {
        slug: String,
        format: PluginFormat,
        path: PathBuf,
    },
    InstallStateRecordingStarted {
        slug: String,
    },
    InstallStateRecorded {
        slug: String,
    },
    InstallRolledBack {
        slug: String,
        format: PluginFormat,
        path: PathBuf,
    },
    InstallFinished {
        slug: String,
        installed_format_count: usize,
    },
    InstallFailed {
        slug: String,
        error: String,
    },
    RemoveStarted {
        slug: String,
        version: String,
        format_count: usize,
    },
    RemoveFormatRemoved {
        slug: String,
        format: PluginFormat,
        path: PathBuf,
    },
    RemoveFormatMissing {
        slug: String,
        format: PluginFormat,
        path: PathBuf,
    },
    RemoveStateRecorded {
        slug: String,
    },
    RemoveFinished {
        slug: String,
        removed_format_count: usize,
    },
    RemoveFailed {
        slug: String,
        error: String,
    },
    ModelWeightPullStarted {
        package_id: String,
    },
    ModelWeightPullProgress {
        package_id: String,
        bytes: u64,
        total_bytes: Option<u64>,
    },
    ModelWeightPullFinished {
        package_id: String,
        status: String,
        bytes: u64,
    },
    ModelWeightPullFailed {
        package_id: String,
        error: String,
    },
    ModelInstallStarted {
        package_id: String,
    },
    ModelInstallFinished {
        package_id: String,
        adapter: String,
        runtime_mode: String,
        runtime_status: String,
        weights_status: String,
    },
    ModelInstallFailed {
        package_id: String,
        error: String,
    },
    ModelRunStarted {
        package_id: String,
    },
    ModelRunCompleted {
        package_id: String,
        output_path: String,
        message: String,
    },
    ModelRunBlocked {
        package_id: String,
        blocker: String,
        message: String,
    },
    ModelRunFailed {
        package_id: String,
        error: String,
    },
}

impl EngineEvent {
    pub const SERIALIZED_NAMES: &'static [&'static str] = &[
        "scan_started",
        "scan_finished",
        "registry_sync_started",
        "registry_source_sync_started",
        "registry_source_sync_finished",
        "registry_source_sync_failed",
        "registry_sync_finished",
        "install_started",
        "install_format_started",
        "install_download_started",
        "install_download_progress",
        "install_download_finished",
        "install_archive_install_started",
        "install_archive_verified",
        "install_quarantine_removal_started",
        "install_format_placed",
        "install_state_recording_started",
        "install_state_recorded",
        "install_rolled_back",
        "install_finished",
        "install_failed",
        "remove_started",
        "remove_format_removed",
        "remove_format_missing",
        "remove_state_recorded",
        "remove_finished",
        "remove_failed",
        "model_weight_pull_started",
        "model_weight_pull_progress",
        "model_weight_pull_finished",
        "model_weight_pull_failed",
        "model_install_started",
        "model_install_finished",
        "model_install_failed",
        "model_run_started",
        "model_run_completed",
        "model_run_blocked",
        "model_run_failed",
    ];
}

pub trait EventSink {
    fn emit(&mut self, event: EngineEvent);

    fn cancel_requested(&self) -> bool {
        false
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct NoopEventSink;

impl EventSink for NoopEventSink {
    fn emit(&mut self, _event: EngineEvent) {}
}

impl<F> EventSink for F
where
    F: FnMut(EngineEvent),
{
    fn emit(&mut self, event: EngineEvent) {
        self(event);
    }
}

impl<T: EventSink + ?Sized> CancellationToken for T {
    fn cancel_requested(&self) -> bool {
        EventSink::cancel_requested(self)
    }
}

pub fn is_operation_canceled(error: &Error) -> bool {
    matches!(
        error.downcast_ref::<ApmError>(),
        Some(ApmError::OperationCanceled)
    )
}

#[cfg(test)]
#[path = "events_tests.rs"]
mod tests;
