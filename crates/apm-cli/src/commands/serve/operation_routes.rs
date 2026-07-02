use std::{convert::Infallible, result::Result as StdResult};

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse, Response,
    },
    Json,
};
use futures_util::{stream, Stream};
use tokio::sync::broadcast;

use apm_core::{
    engine::{
        ApmEngine, EngineEvent, InstallPackageRequest, RemovePackageRequest, ScanPackagesRequest,
        UpdatePackageRequest,
    },
    service::{
        ModelRunPlanRequest, OperationAccepted, OperationCancelResult,
        OperationRecoveryRetryResult, OperationRecoverySummary, OperationRequest,
        OperationRetryResult, OperationStatus, PackageRemoveBody, PackageUpdateBody,
    },
};

use super::{
    model_operations::{
        run_model_install_operation, run_model_run_operation, run_model_weight_pull_operation,
    },
    operations::{
        OperationEventReplay, OperationEventSink, OperationRetryError, OperationStore,
        OperationStreamMessage, OPERATION_ALREADY_SUCCEEDED, OPERATION_NOT_TERMINAL,
        OPERATION_REQUEST_UNAVAILABLE,
    },
    ServeState, ServiceAccepted, ServiceHttpError, ServiceJson,
};

pub(super) async fn submit_registry_sync(
    State(state): State<ServeState>,
) -> ServiceAccepted<OperationAccepted> {
    accepted_operation_response(submit_operation_request(
        &state,
        OperationRequest::RegistrySync,
    ))
}

pub(super) async fn submit_library_scan(
    State(state): State<ServeState>,
) -> ServiceAccepted<OperationAccepted> {
    accepted_operation_response(submit_operation_request(
        &state,
        OperationRequest::LibraryScan,
    ))
}

pub(super) async fn submit_model_weight_pull(
    State(state): State<ServeState>,
    Path((name, version)): Path<(String, String)>,
) -> ServiceAccepted<OperationAccepted> {
    accepted_operation_response(submit_operation_request(
        &state,
        OperationRequest::ModelWeightPull { name, version },
    ))
}

pub(super) async fn submit_model_install(
    State(state): State<ServeState>,
    Path((name, version)): Path<(String, String)>,
) -> ServiceAccepted<OperationAccepted> {
    accepted_operation_response(submit_operation_request(
        &state,
        OperationRequest::ModelInstall { name, version },
    ))
}

pub(super) async fn submit_model_run(
    State(state): State<ServeState>,
    Path((name, version)): Path<(String, String)>,
    Json(request): Json<ModelRunPlanRequest>,
) -> ServiceAccepted<OperationAccepted> {
    accepted_operation_response(submit_operation_request(
        &state,
        OperationRequest::ModelRun {
            name,
            version,
            request,
        },
    ))
}

pub(super) async fn submit_archive_install(
    State(state): State<ServeState>,
    Json(request): Json<InstallPackageRequest>,
) -> ServiceAccepted<OperationAccepted> {
    accepted_operation_response(submit_operation_request(
        &state,
        OperationRequest::InstallArchive { request },
    ))
}

pub(super) async fn submit_url_install(
    State(state): State<ServeState>,
    Json(request): Json<InstallPackageRequest>,
) -> ServiceAccepted<OperationAccepted> {
    accepted_operation_response(submit_operation_request(
        &state,
        OperationRequest::InstallUrl { request },
    ))
}

pub(super) async fn submit_package_update(
    State(state): State<ServeState>,
    Path(slug): Path<String>,
    Json(request): Json<PackageUpdateBody>,
) -> ServiceAccepted<OperationAccepted> {
    accepted_operation_response(submit_operation_request(
        &state,
        OperationRequest::PackageUpdate {
            slug,
            body: request,
        },
    ))
}

pub(super) async fn submit_package_remove(
    State(state): State<ServeState>,
    Path(slug): Path<String>,
    Json(request): Json<PackageRemoveBody>,
) -> ServiceAccepted<OperationAccepted> {
    accepted_operation_response(submit_operation_request(
        &state,
        OperationRequest::PackageRemove {
            slug,
            body: request,
        },
    ))
}

pub(super) async fn operation_status(
    State(state): State<ServeState>,
    Path(operation_id): Path<String>,
) -> ServiceJson<OperationStatus> {
    state
        .operations
        .get(&operation_id)
        .map(Json)
        .ok_or_else(|| ServiceHttpError::not_found(format!("Unknown operation: {operation_id}")))
}

pub(super) async fn operation_history(
    State(state): State<ServeState>,
) -> Json<Vec<OperationStatus>> {
    Json(state.operations.list())
}

pub(super) async fn operation_recovery(
    State(state): State<ServeState>,
) -> Json<OperationRecoverySummary> {
    Json(state.operations.recovery_summary())
}

pub(super) async fn cancel_operation(
    State(state): State<ServeState>,
    Path(operation_id): Path<String>,
) -> ServiceJson<OperationCancelResult> {
    state
        .operations
        .cancel(&operation_id)
        .map(Json)
        .ok_or_else(|| ServiceHttpError::not_found(format!("Unknown operation: {operation_id}")))
}

pub(super) async fn retry_operation(
    State(state): State<ServeState>,
    Path(operation_id): Path<String>,
) -> ServiceAccepted<OperationRetryResult> {
    let request = state
        .operations
        .retry_request(&operation_id)
        .map_err(|error| retry_error_response(&operation_id, error))?;
    let retry = submit_retry_operation(&state, operation_id, request);

    Ok((StatusCode::ACCEPTED, Json(retry)))
}

pub(super) async fn retry_recovery_operations(
    State(state): State<ServeState>,
) -> ServiceAccepted<OperationRecoveryRetryResult> {
    let operations = state
        .operations
        .recovery_retry_requests()
        .into_iter()
        .map(|(operation_id, request)| submit_retry_operation(&state, operation_id, request))
        .collect::<Vec<_>>();

    Ok((
        StatusCode::ACCEPTED,
        Json(OperationRecoveryRetryResult {
            retried_count: operations.len(),
            message: recovery_retry_message(operations.len()),
            operations,
        }),
    ))
}

pub(super) async fn operation_events(
    State(state): State<ServeState>,
    Path(operation_id): Path<String>,
) -> StdResult<Response, ServiceHttpError> {
    let replay = state
        .operations
        .event_stream(&operation_id)
        .ok_or_else(|| ServiceHttpError::not_found(format!("Unknown operation: {operation_id}")))?;

    Ok(Sse::new(operation_event_stream(replay))
        .keep_alive(KeepAlive::default())
        .into_response())
}

fn submit_operation_request(state: &ServeState, request: OperationRequest) -> OperationAccepted {
    let accepted = state.operations.accept(request.clone());
    let operation_id = accepted.operation_id.clone();
    spawn_operation_request(
        state.operations.clone(),
        state.engine.clone(),
        operation_id,
        request,
    );
    accepted
}

fn spawn_operation_request(
    operations: OperationStore,
    engine: ApmEngine,
    operation_id: String,
    request: OperationRequest,
) {
    match request {
        OperationRequest::RegistrySync => {
            spawn_engine_operation(operations, operation_id, move |sink| {
                let result = engine.sync_registries(sink);
                sink.finish_registry_sync(result);
            });
        }
        OperationRequest::LibraryScan => {
            spawn_engine_operation(operations, operation_id, move |sink| {
                let result = engine.scan_packages(ScanPackagesRequest::reconcile(), sink);
                sink.finish_library_scan(result);
            });
        }
        OperationRequest::InstallArchive { request } => {
            spawn_engine_operation(operations, operation_id, move |sink| {
                let result = engine.install_package_from_archive(request, sink);
                sink.finish_install_package(result);
            });
        }
        OperationRequest::InstallUrl { request } => {
            spawn_engine_operation(operations, operation_id, move |sink| {
                let result = engine.install_package_from_url(request, sink);
                sink.finish_install_package(result);
            });
        }
        OperationRequest::PackageUpdate { slug, body } => {
            spawn_engine_operation(operations, operation_id, move |sink| {
                let result = engine.update_package(
                    UpdatePackageRequest {
                        slug,
                        format: body.format,
                        scope: body.scope,
                    },
                    sink,
                );
                sink.finish_update_package(result);
            });
        }
        OperationRequest::PackageRemove { slug, body } => {
            spawn_engine_operation(operations, operation_id, move |sink| {
                let result = engine.remove_package(
                    RemovePackageRequest {
                        slug,
                        dry_run: body.dry_run,
                    },
                    sink,
                );
                sink.finish_package_remove(result);
            });
        }
        OperationRequest::ModelWeightPull { name, version } => {
            spawn_engine_operation(operations, operation_id, move |sink| {
                run_model_weight_pull_operation(sink, name, version);
            });
        }
        OperationRequest::ModelInstall { name, version } => {
            spawn_engine_operation(operations, operation_id, move |sink| {
                run_model_install_operation(sink, name, version);
            });
        }
        OperationRequest::ModelRun {
            name,
            version,
            request,
        } => {
            spawn_engine_operation(operations, operation_id, move |sink| {
                run_model_run_operation(sink, name, version, request);
            });
        }
    }
}

fn accepted_operation_response(accepted: OperationAccepted) -> ServiceAccepted<OperationAccepted> {
    Ok((StatusCode::ACCEPTED, Json(accepted)))
}

fn spawn_engine_operation(
    operations: OperationStore,
    operation_id: String,
    task: impl FnOnce(&mut OperationEventSink) + Send + 'static,
) {
    tokio::task::spawn_blocking(move || {
        if !operations.mark_running(&operation_id) {
            return;
        }
        let mut sink = OperationEventSink::new(operations, operation_id);
        task(&mut sink);
    });
}

fn submit_retry_operation(
    state: &ServeState,
    original_operation_id: String,
    request: OperationRequest,
) -> OperationRetryResult {
    let operation = submit_operation_request(state, request);
    state
        .operations
        .mark_recovery_retry_submitted(&original_operation_id, &operation.operation_id);
    OperationRetryResult {
        original_operation_id,
        operation,
        message: "Retry operation accepted.".to_string(),
    }
}

fn recovery_retry_message(retried_count: usize) -> String {
    match retried_count {
        0 => "No retryable recovery operations.".to_string(),
        1 => "Retry operation accepted.".to_string(),
        count => format!("{count} retry operations accepted."),
    }
}

fn operation_event_stream(
    replay: OperationEventReplay,
) -> impl Stream<Item = StdResult<Event, Infallible>> {
    stream::unfold(replay, |mut replay| async move {
        if let Some(event) = replay.events.pop_front() {
            return Some((Ok(sse_engine_event(event)), replay));
        }

        let mut receiver = replay.receiver.take()?;
        loop {
            match receiver.recv().await {
                Ok(OperationStreamMessage::Event(event)) => {
                    replay.receiver = Some(receiver);
                    return Some((Ok(sse_engine_event(event)), replay));
                }
                Ok(OperationStreamMessage::Finished) => return None,
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => return None,
            }
        }
    })
}

fn sse_engine_event(event: EngineEvent) -> Event {
    Event::default()
        .event("engine_event")
        .json_data(event)
        .unwrap_or_else(|error| Event::default().event("error").data(error.to_string()))
}

fn retry_error_response(operation_id: &str, error: OperationRetryError) -> ServiceHttpError {
    match error {
        OperationRetryError::Unknown => {
            ServiceHttpError::not_found(format!("Unknown operation: {operation_id}"))
        }
        OperationRetryError::AlreadySucceeded => {
            ServiceHttpError::conflict(OPERATION_ALREADY_SUCCEEDED.to_string())
        }
        OperationRetryError::NotTerminal => {
            ServiceHttpError::conflict(OPERATION_NOT_TERMINAL.to_string())
        }
        OperationRetryError::RequestUnavailable => {
            ServiceHttpError::conflict(OPERATION_REQUEST_UNAVAILABLE.to_string())
        }
    }
}
