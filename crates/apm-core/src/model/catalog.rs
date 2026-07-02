use std::{
    cmp::Ordering,
    collections::HashMap,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use walkdir::WalkDir;

use crate::{
    config::Config,
    registry::{is_hidden, registry_data_dir, source_effective_path},
};

use super::{model_manifest_matches_query, ModelManifest};

#[derive(Debug, Clone, PartialEq)]
pub struct ModelCatalogPackage {
    pub manifest: ModelManifest,
    pub manifest_toml: String,
    pub path: PathBuf,
    pub source_name: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ModelCatalog {
    packages: HashMap<String, ModelCatalogPackage>,
}

impl ModelCatalog {
    pub fn load_from_cache(cache_dir: &Path) -> Result<Self> {
        let registry_dir = registry_data_dir(cache_dir);
        let models_dir = registry_dir.join("models");
        let mut catalog = Self::default();

        if !models_dir.exists() {
            return Ok(catalog);
        }

        for entry in WalkDir::new(&models_dir)
            .into_iter()
            .filter_entry(|entry| !is_hidden(entry.path()))
        {
            let entry = entry.with_context(|| {
                format!(
                    "Cannot read directory entry in model catalog {}",
                    models_dir.display()
                )
            })?;
            let path = entry.path();
            if !entry.file_type().is_file()
                || path.extension().and_then(|ext| ext.to_str()) != Some("toml")
            {
                continue;
            }

            match load_model_manifest(path) {
                Ok(package) => {
                    catalog.insert(package);
                }
                Err(error) => {
                    tracing::warn!("Skipping model manifest {}: {error}", path.display());
                }
            }
        }

        Ok(catalog)
    }

    pub fn load_all_sources(config: &Config) -> Result<Self> {
        let mut merged = Self::default();
        for source in config.sources() {
            let effective_path = source_effective_path(config, &source);
            match Self::load_from_cache(&effective_path) {
                Ok(catalog) => {
                    for mut package in catalog.into_packages() {
                        package.source_name = Some(source.name.clone());
                        merged.insert(package);
                    }
                }
                Err(error) => {
                    tracing::warn!(
                        "Could not load model catalog source '{}': {error}",
                        source.name
                    );
                }
            }
        }
        Ok(merged)
    }

    pub fn find(&self, name: &str, version: Option<&str>) -> Option<&ModelCatalogPackage> {
        if let Some(version) = version {
            return self.packages.get(&catalog_key(name, version));
        }

        self.packages()
            .into_iter()
            .filter(|package| package.manifest.package.name.eq_ignore_ascii_case(name))
            .max_by(|left, right| {
                compare_versions(
                    &left.manifest.package.version,
                    &right.manifest.package.version,
                )
            })
    }

    pub fn search(&self, query: &str) -> Vec<&ModelCatalogPackage> {
        self.packages()
            .into_iter()
            .filter(|package| model_manifest_matches_query(&package.manifest, query))
            .collect()
    }

    pub fn packages(&self) -> Vec<&ModelCatalogPackage> {
        let mut packages = self.packages.values().collect::<Vec<_>>();
        packages.sort_by(|left, right| {
            left.manifest
                .package
                .name
                .cmp(&right.manifest.package.name)
                .then_with(|| {
                    compare_versions(
                        &right.manifest.package.version,
                        &left.manifest.package.version,
                    )
                })
        });
        packages
    }

    fn into_packages(self) -> Vec<ModelCatalogPackage> {
        self.packages.into_values().collect()
    }

    fn insert(&mut self, package: ModelCatalogPackage) {
        self.packages.insert(
            catalog_key(
                &package.manifest.package.name,
                &package.manifest.package.version,
            ),
            package,
        );
    }
}

fn load_model_manifest(path: &Path) -> Result<ModelCatalogPackage> {
    let manifest_toml = std::fs::read_to_string(path)
        .with_context(|| format!("Cannot read model manifest: {}", path.display()))?;
    let manifest = ModelManifest::from_toml_str(&manifest_toml)
        .with_context(|| format!("Invalid model manifest at {}", path.display()))?;
    Ok(ModelCatalogPackage {
        manifest,
        manifest_toml,
        path: path.to_path_buf(),
        source_name: None,
    })
}

fn catalog_key(name: &str, version: &str) -> String {
    format!("{}@{}", name.to_lowercase(), version.to_lowercase())
}

fn compare_versions(left: &str, right: &str) -> Ordering {
    match (semver::Version::parse(left), semver::Version::parse(right)) {
        (Ok(left), Ok(right)) => left.cmp(&right),
        _ => left.cmp(right),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_model_manifests_from_registry_models_dir() {
        let temp = tempfile::tempdir().expect("temp dir");
        write_manifest(temp.path(), "demucs", "4.0.1", "stems");

        let catalog = ModelCatalog::load_from_cache(temp.path()).expect("load catalog");

        let package = catalog
            .find("demucs", Some("4.0.1"))
            .expect("model package");
        assert_eq!(package.manifest.package_id(), "demucs@4.0.1");
        assert!(package.manifest_toml.contains("[package]"));
    }

    #[test]
    fn loads_model_only_nested_registry_dir() {
        let temp = tempfile::tempdir().expect("temp dir");
        write_manifest(&temp.path().join("registry"), "demucs", "4.0.1", "stems");

        let catalog = ModelCatalog::load_from_cache(temp.path()).expect("load catalog");

        assert_eq!(
            catalog
                .find("demucs", Some("4.0.1"))
                .expect("model package")
                .manifest
                .package_id(),
            "demucs@4.0.1"
        );
    }

    #[test]
    fn searches_model_manifest_metadata() {
        let temp = tempfile::tempdir().expect("temp dir");
        write_manifest(temp.path(), "demucs", "4.0.1", "stems");
        write_manifest(temp.path(), "whisper", "1.0.0", "text");
        let catalog = ModelCatalog::load_from_cache(temp.path()).expect("load catalog");

        let results = catalog.search("source stems");

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].manifest.package_id(), "demucs@4.0.1");
    }

    #[test]
    fn resolves_latest_version_when_version_is_omitted() {
        let temp = tempfile::tempdir().expect("temp dir");
        write_manifest(temp.path(), "demucs", "4.0.0", "stems");
        write_manifest(temp.path(), "demucs", "4.0.1", "stems");
        let catalog = ModelCatalog::load_from_cache(temp.path()).expect("load catalog");

        let package = catalog.find("demucs", None).expect("latest demucs");

        assert_eq!(package.manifest.package.version, "4.0.1");
    }

    fn write_manifest(root: &Path, name: &str, version: &str, output: &str) {
        let path = root.join(format!("models/{name}/{version}.toml"));
        std::fs::create_dir_all(path.parent().expect("manifest parent"))
            .expect("create manifest parent");
        std::fs::write(path, test_manifest(name, version, output)).expect("write manifest");
    }

    fn test_manifest(name: &str, version: &str, output: &str) -> String {
        format!(
            r#"
[package]
name = "{name}"
version = "{version}"
description = "Source model"
publisher = "apm-core"

[runtime]
mode = "native-mlx"
entry = "{name}.Model"

[weights]
source = "https://example.test/{name}.safetensors"
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
