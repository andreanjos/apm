use apm_core::{
    engine::{EngineEvent, EventSink},
    model::{
        ModelRunObserver, ModelRunPlanRequest, ModelRunResult, ModelWeightPullObserver,
        ModelWeightPullProgress,
    },
    service::{
        install_cached_model_package_with_cancellation, pull_cached_model_weights_with_observer,
        run_cached_model_with_observer,
    },
    CancellationToken,
};

use super::operations::OperationEventSink;

pub(super) fn run_model_weight_pull_operation(
    sink: &mut OperationEventSink,
    name: String,
    version: String,
) {
    let package_id = format!("{name}@{version}");
    sink.emit(EngineEvent::ModelWeightPullStarted {
        package_id: package_id.clone(),
    });
    let result = {
        let mut observer = ModelWeightPullOperationObserver {
            package_id: &package_id,
            sink,
        };
        pull_cached_model_weights_with_observer(name, version, &mut observer)
    };
    match &result {
        Ok(result) => sink.emit(EngineEvent::ModelWeightPullFinished {
            package_id,
            status: result.status.to_string(),
            bytes: result.bytes,
        }),
        Err(error) => sink.emit(EngineEvent::ModelWeightPullFailed {
            package_id,
            error: error.to_string(),
        }),
    }
    sink.finish_model_weight_pull(result);
}

struct ModelWeightPullOperationObserver<'a> {
    package_id: &'a str,
    sink: &'a mut OperationEventSink,
}

impl CancellationToken for ModelWeightPullOperationObserver<'_> {
    fn cancel_requested(&self) -> bool {
        EventSink::cancel_requested(self.sink)
    }
}

impl ModelWeightPullObserver for ModelWeightPullOperationObserver<'_> {
    fn progress(&mut self, progress: ModelWeightPullProgress) {
        self.sink.emit(EngineEvent::ModelWeightPullProgress {
            package_id: self.package_id.to_string(),
            bytes: progress.bytes,
            total_bytes: progress.total_bytes,
        });
    }
}

pub(super) fn run_model_install_operation(
    sink: &mut OperationEventSink,
    name: String,
    version: String,
) {
    let package_id = format!("{name}@{version}");
    sink.emit(EngineEvent::ModelInstallStarted {
        package_id: package_id.clone(),
    });
    let result = install_cached_model_package_with_cancellation(name, version, &*sink);
    match &result {
        Ok(result) => sink.emit(EngineEvent::ModelInstallFinished {
            package_id,
            adapter: result.runtime.adapter.clone(),
            runtime_mode: result.runtime_mode.to_string(),
            runtime_status: result.runtime.status.to_string(),
            weights_status: result.weights.status.to_string(),
        }),
        Err(error) => sink.emit(EngineEvent::ModelInstallFailed {
            package_id,
            error: error.to_string(),
        }),
    }
    sink.finish_model_install(result);
}

pub(super) fn run_model_run_operation(
    sink: &mut OperationEventSink,
    name: String,
    version: String,
    request: ModelRunPlanRequest,
) {
    let result = {
        let mut observer = ModelRunOperationObserver { sink };
        run_cached_model_with_observer(name, version, request, &mut observer)
    };
    sink.finish_model_run(result);
}

struct ModelRunOperationObserver<'a> {
    sink: &'a mut OperationEventSink,
}

impl CancellationToken for ModelRunOperationObserver<'_> {
    fn cancel_requested(&self) -> bool {
        EventSink::cancel_requested(self.sink)
    }
}

impl ModelRunObserver for ModelRunOperationObserver<'_> {
    fn started(&mut self, package_id: &str) {
        self.sink.emit(EngineEvent::ModelRunStarted {
            package_id: package_id.to_string(),
        });
    }

    fn completed(&mut self, result: &ModelRunResult) {
        self.sink.emit(EngineEvent::ModelRunCompleted {
            package_id: result.package_id().to_string(),
            output_path: result.plan().output_path.clone(),
            message: result.message().to_string(),
        });
    }

    fn blocked(&mut self, result: &ModelRunResult) {
        let Some((blocker, message)) = result.blocked_execution() else {
            return;
        };
        self.sink.emit(EngineEvent::ModelRunBlocked {
            package_id: result.package_id().to_string(),
            blocker: blocker.to_string(),
            message: message.to_string(),
        });
    }

    fn failed(&mut self, package_id: &str, error: &str) {
        self.sink.emit(EngineEvent::ModelRunFailed {
            package_id: package_id.to_string(),
            error: error.to_string(),
        });
    }
}
