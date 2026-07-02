use std::{
    collections::{HashMap, VecDeque},
    fs,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
};

use anyhow::{anyhow, Context, Error, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use tracing::warn;

use apm_core::{
    engine::{
        is_operation_canceled, EngineEvent, EventSink, InstallPackageResult, RegistrySyncResult,
        RemovePackageResult, ScanPackagesResult, UpdatePackageResult,
    },
    file::atomic_write,
    model::{ModelInstallResult, ModelRunResult, ModelWeightPullResult},
    service::{
        OperationAccepted, OperationRecoveryCandidate, OperationRecoverySummary, OperationRequest,
        OperationResult, OperationState, OperationStatus,
    },
};

const OPERATION_EVENT_BUFFER_SIZE: usize = 1024;
const OPERATION_HISTORY_SCHEMA_VERSION: u16 = 2;
const MAX_OPERATION_EVENTS_PER_RECORD: usize = OPERATION_EVENT_BUFFER_SIZE;
const MAX_OPERATION_HISTORY_RECORDS: usize = 250;
const OPERATION_INTERRUPTED_BY_RESTART: &str =
    "Operation did not finish before the service restarted.";
const OPERATION_CANCELED_BEFORE_START: &str = "Operation was canceled before it started.";
const OPERATION_CANCEL_REQUESTED: &str =
    "Cancellation requested; the current executor will stop at the next cooperative checkpoint.";
const OPERATION_ALREADY_TERMINAL: &str = "Operation is already terminal.";
pub(super) const OPERATION_ALREADY_SUCCEEDED: &str = "Operation already succeeded.";
pub(super) const OPERATION_NOT_TERMINAL: &str = "Operation is not terminal yet.";
pub(super) const OPERATION_REQUEST_UNAVAILABLE: &str =
    "Operation does not have saved request metadata.";

#[derive(Clone)]
pub(super) struct OperationStore {
    inner: Arc<OperationStoreInner>,
}

struct OperationStoreInner {
    operations: Mutex<HashMap<String, OperationRecord>>,
    next_id: AtomicU64,
    history_path: PathBuf,
}

struct OperationRecord {
    status: OperationStatus,
    sender: Option<broadcast::Sender<OperationStreamMessage>>,
}

#[derive(Debug, Clone)]
pub(super) enum OperationStreamMessage {
    Event(EngineEvent),
    Finished,
}

pub(super) struct OperationEventReplay {
    pub(super) events: VecDeque<EngineEvent>,
    pub(super) receiver: Option<broadcast::Receiver<OperationStreamMessage>>,
}

pub(super) struct OperationEventSink {
    operations: OperationStore,
    operation_id: String,
}

#[derive(Debug)]
pub(super) enum OperationRetryError {
    Unknown,
    AlreadySucceeded,
    NotTerminal,
    RequestUnavailable,
}

#[derive(Debug, Serialize, Deserialize)]
struct OperationHistoryFile {
    schema_version: u16,
    operations: Vec<OperationStatus>,
}

impl OperationStore {
    pub(super) fn new(history_path: PathBuf) -> Result<Self> {
        let operations = load_operation_history(&history_path)?;
        let next_id = operations.len() as u64 + 1;
        let store = Self {
            inner: Arc::new(OperationStoreInner {
                operations: Mutex::new(HashMap::new()),
                next_id: AtomicU64::new(next_id),
                history_path,
            }),
        };
        store.mutate_operations(|store_operations| {
            *store_operations = operations;
        });
        Ok(store)
    }

    pub(super) fn accept(&self, request: OperationRequest) -> OperationAccepted {
        let sequence = self.inner.next_id.fetch_add(1, Ordering::Relaxed);
        let operation_id = format!("op-{}-{sequence}", Utc::now().timestamp_millis());
        let kind = request.kind();
        let status = OperationStatus {
            operation_id: operation_id.clone(),
            kind,
            request: Some(request),
            state: OperationState::Queued,
            created_at: Utc::now(),
            started_at: None,
            finished_at: None,
            result: None,
            error: None,
            events: Vec::new(),
        };
        self.mutate_operations(|operations| {
            let (sender, _) = broadcast::channel(OPERATION_EVENT_BUFFER_SIZE);
            operations.insert(
                operation_id.clone(),
                OperationRecord {
                    status,
                    sender: Some(sender),
                },
            );
        });
        apm_core::service::operation_accepted(operation_id, kind)
    }

    pub(super) fn get(&self, operation_id: &str) -> Option<OperationStatus> {
        self.with_operations(|operations| {
            operations
                .get(operation_id)
                .map(|operation| operation.status.clone())
        })
    }

    pub(super) fn list(&self) -> Vec<OperationStatus> {
        self.with_operations(|operations| operation_history_snapshot(operations))
    }

    pub(super) fn recovery_summary(&self) -> OperationRecoverySummary {
        let candidates = self.with_operations(|operations| {
            operation_history_snapshot(operations)
                .into_iter()
                .filter_map(recovery_candidate)
                .collect::<Vec<_>>()
        });
        let retryable_count = candidates
            .iter()
            .filter(|candidate| candidate.retryable)
            .count();

        OperationRecoverySummary {
            interrupted_count: candidates.len(),
            retryable_count,
            candidates,
        }
    }

    pub(super) fn event_stream(&self, operation_id: &str) -> Option<OperationEventReplay> {
        self.with_operations(|operations| {
            let operation = operations.get(operation_id)?;
            let receiver = if operation_is_terminal(operation.status.state) {
                None
            } else {
                operation.sender.as_ref().map(broadcast::Sender::subscribe)
            };

            Some(OperationEventReplay {
                events: operation.status.events.clone().into(),
                receiver,
            })
        })
    }

    pub(super) fn mark_running(&self, operation_id: &str) -> bool {
        self.mutate_operations(|operations| {
            let Some(operation) = operations.get_mut(operation_id) else {
                return false;
            };
            if operation.status.state != OperationState::Queued {
                return false;
            }
            operation.status.state = OperationState::Running;
            operation.status.started_at = Some(Utc::now());
            true
        })
    }

    pub(super) fn cancel(
        &self,
        operation_id: &str,
    ) -> Option<apm_core::service::OperationCancelResult> {
        self.mutate_operations(|operations| {
            let operation = operations.get_mut(operation_id)?;
            match operation.status.state {
                OperationState::Queued => {
                    operation.status.state = OperationState::Canceled;
                    operation.status.finished_at = Some(Utc::now());
                    operation.status.error = Some(OPERATION_CANCELED_BEFORE_START.to_string());
                    if let Some(sender) = operation.sender.take() {
                        let _ = sender.send(OperationStreamMessage::Finished);
                    }
                    Some(cancel_result(
                        &operation.status,
                        true,
                        OPERATION_CANCELED_BEFORE_START,
                    ))
                }
                OperationState::Running | OperationState::CancelRequested => {
                    operation.status.state = OperationState::CancelRequested;
                    Some(cancel_result(
                        &operation.status,
                        true,
                        OPERATION_CANCEL_REQUESTED,
                    ))
                }
                OperationState::Canceled | OperationState::Succeeded | OperationState::Failed => {
                    Some(cancel_result(
                        &operation.status,
                        false,
                        OPERATION_ALREADY_TERMINAL,
                    ))
                }
            }
        })
    }

    pub(super) fn retry_request(
        &self,
        operation_id: &str,
    ) -> std::result::Result<OperationRequest, OperationRetryError> {
        self.with_operations(|operations| {
            let operation = operations
                .get(operation_id)
                .ok_or(OperationRetryError::Unknown)?;
            match operation.status.state {
                OperationState::Failed | OperationState::Canceled => operation
                    .status
                    .request
                    .clone()
                    .ok_or(OperationRetryError::RequestUnavailable),
                OperationState::Succeeded => Err(OperationRetryError::AlreadySucceeded),
                OperationState::Queued
                | OperationState::Running
                | OperationState::CancelRequested => Err(OperationRetryError::NotTerminal),
            }
        })
    }

    pub(super) fn recovery_retry_requests(&self) -> Vec<(String, OperationRequest)> {
        self.with_operations(|operations| {
            operation_history_snapshot(operations)
                .into_iter()
                .filter(is_restart_interrupted_recovery)
                .filter_map(|status| status.request.map(|request| (status.operation_id, request)))
                .collect()
        })
    }

    pub(super) fn mark_recovery_retry_submitted(
        &self,
        operation_id: &str,
        retry_operation_id: &str,
    ) -> bool {
        self.mutate_operations(|operations| {
            let Some(operation) = operations.get_mut(operation_id) else {
                return false;
            };
            if !is_restart_interrupted_recovery(&operation.status) {
                return false;
            }
            operation.status.error = Some(format!("Retry submitted as {retry_operation_id}."));
            operation.status.request = None;
            true
        })
    }

    fn cancel_requested(&self, operation_id: &str) -> bool {
        self.with_operations(|operations| {
            operations
                .get(operation_id)
                .is_some_and(|operation| operation.status.state == OperationState::CancelRequested)
        })
    }

    fn record_event(&self, operation_id: &str, event: EngineEvent) {
        self.mutate_operations(|operations| {
            if let Some(operation) = operations.get_mut(operation_id) {
                operation.status.events.push(event.clone());
                retain_operation_events(&mut operation.status);
                if let Some(sender) = operation.sender.as_ref() {
                    let _ = sender.send(OperationStreamMessage::Event(event));
                }
            }
        });
    }

    fn finish_registry_sync(&self, operation_id: &str, result: Result<RegistrySyncResult>) {
        match result {
            Ok(result) => {
                let error = result
                    .has_errors()
                    .then(|| anyhow!("One or more registry sources failed to sync."));
                self.finish_operation(
                    operation_id,
                    Some(OperationResult::RegistrySync(result)),
                    error,
                );
            }
            Err(error) => self.finish_operation(operation_id, None, Some(error)),
        }
    }

    fn finish_library_scan(&self, operation_id: &str, result: Result<ScanPackagesResult>) {
        self.finish_result(operation_id, result, OperationResult::LibraryScan);
    }

    fn finish_package_remove(&self, operation_id: &str, result: Result<RemovePackageResult>) {
        self.finish_result(operation_id, result, OperationResult::RemovePackage);
    }

    fn finish_install_package(&self, operation_id: &str, result: Result<InstallPackageResult>) {
        self.finish_result(operation_id, result, OperationResult::InstallPackage);
    }

    fn finish_update_package(&self, operation_id: &str, result: Result<UpdatePackageResult>) {
        self.finish_result(operation_id, result, OperationResult::UpdatePackage);
    }

    fn finish_model_weight_pull(&self, operation_id: &str, result: Result<ModelWeightPullResult>) {
        self.finish_result(operation_id, result, OperationResult::ModelWeightPull);
    }

    fn finish_model_install(&self, operation_id: &str, result: Result<ModelInstallResult>) {
        self.finish_result(operation_id, result, OperationResult::ModelInstall);
    }

    fn finish_model_run(&self, operation_id: &str, result: Result<ModelRunResult>) {
        match result {
            Ok(result) => {
                let error = result
                    .terminal_error_message()
                    .map(|message| anyhow!(message.to_string()));
                self.finish_operation(operation_id, Some(OperationResult::ModelRun(result)), error);
            }
            Err(error) => self.finish_operation(operation_id, None, Some(error)),
        }
    }

    fn finish_result<T>(
        &self,
        operation_id: &str,
        result: Result<T>,
        wrap: impl FnOnce(T) -> OperationResult,
    ) {
        match result {
            Ok(result) => {
                self.finish_operation(operation_id, Some(wrap(result)), None);
            }
            Err(error) => {
                self.finish_operation(operation_id, None, Some(error));
            }
        }
    }

    fn finish_operation(
        &self,
        operation_id: &str,
        result: Option<OperationResult>,
        error: Option<Error>,
    ) {
        self.mutate_operations(|operations| {
            if let Some(operation) = operations.get_mut(operation_id) {
                operation.status.finished_at = Some(Utc::now());
                let canceled = error.as_ref().is_some_and(is_operation_canceled);
                operation.status.state =
                    if operation.status.state == OperationState::CancelRequested && canceled {
                        OperationState::Canceled
                    } else if error.is_some() {
                        OperationState::Failed
                    } else {
                        OperationState::Succeeded
                    };
                operation.status.error = error.map(|error| error.to_string());
                operation.status.result = result;
                if let Some(sender) = operation.sender.take() {
                    let _ = sender.send(OperationStreamMessage::Finished);
                }
            }
        });
    }

    fn with_operations<T>(
        &self,
        action: impl FnOnce(&mut HashMap<String, OperationRecord>) -> T,
    ) -> T {
        let mut operations = self
            .inner
            .operations
            .lock()
            .expect("operation store mutex poisoned");
        action(&mut operations)
    }

    fn mutate_operations<T>(
        &self,
        action: impl FnOnce(&mut HashMap<String, OperationRecord>) -> T,
    ) -> T {
        let (result, snapshot) = {
            let mut operations = self
                .inner
                .operations
                .lock()
                .expect("operation store mutex poisoned");
            let result = action(&mut operations);
            (result, operation_history_snapshot(&operations))
        };
        self.persist_snapshot(snapshot);
        result
    }

    fn persist_snapshot(&self, operations: Vec<OperationStatus>) {
        if let Err(error) = write_operation_history(&self.inner.history_path, operations) {
            warn!(
                error = %error,
                path = %self.inner.history_path.display(),
                "failed to persist apm service operation history"
            );
        }
    }
}

impl OperationEventSink {
    pub(super) fn new(operations: OperationStore, operation_id: String) -> Self {
        Self {
            operations,
            operation_id,
        }
    }

    pub(super) fn finish_registry_sync(&self, result: Result<RegistrySyncResult>) {
        self.operations
            .finish_registry_sync(&self.operation_id, result);
    }

    pub(super) fn finish_library_scan(&self, result: Result<ScanPackagesResult>) {
        self.operations
            .finish_library_scan(&self.operation_id, result);
    }

    pub(super) fn finish_package_remove(&self, result: Result<RemovePackageResult>) {
        self.operations
            .finish_package_remove(&self.operation_id, result);
    }

    pub(super) fn finish_install_package(&self, result: Result<InstallPackageResult>) {
        self.operations
            .finish_install_package(&self.operation_id, result);
    }

    pub(super) fn finish_update_package(&self, result: Result<UpdatePackageResult>) {
        self.operations
            .finish_update_package(&self.operation_id, result);
    }

    pub(super) fn finish_model_weight_pull(&self, result: Result<ModelWeightPullResult>) {
        self.operations
            .finish_model_weight_pull(&self.operation_id, result);
    }

    pub(super) fn finish_model_install(&self, result: Result<ModelInstallResult>) {
        self.operations
            .finish_model_install(&self.operation_id, result);
    }

    pub(super) fn finish_model_run(&self, result: Result<ModelRunResult>) {
        self.operations.finish_model_run(&self.operation_id, result);
    }
}

impl EventSink for OperationEventSink {
    fn emit(&mut self, event: EngineEvent) {
        self.operations.record_event(&self.operation_id, event);
    }

    fn cancel_requested(&self) -> bool {
        self.operations.cancel_requested(&self.operation_id)
    }
}

fn load_operation_history(history_path: &Path) -> Result<HashMap<String, OperationRecord>> {
    if !history_path.exists() {
        return Ok(HashMap::new());
    }

    let content = fs::read_to_string(history_path).with_context(|| {
        format!(
            "Failed to read operation history: {}",
            history_path.display()
        )
    })?;
    let history: OperationHistoryFile = serde_json::from_str(&content).with_context(|| {
        format!(
            "Failed to parse operation history: {}",
            history_path.display()
        )
    })?;

    let retained_history =
        retained_operation_history(history.operations, MAX_OPERATION_HISTORY_RECORDS);
    let mut operations = HashMap::new();
    for mut status in retained_history {
        retain_operation_events(&mut status);
        if !operation_is_terminal(status.state) {
            status.state = OperationState::Failed;
            status.finished_at.get_or_insert_with(Utc::now);
            status
                .error
                .get_or_insert_with(|| OPERATION_INTERRUPTED_BY_RESTART.to_string());
        }
        operations.insert(
            status.operation_id.clone(),
            OperationRecord {
                status,
                sender: None,
            },
        );
    }
    Ok(operations)
}

fn write_operation_history(history_path: &Path, operations: Vec<OperationStatus>) -> Result<()> {
    if let Some(parent) = history_path.parent() {
        apm_core::config::ensure_dir(parent).with_context(|| {
            format!(
                "Failed to create operation history directory: {}",
                parent.display()
            )
        })?;
    }

    let history = OperationHistoryFile {
        schema_version: OPERATION_HISTORY_SCHEMA_VERSION,
        operations,
    };
    let content = serde_json::to_vec_pretty(&history)?;
    atomic_write(history_path, &content).with_context(|| {
        format!(
            "Failed to write operation history: {}",
            history_path.display()
        )
    })
}

fn operation_history_snapshot(
    operations: &HashMap<String, OperationRecord>,
) -> Vec<OperationStatus> {
    let snapshot = operations
        .values()
        .map(|operation| {
            let mut status = operation.status.clone();
            retain_operation_events(&mut status);
            status
        })
        .collect();
    retained_operation_history(snapshot, MAX_OPERATION_HISTORY_RECORDS)
}

fn retained_operation_history(
    mut operations: Vec<OperationStatus>,
    max_records: usize,
) -> Vec<OperationStatus> {
    operations.sort_by(|left, right| {
        left.created_at
            .cmp(&right.created_at)
            .then_with(|| left.operation_id.cmp(&right.operation_id))
    });
    let excess_records = operations.len().saturating_sub(max_records);
    if excess_records > 0 {
        operations.drain(0..excess_records);
    }
    operations
}

fn retain_operation_events(status: &mut OperationStatus) {
    retain_recent_events(&mut status.events, MAX_OPERATION_EVENTS_PER_RECORD);
}

fn retain_recent_events(events: &mut Vec<EngineEvent>, max_events: usize) {
    let excess_events = events.len().saturating_sub(max_events);
    if excess_events > 0 {
        events.drain(0..excess_events);
    }
}

fn recovery_candidate(status: OperationStatus) -> Option<OperationRecoveryCandidate> {
    if !is_restart_interrupted_recovery(&status) {
        return None;
    }

    Some(OperationRecoveryCandidate {
        operation_id: status.operation_id,
        kind: status.kind,
        created_at: status.created_at,
        finished_at: status.finished_at,
        retryable: status.request.is_some(),
        reason: OPERATION_INTERRUPTED_BY_RESTART.to_string(),
    })
}

fn is_restart_interrupted_recovery(status: &OperationStatus) -> bool {
    status.state == OperationState::Failed
        && status.error.as_deref() == Some(OPERATION_INTERRUPTED_BY_RESTART)
}

pub(super) fn operation_is_terminal(state: OperationState) -> bool {
    matches!(
        state,
        OperationState::Canceled | OperationState::Succeeded | OperationState::Failed
    )
}

fn cancel_result(
    status: &OperationStatus,
    accepted: bool,
    message: &str,
) -> apm_core::service::OperationCancelResult {
    apm_core::service::OperationCancelResult {
        operation_id: status.operation_id.clone(),
        state: status.state,
        accepted,
        message: message.to_string(),
    }
}

#[cfg(test)]
mod tests;
