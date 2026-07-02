use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use super::{ModelManifest, ModelStore};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelRemoveResult {
    pub package_id: String,
    pub manifest_path: String,
    pub runtime_dir: Option<String>,
    pub weight_path: Option<String>,
    pub status: ModelRemoveStatus,
    pub removed_manifest: bool,
    pub removed_runtime: bool,
    pub removed_weight: bool,
    pub weight_still_referenced: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelRemoveStatus {
    Removed,
    NotCached,
}

pub fn remove_cached_model(
    store: &ModelStore,
    name: &str,
    version: &str,
) -> Result<ModelRemoveResult> {
    let manifest_path = store.cached_manifest_path(name, version)?;
    let requested_package_id = format!("{name}@{version}");
    if !manifest_path.exists() {
        return Ok(ModelRemoveResult {
            package_id: requested_package_id,
            manifest_path: path_string(&manifest_path),
            runtime_dir: None,
            weight_path: None,
            status: ModelRemoveStatus::NotCached,
            removed_manifest: false,
            removed_runtime: false,
            removed_weight: false,
            weight_still_referenced: false,
        });
    }

    let manifest = ModelManifest::from_path(&manifest_path)?;
    let runtime_dir = store.runtime_package_dir(
        &manifest.runtime.mode.to_string(),
        &manifest.package.name,
        &manifest.package.version,
    )?;
    let weight_path = store.weight_path(&manifest.weights.sha256);
    let weight_still_referenced =
        cached_weight_is_referenced_elsewhere(store, &manifest.weights.sha256, &manifest_path)?;

    std::fs::remove_file(&manifest_path).with_context(|| {
        format!(
            "Cannot remove cached model manifest: {}",
            manifest_path.display()
        )
    })?;
    remove_empty_parent_dir(&manifest_path);

    let removed_runtime = if runtime_dir.exists() {
        std::fs::remove_dir_all(&runtime_dir).with_context(|| {
            format!(
                "Cannot remove cached model runtime metadata: {}",
                runtime_dir.display()
            )
        })?;
        remove_empty_parent_dir(&runtime_dir);
        true
    } else {
        false
    };

    let removed_weight = if weight_path.exists() && !weight_still_referenced {
        std::fs::remove_file(&weight_path).with_context(|| {
            format!(
                "Cannot remove cached model weights: {}",
                weight_path.display()
            )
        })?;
        true
    } else {
        false
    };

    Ok(ModelRemoveResult {
        package_id: manifest.package_id(),
        manifest_path: path_string(&manifest_path),
        runtime_dir: Some(path_string(&runtime_dir)),
        weight_path: Some(path_string(&weight_path)),
        status: ModelRemoveStatus::Removed,
        removed_manifest: true,
        removed_runtime,
        removed_weight,
        weight_still_referenced,
    })
}

fn cached_weight_is_referenced_elsewhere(
    store: &ModelStore,
    sha256: &str,
    target_manifest_path: &Path,
) -> Result<bool> {
    for path in store.cached_manifest_paths()? {
        if path == target_manifest_path {
            continue;
        }
        let manifest = ModelManifest::from_path(&path)?;
        if manifest.weights.sha256.eq_ignore_ascii_case(sha256) {
            return Ok(true);
        }
    }
    Ok(false)
}

fn remove_empty_parent_dir(path: &Path) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::remove_dir(parent);
    }
}

fn path_string(path: &Path) -> String {
    path.display().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removes_cached_manifest_and_unreferenced_weight() {
        let temp = tempfile::tempdir().expect("temp dir");
        let store = ModelStore::new(temp.path());
        let raw = test_manifest("demucs", "4.0.1", TEST_WEIGHT_SHA256);
        let manifest = ModelManifest::from_toml_str(&raw).expect("manifest");
        store
            .cache_manifest(&manifest, &raw)
            .expect("cache manifest");
        std::fs::create_dir_all(store.weights_dir()).expect("create weights dir");
        std::fs::write(store.weight_path(TEST_WEIGHT_SHA256), TEST_WEIGHT_BYTES)
            .expect("write weights");
        let runtime_dir = store
            .runtime_package_dir("native-mlx", "demucs", "4.0.1")
            .expect("runtime dir");
        std::fs::create_dir_all(&runtime_dir).expect("create runtime dir");
        std::fs::write(runtime_dir.join("adapter.toml"), "adapter = \"native-mlx\"")
            .expect("write runtime metadata");

        let result = remove_cached_model(&store, "demucs", "4.0.1").expect("remove model");

        assert_eq!(result.status, ModelRemoveStatus::Removed);
        assert!(result.removed_manifest);
        assert!(result.removed_runtime);
        assert!(result.removed_weight);
        assert!(!result.weight_still_referenced);
        assert!(!store.manifest_path("demucs", "4.0.1").exists());
        assert!(!runtime_dir.exists());
        assert!(!store.weight_path(TEST_WEIGHT_SHA256).exists());
    }

    #[test]
    fn preserves_weight_referenced_by_another_manifest() {
        let temp = tempfile::tempdir().expect("temp dir");
        let store = ModelStore::new(temp.path());
        let first = test_manifest("demucs", "4.0.1", TEST_WEIGHT_SHA256);
        let second = test_manifest("demucs-alt", "4.0.1", TEST_WEIGHT_SHA256);
        let first_manifest = ModelManifest::from_toml_str(&first).expect("first manifest");
        let second_manifest = ModelManifest::from_toml_str(&second).expect("second manifest");
        store
            .cache_manifest(&first_manifest, &first)
            .expect("cache first");
        store
            .cache_manifest(&second_manifest, &second)
            .expect("cache second");
        std::fs::create_dir_all(store.weights_dir()).expect("create weights dir");
        std::fs::write(store.weight_path(TEST_WEIGHT_SHA256), TEST_WEIGHT_BYTES)
            .expect("write weights");

        let result = remove_cached_model(&store, "demucs", "4.0.1").expect("remove model");

        assert_eq!(result.status, ModelRemoveStatus::Removed);
        assert!(!result.removed_weight);
        assert!(result.weight_still_referenced);
        assert!(store.weight_path(TEST_WEIGHT_SHA256).exists());
    }

    #[test]
    fn fails_before_mutation_when_reference_scan_fails() {
        let temp = tempfile::tempdir().expect("temp dir");
        let store = ModelStore::new(temp.path());
        let raw = test_manifest("demucs", "4.0.1", TEST_WEIGHT_SHA256);
        let manifest = ModelManifest::from_toml_str(&raw).expect("manifest");
        store
            .cache_manifest(&manifest, &raw)
            .expect("cache manifest");
        let broken = store.manifest_path("broken", "1.0.0");
        std::fs::create_dir_all(broken.parent().expect("broken manifest parent"))
            .expect("create broken manifest parent");
        std::fs::write(&broken, "[package]\nname = \"broken\"").expect("write broken manifest");

        let error = remove_cached_model(&store, "demucs", "4.0.1")
            .expect_err("invalid adjacent manifest should fail removal");

        assert!(error.to_string().contains("Invalid model manifest"));
        assert!(store.manifest_path("demucs", "4.0.1").exists());
    }

    #[test]
    fn reports_not_cached_for_missing_manifest() {
        let temp = tempfile::tempdir().expect("temp dir");
        let store = ModelStore::new(temp.path());

        let result = remove_cached_model(&store, "demucs", "4.0.1").expect("remove missing model");

        assert_eq!(result.status, ModelRemoveStatus::NotCached);
        assert_eq!(result.package_id, "demucs@4.0.1");
        assert!(!result.removed_manifest);
        assert!(!result.removed_weight);
    }

    #[test]
    fn rejects_unsafe_model_segments() {
        let temp = tempfile::tempdir().expect("temp dir");
        let store = ModelStore::new(temp.path());

        let error = remove_cached_model(&store, "../demucs", "4.0.1")
            .expect_err("unsafe model name should fail");

        assert!(error.to_string().contains("package.name"));
    }

    const TEST_WEIGHT_BYTES: &[u8] = b"weights";
    const TEST_WEIGHT_SHA256: &str =
        "9a129038d9a00aed0cf6a7ea059ca50a813449061ab87848cf1a13eafdf33b2c";

    fn test_manifest(name: &str, version: &str, sha256: &str) -> String {
        format!(
            r#"
[package]
name = "{name}"
version = "{version}"
description = "Test model"
publisher = "apm-core"

[runtime]
mode = "native-mlx"
entry = "{name}.Model"

[weights]
source = "https://example.test/model.safetensors"
sha256 = "{sha256}"
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
