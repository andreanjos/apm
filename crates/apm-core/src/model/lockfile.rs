use std::collections::HashSet;
use std::path::Path;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use super::manifest::{ModelManifest, RuntimeMode};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ModelLockfile {
    #[serde(rename = "package", default)]
    pub packages: Vec<ModelLockPackage>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelLockPackage {
    pub name: String,
    pub version: String,
    pub mode: RuntimeMode,
    pub weights_sha256: String,
    pub source: String,
}

impl ModelLockfile {
    pub fn from_manifests(manifests: &[ModelManifest]) -> Result<Self> {
        let packages = manifests
            .iter()
            .map(ModelLockPackage::from_manifest)
            .collect();
        let lockfile = Self { packages };
        lockfile.validate()?;
        Ok(lockfile)
    }

    pub fn from_toml_str(input: &str) -> Result<Self> {
        let lockfile: Self = toml::from_str(input).context("Failed to parse apm.lock TOML")?;
        lockfile.validate()?;
        Ok(lockfile)
    }

    pub fn from_path(path: &Path) -> Result<Self> {
        let raw = std::fs::read_to_string(path)
            .with_context(|| format!("Cannot read lockfile: {}", path.display()))?;
        Self::from_toml_str(&raw).with_context(|| format!("Invalid lockfile at {}", path.display()))
    }

    pub fn to_toml_string(&self) -> Result<String> {
        toml::to_string_pretty(self).context("Failed to serialize apm.lock")
    }

    pub fn write_to_path(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).with_context(|| {
                    format!("Cannot create lockfile directory: {}", parent.display())
                })?;
            }
        }
        std::fs::write(path, self.to_toml_string()?)
            .with_context(|| format!("Cannot write lockfile: {}", path.display()))
    }

    pub fn validate(&self) -> Result<()> {
        let mut seen = HashSet::new();
        for package in &self.packages {
            package.validate()?;
            if !seen.insert(package.name.clone()) {
                bail!("duplicate locked package '{}'", package.name);
            }
        }
        Ok(())
    }
}

impl ModelLockPackage {
    pub fn from_manifest(manifest: &ModelManifest) -> Self {
        Self {
            name: manifest.package.name.clone(),
            version: manifest.package.version.clone(),
            mode: manifest.runtime.mode,
            weights_sha256: manifest.weights.sha256.clone(),
            source: manifest.weights.source.clone(),
        }
    }

    fn validate(&self) -> Result<()> {
        if self.name.trim().is_empty() {
            bail!("lock package name must not be empty");
        }
        if self.version.trim().is_empty() {
            bail!("lock package '{}' version must not be empty", self.name);
        }
        if self.weights_sha256.trim().is_empty() {
            bail!(
                "lock package '{}' weights_sha256 must not be empty",
                self.name
            );
        }
        if self.weights_sha256.len() != 64
            || !self.weights_sha256.chars().all(|ch| ch.is_ascii_hexdigit())
        {
            bail!(
                "lock package '{}' weights_sha256 must be a 64-character SHA256 hex digest",
                self.name
            );
        }
        if self.source.trim().is_empty() {
            bail!("lock package '{}' source must not be empty", self.name);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::manifest::ModelManifest;

    const MANIFEST: &str = r#"
[package]
name = "whisper-large-v3"
version = "1.0.0"
description = "Speech transcription"
publisher = "apm-core"

[runtime]
mode = "native-mlx"
entry = "whisper_mlx.Transcriber"

[weights]
source = "hf:mlx-community/whisper-large-v3-mlx"
sha256 = "abcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcd"
format = "safetensors"

[io]
input = "audio"
output = "text"

[license]
spdx = "MIT"
commercial = true

[hardware]
min_memory_gb = 16
requires = ["apple-silicon"]
"#;

    #[test]
    fn builds_lockfile_from_manifests() {
        let manifest = ModelManifest::from_toml_str(MANIFEST).expect("manifest should parse");
        let lockfile = ModelLockfile::from_manifests(&[manifest]).expect("lock should build");

        assert_eq!(lockfile.packages.len(), 1);
        assert_eq!(lockfile.packages[0].name, "whisper-large-v3");
        assert_eq!(lockfile.packages[0].mode, RuntimeMode::NativeMlx);
    }

    #[test]
    fn lockfile_roundtrips_as_package_array() {
        let manifest = ModelManifest::from_toml_str(MANIFEST).expect("manifest should parse");
        let lockfile = ModelLockfile::from_manifests(&[manifest]).expect("lock should build");

        let toml = lockfile.to_toml_string().expect("lock should serialize");
        assert!(toml.contains("[[package]]"));

        let parsed = ModelLockfile::from_toml_str(&toml).expect("lock should parse");
        assert_eq!(parsed, lockfile);
    }

    #[test]
    fn rejects_bad_locked_weight_hash() {
        let lock = r#"
[[package]]
name = "demucs"
version = "4.0.1"
mode = "native-mlx"
weights_sha256 = "bad"
source = "hf:mlx-community/demucs-mlx-fp16"
"#;

        let error = ModelLockfile::from_toml_str(lock).expect_err("bad hash should fail");
        assert!(format!("{error:#}").contains("weights_sha256"));
    }
}
