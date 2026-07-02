use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::cancel::{ensure_not_cancelled, CancellationToken, NoopCancellationToken};

use super::{
    provision_runtime_adapter, pull_model_weights_with_cancellation, ModelManifest,
    ModelRuntimeProvisioning, ModelStore, ModelWeightPullResult, RuntimeMode,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelInstallResult {
    pub package_id: String,
    pub manifest_path: String,
    pub runtime_mode: RuntimeMode,
    pub runtime_entry: String,
    pub runtime: ModelRuntimeProvisioning,
    pub weights: ModelWeightPullResult,
}

pub fn install_cached_model(
    store: &ModelStore,
    name: &str,
    version: &str,
) -> Result<ModelInstallResult> {
    install_cached_model_with_cancellation(store, name, version, &NoopCancellationToken)
}

pub fn install_cached_model_with_cancellation(
    store: &ModelStore,
    name: &str,
    version: &str,
    cancellation: &(impl CancellationToken + ?Sized),
) -> Result<ModelInstallResult> {
    ensure_not_cancelled(cancellation)?;
    let manifest_path = store.cached_manifest_path(name, version)?;
    let manifest = ModelManifest::from_path(&manifest_path).with_context(|| {
        format!("Cannot install cached model package {name}@{version}: manifest is not cached")
    })?;
    ensure_not_cancelled(cancellation)?;
    let weights = pull_model_weights_with_cancellation(store, &manifest, cancellation)?;
    ensure_not_cancelled(cancellation)?;
    let runtime = provision_runtime_adapter(store, &manifest, &weights)?;

    Ok(ModelInstallResult {
        package_id: manifest.package_id(),
        manifest_path: manifest_path.display().to_string(),
        runtime_mode: manifest.runtime.mode,
        runtime_entry: manifest.runtime.entry,
        runtime,
        weights,
    })
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "reqwest")]
    use std::cell::Cell;

    #[cfg(feature = "reqwest")]
    use crate::{cancel::CancellationToken, ApmError};

    use super::*;

    #[cfg(feature = "reqwest")]
    #[test]
    fn installs_cached_manifest_with_existing_verified_weights() {
        let temp = tempfile::tempdir().expect("temp dir");
        let store = ModelStore::new(temp.path());
        let raw = test_manifest();
        let manifest = ModelManifest::from_toml_str(&raw).expect("manifest");
        store
            .cache_manifest(&manifest, &raw)
            .expect("cache manifest");
        std::fs::create_dir_all(store.weights_dir()).expect("create weights dir");
        std::fs::write(store.weight_path(TEST_WEIGHT_SHA256), TEST_WEIGHT_BYTES)
            .expect("write weights");

        let result = install_cached_model(&store, "demucs", "4.0.1").expect("install cached model");

        assert_eq!(result.package_id, "demucs@4.0.1");
        assert_eq!(result.runtime_mode, RuntimeMode::NativeMlx);
        assert_eq!(result.runtime_entry, "demucs_mlx.Separator");
        assert_eq!(result.runtime.adapter, "native-mlx");
        assert_eq!(
            result.runtime.status,
            super::super::RuntimeProvisioningStatus::Prepared
        );
        assert!(result
            .runtime
            .runtime_dir
            .ends_with("runtimes/native-mlx/demucs/4.0.1"));
        assert!(result.runtime.files[0].ends_with("adapter.toml"));
        assert_eq!(
            result.weights.status,
            super::super::ModelWeightPullStatus::Cached
        );
    }

    #[test]
    fn rejects_unsafe_model_segments() {
        let temp = tempfile::tempdir().expect("temp dir");
        let store = ModelStore::new(temp.path());

        let error = install_cached_model(&store, "../demucs", "4.0.1")
            .expect_err("unsafe model name should fail");

        assert!(error.to_string().contains("package.name"));
    }

    #[cfg(feature = "reqwest")]
    #[test]
    fn cancellation_before_runtime_provisioning_leaves_runtime_unprepared() {
        let temp = tempfile::tempdir().expect("temp dir");
        let store = ModelStore::new(temp.path());
        let raw = test_manifest();
        let manifest = ModelManifest::from_toml_str(&raw).expect("manifest");
        store
            .cache_manifest(&manifest, &raw)
            .expect("cache manifest");
        std::fs::create_dir_all(store.weights_dir()).expect("create weights dir");
        std::fs::write(store.weight_path(TEST_WEIGHT_SHA256), TEST_WEIGHT_BYTES)
            .expect("write weights");
        let cancellation = CancelAfterChecks::new(3);

        let error =
            install_cached_model_with_cancellation(&store, "demucs", "4.0.1", &cancellation)
                .expect_err("canceled install should fail");

        assert_operation_canceled(&error);
        let runtime_dir = store
            .runtime_package_dir("native-mlx", "demucs", "4.0.1")
            .expect("runtime dir");
        assert!(!runtime_dir.exists());
    }

    #[cfg(feature = "reqwest")]
    const TEST_WEIGHT_BYTES: &[u8] = b"weights";
    #[cfg(feature = "reqwest")]
    const TEST_WEIGHT_SHA256: &str =
        "9a129038d9a00aed0cf6a7ea059ca50a813449061ab87848cf1a13eafdf33b2c";

    #[cfg(feature = "reqwest")]
    struct CancelAfterChecks {
        checks: Cell<usize>,
        cancel_after: usize,
    }

    #[cfg(feature = "reqwest")]
    impl CancelAfterChecks {
        fn new(cancel_after: usize) -> Self {
            Self {
                checks: Cell::new(0),
                cancel_after,
            }
        }
    }

    #[cfg(feature = "reqwest")]
    impl CancellationToken for CancelAfterChecks {
        fn cancel_requested(&self) -> bool {
            let checks = self.checks.get() + 1;
            self.checks.set(checks);
            checks > self.cancel_after
        }
    }

    #[cfg(feature = "reqwest")]
    fn assert_operation_canceled(error: &anyhow::Error) {
        assert!(matches!(
            error.downcast_ref::<ApmError>(),
            Some(ApmError::OperationCanceled)
        ));
    }

    #[cfg(feature = "reqwest")]
    fn test_manifest() -> String {
        format!(
            r#"
[package]
name = "demucs"
version = "4.0.1"
description = "Music source separation into stems"
publisher = "apm-core"

[runtime]
mode = "native-mlx"
entry = "demucs_mlx.Separator"

[weights]
source = "hf:mlx-community/demucs-mlx-fp16"
sha256 = "{TEST_WEIGHT_SHA256}"
format = "safetensors"

[io]
input = "audio"
output = "stems"

[license]
spdx = "MIT"
commercial = true

[hardware]
min_memory_gb = 8
"#
        )
    }
}
