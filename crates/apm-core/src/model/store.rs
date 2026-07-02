use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use walkdir::WalkDir;

use crate::config::ensure_dir;

use super::manifest::ModelManifest;

pub const APM_HOME_ENV: &str = "APM_HOME";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelStore {
    root: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CachedManifestWrite {
    pub path: PathBuf,
    pub replaced: bool,
}

impl Default for ModelStore {
    fn default() -> Self {
        Self::new(default_store_root())
    }
}

impl ModelStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn manifests_dir(&self) -> PathBuf {
        self.root.join("manifests")
    }

    pub fn weights_dir(&self) -> PathBuf {
        self.root.join("weights")
    }

    pub fn runtimes_dir(&self) -> PathBuf {
        self.root.join("runtimes")
    }

    pub fn cache_dir(&self) -> PathBuf {
        self.root.join("cache")
    }

    pub fn logs_dir(&self) -> PathBuf {
        self.root.join("logs")
    }

    pub fn config_file(&self) -> PathBuf {
        self.root.join("config.toml")
    }

    pub fn ensure(&self) -> Result<()> {
        ensure_dir(&self.manifests_dir())?;
        ensure_dir(&self.weights_dir())?;
        ensure_dir(&self.runtimes_dir())?;
        ensure_dir(&self.cache_dir())?;
        ensure_dir(&self.logs_dir())?;
        Ok(())
    }

    pub fn weight_path(&self, sha256: &str) -> PathBuf {
        self.weights_dir().join(sha256)
    }

    pub fn manifest_path(&self, name: &str, version: &str) -> PathBuf {
        self.manifests_dir()
            .join(name)
            .join(format!("{version}.toml"))
    }

    pub fn cached_manifest_path(&self, name: &str, version: &str) -> Result<PathBuf> {
        let name = safe_store_segment("package.name", name)?;
        let version = safe_store_segment("package.version", version)?;
        Ok(self.manifest_path(name, version))
    }

    pub fn runtime_package_dir(
        &self,
        runtime_mode: &str,
        name: &str,
        version: &str,
    ) -> Result<PathBuf> {
        let runtime_mode = safe_store_segment("runtime.mode", runtime_mode)?;
        let name = safe_store_segment("package.name", name)?;
        let version = safe_store_segment("package.version", version)?;
        Ok(self
            .runtimes_dir()
            .join(runtime_mode)
            .join(name)
            .join(version))
    }

    pub fn cache_manifest(
        &self,
        manifest: &ModelManifest,
        manifest_toml: &str,
    ) -> Result<CachedManifestWrite> {
        let name = safe_store_segment("package.name", &manifest.package.name)?;
        let version = safe_store_segment("package.version", &manifest.package.version)?;
        ensure_dir(&self.manifests_dir())?;
        let path = self.manifest_path(name, version);
        ensure_dir(path.parent().expect("manifest path always has a parent"))?;
        let replaced = path.exists();
        std::fs::write(&path, manifest_toml)
            .with_context(|| format!("Cannot cache model manifest: {}", path.display()))?;
        Ok(CachedManifestWrite { path, replaced })
    }

    pub fn cached_manifest_paths(&self) -> Result<Vec<PathBuf>> {
        let manifests_dir = self.manifests_dir();
        if !manifests_dir.exists() {
            return Ok(Vec::new());
        }

        let mut paths = Vec::new();
        for entry in WalkDir::new(&manifests_dir) {
            let entry = entry.with_context(|| {
                format!(
                    "Cannot inspect cached model manifests under {}",
                    manifests_dir.display()
                )
            })?;
            if entry.file_type().is_file()
                && entry.path().extension().is_some_and(|ext| ext == "toml")
            {
                paths.push(entry.path().to_path_buf());
            }
        }
        paths.sort();
        Ok(paths)
    }

    pub fn cached_manifests(&self) -> Result<Vec<ModelManifest>> {
        self.cached_manifest_paths()?
            .into_iter()
            .map(|path| ModelManifest::from_path(&path))
            .collect()
    }
}

fn safe_store_segment<'a>(field: &str, value: &'a str) -> Result<&'a str> {
    let safe = value
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'+'));
    if safe && !value.is_empty() && value != "." && value != ".." {
        Ok(value)
    } else {
        bail!("{field} is not safe for the local model store path");
    }
}

pub fn default_store_root() -> PathBuf {
    if let Some(path) = std::env::var_os(APM_HOME_ENV) {
        return PathBuf::from(path);
    }

    dirs::home_dir()
        .expect("Cannot determine home directory")
        .join(".apm")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_layout_matches_spec() {
        let store = ModelStore::new("/tmp/apm-test");

        assert_eq!(
            store.manifests_dir(),
            PathBuf::from("/tmp/apm-test/manifests")
        );
        assert_eq!(store.weights_dir(), PathBuf::from("/tmp/apm-test/weights"));
        assert_eq!(
            store.runtimes_dir(),
            PathBuf::from("/tmp/apm-test/runtimes")
        );
        assert_eq!(store.cache_dir(), PathBuf::from("/tmp/apm-test/cache"));
        assert_eq!(store.logs_dir(), PathBuf::from("/tmp/apm-test/logs"));
        assert_eq!(
            store.config_file(),
            PathBuf::from("/tmp/apm-test/config.toml")
        );
    }

    #[test]
    fn manifest_paths_are_versioned() {
        let store = ModelStore::new("/tmp/apm-test");

        assert_eq!(
            store.manifest_path("demucs", "4.0.1"),
            PathBuf::from("/tmp/apm-test/manifests/demucs/4.0.1.toml")
        );
    }

    #[test]
    fn cached_manifest_path_rejects_unsafe_segments() {
        let store = ModelStore::new("/tmp/apm-test");

        let error = store
            .cached_manifest_path("../demucs", "4.0.1")
            .expect_err("unsafe cached manifest lookup should fail");

        assert!(error.to_string().contains("package.name"));
    }

    #[test]
    fn runtime_package_dir_rejects_unsafe_segments() {
        let store = ModelStore::new("/tmp/apm-test");

        let error = store
            .runtime_package_dir("native-mlx", "../demucs", "4.0.1")
            .expect_err("unsafe runtime package path should fail");

        assert!(error.to_string().contains("package.name"));
    }

    #[test]
    fn cached_manifests_are_loaded_in_stable_path_order() {
        let temp = tempfile::tempdir().expect("temp dir");
        let store = ModelStore::new(temp.path());
        let first = store.manifest_path("demucs", "4.0.1");
        let second = store.manifest_path("whisper", "3.0.0");
        std::fs::create_dir_all(first.parent().expect("first manifest parent"))
            .expect("create first manifest parent");
        std::fs::create_dir_all(second.parent().expect("second manifest parent"))
            .expect("create second manifest parent");
        std::fs::write(&second, test_manifest("whisper", "3.0.0", "text"))
            .expect("write second manifest");
        std::fs::write(&first, test_manifest("demucs", "4.0.1", "stems"))
            .expect("write first manifest");

        let manifests = store.cached_manifests().expect("load cached manifests");

        assert_eq!(manifests.len(), 2);
        assert_eq!(manifests[0].package_id(), "demucs@4.0.1");
        assert_eq!(manifests[1].package_id(), "whisper@3.0.0");
    }

    #[test]
    fn cache_manifest_writes_validated_manifest_to_versioned_path() {
        let temp = tempfile::tempdir().expect("temp dir");
        let store = ModelStore::new(temp.path());
        let raw = test_manifest("demucs", "4.0.1", "stems");
        let manifest = ModelManifest::from_toml_str(&raw).expect("valid manifest");

        let first = store
            .cache_manifest(&manifest, &raw)
            .expect("cache model manifest");
        let second = store
            .cache_manifest(&manifest, &raw)
            .expect("replace cached model manifest");

        assert_eq!(first.path, temp.path().join("manifests/demucs/4.0.1.toml"));
        assert!(!first.replaced);
        assert!(second.replaced);
        assert_eq!(
            std::fs::read_to_string(first.path).expect("read cached"),
            raw
        );
    }

    #[test]
    fn cache_manifest_rejects_unsafe_path_segments() {
        let temp = tempfile::tempdir().expect("temp dir");
        let store = ModelStore::new(temp.path());
        let raw = test_manifest("demucs", "4.0.1", "stems");
        let mut manifest = ModelManifest::from_toml_str(&raw).expect("valid manifest");
        manifest.package.name = "../demucs".to_string();

        let error = store
            .cache_manifest(&manifest, &raw)
            .expect_err("unsafe model name should be rejected");

        assert!(error.to_string().contains("package.name"));
    }

    #[test]
    fn missing_manifest_cache_is_empty() {
        let temp = tempfile::tempdir().expect("temp dir");
        let store = ModelStore::new(temp.path());

        assert!(store
            .cached_manifests()
            .expect("missing cache should be readable")
            .is_empty());
    }

    fn test_manifest(name: &str, version: &str, output: &str) -> String {
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
source = "hf:apm/{name}"
sha256 = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
format = "safetensors"

[io]
input = "audio"
output = "{output}"

[license]
spdx = "MIT"
commercial = true

[hardware]
min_memory_gb = 8
"#
        )
    }
}
