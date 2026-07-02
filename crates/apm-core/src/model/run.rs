use anyhow::{bail, Result};

use crate::cancel::{ensure_not_cancelled, CancellationToken};

use super::{
    plan_model_run, ModelRunPlanRequest, ModelRunResult, ModelRunStatus, ModelRunner, ModelStore,
    UnavailableModelRunner,
};

pub trait ModelRunObserver: CancellationToken {
    fn started(&mut self, _package_id: &str) {}
    fn completed(&mut self, _result: &ModelRunResult) {}
    fn blocked(&mut self, _result: &ModelRunResult) {}
    fn failed(&mut self, _package_id: &str, _error: &str) {}
}

#[derive(Debug, Clone, Copy, Default)]
pub struct NoopModelRunObserver;

impl CancellationToken for NoopModelRunObserver {}

impl ModelRunObserver for NoopModelRunObserver {}

pub fn run_model(
    store: &ModelStore,
    name: &str,
    version: &str,
    request: ModelRunPlanRequest,
) -> Result<ModelRunResult> {
    let mut observer = NoopModelRunObserver;
    run_model_with_observer(store, name, version, request, &mut observer)
}

pub fn run_model_with_observer(
    store: &ModelStore,
    name: &str,
    version: &str,
    request: ModelRunPlanRequest,
    observer: &mut (impl ModelRunObserver + ?Sized),
) -> Result<ModelRunResult> {
    let runner = UnavailableModelRunner;
    run_model_with_runner_and_observer(store, name, version, request, &runner, observer)
}

fn run_model_with_runner_and_observer(
    store: &ModelStore,
    name: &str,
    version: &str,
    request: ModelRunPlanRequest,
    runner: &(impl ModelRunner + ?Sized),
    observer: &mut (impl ModelRunObserver + ?Sized),
) -> Result<ModelRunResult> {
    let requested_package_id = format!("{name}@{version}");
    observer.started(&requested_package_id);
    ensure_model_run_not_cancelled(observer, &requested_package_id)?;

    let plan = match plan_model_run(store, name, version, request) {
        Ok(plan) => plan,
        Err(error) => {
            observer.failed(&requested_package_id, &error.to_string());
            return Err(error);
        }
    };

    let runner_result = {
        let cancellation = BorrowedCancellationToken { inner: &*observer };
        runner.run(plan, &cancellation)
    };
    let result = match runner_result {
        Ok(result) => result,
        Err(error) => {
            observer.failed(&requested_package_id, &error.to_string());
            return Err(error);
        }
    };
    if let Err(error) = validate_runner_result(&requested_package_id, &result) {
        observer.failed(&requested_package_id, &error.to_string());
        return Err(error);
    }
    match result.status() {
        ModelRunStatus::Completed => observer.completed(&result),
        ModelRunStatus::Blocked => observer.blocked(&result),
    }
    Ok(result)
}

fn validate_runner_result(package_id: &str, result: &ModelRunResult) -> Result<()> {
    if result.package_id() != package_id || result.plan().package_id != package_id {
        bail!(
            "model runner returned result for {}, expected {}",
            result.package_id(),
            package_id
        );
    }
    if result.status() == ModelRunStatus::Completed && result.blocked_execution().is_some() {
        bail!("model runner completed blocked execution plan for {package_id}");
    }
    if result.status() == ModelRunStatus::Blocked && result.blocked_execution().is_none() {
        bail!("model runner blocked ready execution plan for {package_id}");
    }
    Ok(())
}

struct BorrowedCancellationToken<'a, T: CancellationToken + ?Sized> {
    inner: &'a T,
}

impl<T: CancellationToken + ?Sized> CancellationToken for BorrowedCancellationToken<'_, T> {
    fn cancel_requested(&self) -> bool {
        self.inner.cancel_requested()
    }
}

fn ensure_model_run_not_cancelled(
    observer: &mut (impl ModelRunObserver + ?Sized),
    package_id: &str,
) -> Result<()> {
    if let Err(error) = ensure_not_cancelled(observer) {
        observer.failed(package_id, &error.to_string());
        return Err(error);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};

    use super::*;
    use crate::model::{
        provision_runtime_adapter, ModelManifest, ModelRunExecutionBlocker,
        ModelRunExecutionReadiness, ModelRunPlan, ModelWeightPullResult, ModelWeightPullStatus,
    };
    use crate::ApmError;

    #[derive(Default)]
    struct RecordingObserver {
        events: Vec<String>,
    }

    impl CancellationToken for RecordingObserver {}

    impl ModelRunObserver for RecordingObserver {
        fn started(&mut self, package_id: &str) {
            self.events.push(format!("started:{package_id}"));
        }

        fn completed(&mut self, result: &ModelRunResult) {
            self.events
                .push(format!("completed:{}", result.package_id()));
        }

        fn blocked(&mut self, result: &ModelRunResult) {
            self.events.push(format!("blocked:{}", result.package_id()));
        }

        fn failed(&mut self, package_id: &str, error: &str) {
            self.events.push(format!("failed:{package_id}:{error}"));
        }
    }

    #[derive(Default)]
    struct CancelingObserver {
        events: Vec<String>,
        checks: Cell<usize>,
        cancel_after: usize,
    }

    impl CancelingObserver {
        fn new(cancel_after: usize) -> Self {
            Self {
                events: Vec::new(),
                checks: Cell::new(0),
                cancel_after,
            }
        }
    }

    impl CancellationToken for CancelingObserver {
        fn cancel_requested(&self) -> bool {
            let checks = self.checks.get() + 1;
            self.checks.set(checks);
            checks > self.cancel_after
        }
    }

    impl ModelRunObserver for CancelingObserver {
        fn started(&mut self, package_id: &str) {
            self.events.push(format!("started:{package_id}"));
        }

        fn completed(&mut self, result: &ModelRunResult) {
            self.events
                .push(format!("completed:{}", result.package_id()));
        }

        fn blocked(&mut self, result: &ModelRunResult) {
            self.events.push(format!("blocked:{}", result.package_id()));
        }

        fn failed(&mut self, package_id: &str, error: &str) {
            self.events.push(format!("failed:{package_id}:{error}"));
        }
    }

    #[test]
    fn run_model_returns_structured_blocked_result() {
        let temp = tempfile::tempdir().expect("temp dir");
        let store = ModelStore::new(temp.path());
        prepare_manifest(&store);
        let mut observer = RecordingObserver::default();

        let result = run_model_with_observer(
            &store,
            "demucs",
            "4.0.1",
            ModelRunPlanRequest::new("mix.wav", "stems/"),
            &mut observer,
        )
        .expect("run model");

        assert_eq!(result.package_id(), "demucs@4.0.1");
        assert_eq!(result.status(), ModelRunStatus::Blocked);
        assert_eq!(result.plan().package_id, "demucs@4.0.1");
        assert!(matches!(
            result.plan().execution,
            ModelRunExecutionReadiness::Blocked {
                blocker: ModelRunExecutionBlocker::AdapterRunnerUnavailable,
                ..
            }
        ));
        assert!(result.message().contains("not implemented yet"));
        assert_eq!(result.terminal_error_message(), Some(result.message()));
        assert_eq!(
            observer.events,
            vec![
                "started:demucs@4.0.1".to_string(),
                "blocked:demucs@4.0.1".to_string(),
            ]
        );
    }

    #[test]
    fn run_model_reports_plan_failures_to_observer() {
        let temp = tempfile::tempdir().expect("temp dir");
        let store = ModelStore::new(temp.path());
        let mut observer = RecordingObserver::default();

        let error = run_model_with_observer(
            &store,
            "demucs",
            "4.0.1",
            ModelRunPlanRequest::new("mix.wav", "stems/"),
            &mut observer,
        )
        .expect_err("run should fail without cached manifest");

        assert!(error
            .to_string()
            .contains("Cannot read cached model manifest"));
        assert_eq!(observer.events.len(), 2);
        assert_eq!(observer.events[0], "started:demucs@4.0.1");
        assert!(observer.events[1].starts_with("failed:demucs@4.0.1:"));
    }

    #[test]
    fn run_model_honors_cancellation_before_planning() {
        let temp = tempfile::tempdir().expect("temp dir");
        let store = ModelStore::new(temp.path());
        prepare_manifest(&store);
        let mut observer = CancelingObserver::new(0);

        let error = run_model_with_observer(
            &store,
            "demucs",
            "4.0.1",
            ModelRunPlanRequest::new("mix.wav", "stems/"),
            &mut observer,
        )
        .expect_err("run should cancel before planning");

        assert_operation_canceled(&error);
        assert_eq!(observer.checks.get(), 1);
        assert_eq!(observer.events.len(), 2);
        assert_eq!(observer.events[0], "started:demucs@4.0.1");
        assert!(observer.events[1].starts_with("failed:demucs@4.0.1:"));
    }

    #[test]
    fn run_model_honors_cancellation_after_planning_before_blocked_result() {
        let temp = tempfile::tempdir().expect("temp dir");
        let store = ModelStore::new(temp.path());
        prepare_manifest(&store);
        let mut observer = CancelingObserver::new(1);

        let error = run_model_with_observer(
            &store,
            "demucs",
            "4.0.1",
            ModelRunPlanRequest::new("mix.wav", "stems/"),
            &mut observer,
        )
        .expect_err("run should cancel after planning");

        assert_operation_canceled(&error);
        assert_eq!(observer.checks.get(), 2);
        assert_eq!(observer.events.len(), 2);
        assert_eq!(observer.events[0], "started:demucs@4.0.1");
        assert!(observer.events[1].starts_with("failed:demucs@4.0.1:"));
    }

    #[test]
    fn run_model_delegates_prepared_plan_to_runner() {
        let temp = tempfile::tempdir().expect("temp dir");
        let store = ModelStore::new(temp.path());
        prepare_manifest(&store);
        let runner_impl = RecordingRunner::default();
        let runner: &dyn ModelRunner = &runner_impl;
        let mut observer = RecordingObserver::default();

        let result = run_model_with_runner_and_observer(
            &store,
            "demucs",
            "4.0.1",
            ModelRunPlanRequest::new("mix.wav", "stems/"),
            runner,
            &mut observer,
        )
        .expect("run model");

        assert_eq!(
            runner_impl.package_ids.borrow().as_slice(),
            ["demucs@4.0.1"]
        );
        assert_eq!(result.status(), ModelRunStatus::Blocked);
        assert_eq!(
            observer.events,
            vec![
                "started:demucs@4.0.1".to_string(),
                "blocked:demucs@4.0.1".to_string(),
            ]
        );
    }

    #[test]
    fn run_model_reports_completed_runner_result() {
        let temp = tempfile::tempdir().expect("temp dir");
        let store = ModelStore::new(temp.path());
        prepare_manifest(&store);
        let runner_impl = CompletingRunner;
        let runner: &dyn ModelRunner = &runner_impl;
        let mut observer = RecordingObserver::default();

        let result = run_model_with_runner_and_observer(
            &store,
            "demucs",
            "4.0.1",
            ModelRunPlanRequest::new("mix.wav", "stems/"),
            runner,
            &mut observer,
        )
        .expect("run model");

        assert_eq!(result.status(), ModelRunStatus::Completed);
        assert_eq!(result.package_id(), "demucs@4.0.1");
        assert_eq!(result.terminal_error_message(), None);
        assert_eq!(
            observer.events,
            vec![
                "started:demucs@4.0.1".to_string(),
                "completed:demucs@4.0.1".to_string(),
            ]
        );
    }

    #[test]
    fn model_run_result_preserves_serialized_contract() {
        let temp = tempfile::tempdir().expect("temp dir");
        let store = ModelStore::new(temp.path());
        prepare_manifest(&store);
        let mut plan = plan_model_run(
            &store,
            "demucs",
            "4.0.1",
            ModelRunPlanRequest::new("mix.wav", "stems/"),
        )
        .expect("plan model run");
        plan.execution = ModelRunExecutionReadiness::Ready {
            message: "test runner can execute this prepared plan".to_string(),
        };

        let result = ModelRunResult::completed(plan).expect("completed result");
        let value = serde_json::to_value(&result).expect("serialize result");

        assert_eq!(value["package_id"], "demucs@4.0.1");
        assert_eq!(value["status"], "completed");
        assert_eq!(
            value["message"],
            "demucs@4.0.1 completed; output written to stems/."
        );
        assert_eq!(value["plan"]["package_id"], "demucs@4.0.1");

        let round_trip: ModelRunResult = serde_json::from_value(value).expect("deserialize result");
        assert_eq!(round_trip.package_id(), "demucs@4.0.1");
        assert_eq!(round_trip.status(), ModelRunStatus::Completed);
        assert_eq!(round_trip.plan().output_path, "stems/");
        assert_eq!(
            round_trip.message(),
            "demucs@4.0.1 completed; output written to stems/."
        );
    }

    #[test]
    fn run_model_rejects_runner_that_completes_blocked_plan() {
        let temp = tempfile::tempdir().expect("temp dir");
        let store = ModelStore::new(temp.path());
        prepare_manifest(&store);
        let runner_impl = BadCompletingRunner;
        let runner: &dyn ModelRunner = &runner_impl;
        let mut observer = RecordingObserver::default();

        let error = run_model_with_runner_and_observer(
            &store,
            "demucs",
            "4.0.1",
            ModelRunPlanRequest::new("mix.wav", "stems/"),
            runner,
            &mut observer,
        )
        .expect_err("blocked plan completion should fail");

        assert!(error
            .to_string()
            .contains("completed blocked execution plan"));
        assert_eq!(observer.events.len(), 2);
        assert_eq!(observer.events[0], "started:demucs@4.0.1");
        assert!(observer.events[1].starts_with("failed:demucs@4.0.1:"));
    }

    #[test]
    fn run_model_rejects_runner_that_blocks_ready_plan() {
        let temp = tempfile::tempdir().expect("temp dir");
        let store = ModelStore::new(temp.path());
        prepare_manifest(&store);
        let runner_impl = BadBlockingRunner;
        let runner: &dyn ModelRunner = &runner_impl;
        let mut observer = RecordingObserver::default();

        let error = run_model_with_runner_and_observer(
            &store,
            "demucs",
            "4.0.1",
            ModelRunPlanRequest::new("mix.wav", "stems/"),
            runner,
            &mut observer,
        )
        .expect_err("ready plan blocking should fail");

        assert!(error.to_string().contains("blocked ready execution plan"));
        assert_eq!(observer.events.len(), 2);
        assert_eq!(observer.events[0], "started:demucs@4.0.1");
        assert!(observer.events[1].starts_with("failed:demucs@4.0.1:"));
    }

    fn prepare_manifest(store: &ModelStore) {
        let manifest_toml = r#"
[package]
name = "demucs"
version = "4.0.1"
description = "Stem separation"
publisher = "apm-core"

[runtime]
mode = "native-mlx"
entry = "demucs.Model"

[weights]
source = "url:https://example.test/demucs.safetensors"
sha256 = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
format = "safetensors"

[io]
input = "audio"
output = "stems"

[license]
spdx = "MIT"
commercial = true

[hardware]
min_memory_gb = 8
requires = ["apple-silicon"]
"#;
        let manifest = ModelManifest::from_toml_str(manifest_toml).expect("manifest");
        store
            .cache_manifest(&manifest, manifest_toml)
            .expect("cache manifest");
        std::fs::create_dir_all(store.weights_dir()).expect("create weights dir");
        let weights_path = store.weight_path(&manifest.weights.sha256);
        std::fs::write(&weights_path, b"weights").expect("write weights");
        provision_runtime_adapter(
            store,
            &manifest,
            &ModelWeightPullResult {
                package_id: manifest.package_id(),
                source: manifest.weights.source.clone(),
                resolved_url: "https://example.test/demucs.safetensors".to_string(),
                sha256: manifest.weights.sha256.clone(),
                path: weights_path.display().to_string(),
                bytes: 7,
                status: ModelWeightPullStatus::Cached,
            },
        )
        .expect("provision runtime");
    }

    fn assert_operation_canceled(error: &anyhow::Error) {
        assert!(matches!(
            error.downcast_ref::<ApmError>(),
            Some(ApmError::OperationCanceled)
        ));
    }

    #[derive(Default)]
    struct RecordingRunner {
        package_ids: RefCell<Vec<String>>,
    }

    impl ModelRunner for RecordingRunner {
        fn run(
            &self,
            plan: ModelRunPlan,
            cancellation: &dyn CancellationToken,
        ) -> Result<ModelRunResult> {
            ensure_not_cancelled(cancellation)?;
            self.package_ids.borrow_mut().push(plan.package_id.clone());
            ModelRunResult::blocked(plan)
        }
    }

    struct CompletingRunner;

    impl ModelRunner for CompletingRunner {
        fn run(
            &self,
            mut plan: ModelRunPlan,
            cancellation: &dyn CancellationToken,
        ) -> Result<ModelRunResult> {
            ensure_not_cancelled(cancellation)?;
            plan.execution = ModelRunExecutionReadiness::Ready {
                message: "test runner can execute this prepared plan".to_string(),
            };
            ModelRunResult::completed(plan)
        }
    }

    struct BadCompletingRunner;

    impl ModelRunner for BadCompletingRunner {
        fn run(
            &self,
            plan: ModelRunPlan,
            cancellation: &dyn CancellationToken,
        ) -> Result<ModelRunResult> {
            ensure_not_cancelled(cancellation)?;
            let package_id = plan.package_id.clone();
            Ok(serde_json::from_value(serde_json::json!({
                "package_id": package_id,
                "status": "completed",
                "message": "bad runner ignored blocked readiness",
                "plan": plan,
            }))?)
        }
    }

    struct BadBlockingRunner;

    impl ModelRunner for BadBlockingRunner {
        fn run(
            &self,
            mut plan: ModelRunPlan,
            cancellation: &dyn CancellationToken,
        ) -> Result<ModelRunResult> {
            ensure_not_cancelled(cancellation)?;
            plan.execution = ModelRunExecutionReadiness::Ready {
                message: "test runner can execute this prepared plan".to_string(),
            };
            let package_id = plan.package_id.clone();
            Ok(serde_json::from_value(serde_json::json!({
                "package_id": package_id,
                "status": "blocked",
                "message": "bad runner blocked ready execution",
                "plan": plan,
            }))?)
        }
    }
}
