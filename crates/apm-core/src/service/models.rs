use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::cancel::{ensure_not_cancelled, CancellationToken, NoopCancellationToken};
use crate::config::Config;
use crate::model::{
    install_cached_model_with_cancellation, model_manifest_matches_query, plan_model_chain,
    plan_model_run, pull_model_weights_with_cancellation, pull_model_weights_with_observer,
    remove_cached_model, run_model, run_model_with_observer, IoType, ModelCatalog,
    ModelCatalogPackage, ModelChainPlan, ModelChainPlanRequest, ModelInstallResult, ModelManifest,
    ModelRemoveResult, ModelRunObserver, ModelRunParamValue, ModelRunPlan, ModelRunPlanRequest,
    ModelRunResult, ModelStore, ModelWeightPullObserver, ModelWeightPullResult, ParamType,
    Parameter, RuntimeMode,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelStoreLayout {
    pub root: String,
    pub manifests: String,
    pub weights: String,
    pub runtimes: String,
    pub cache: String,
    pub logs: String,
    pub config: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelStoreInitResult {
    pub layout: ModelStoreLayout,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelListResult {
    pub packages: Vec<CachedModelPackage>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelListRequest {
    #[serde(default)]
    pub query: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelCatalogListRequest {
    #[serde(default)]
    pub query: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelCatalogListResult {
    pub packages: Vec<AvailableModelPackage>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AvailableModelPackage {
    pub package: ModelManifestSummary,
    pub source_name: Option<String>,
    pub manifest_path: String,
    pub manifest_cached: bool,
    pub runtime_entry: String,
    pub weights: ModelWeightsSummary,
    pub params: Vec<ModelParameterSummary>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CachedModelPackage {
    pub package: ModelManifestSummary,
    pub runtime_entry: String,
    pub weights: ModelWeightsSummary,
    pub params: Vec<ModelParameterSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelWeightsSummary {
    pub source: String,
    pub sha256: String,
    pub format: String,
    pub cached: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelParameterSummary {
    pub name: String,
    #[serde(rename = "type")]
    pub param_type: ParamType,
    pub values: Option<Vec<String>>,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub default: Option<ModelRunParamValue>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelManifestValidationRequest {
    pub manifest_toml: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelManifestValidationResult {
    pub package: ModelManifestSummary,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelManifestCacheRequest {
    pub manifest_toml: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelManifestCacheResult {
    pub model: CachedModelPackage,
    pub manifest_path: String,
    pub replaced: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelManifestSummary {
    pub package_id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub publisher: String,
    pub runtime_mode: RuntimeMode,
    pub input: IoType,
    pub output: IoType,
    pub parameter_count: usize,
    pub min_memory_gb: u16,
    pub commercial_license: bool,
}

pub fn model_store_layout() -> ModelStoreLayout {
    model_store_layout_for(&ModelStore::default())
}

pub fn initialize_model_store() -> anyhow::Result<ModelStoreInitResult> {
    initialize_model_store_in_store(&ModelStore::default())
}

pub fn initialize_model_store_in_store(store: &ModelStore) -> anyhow::Result<ModelStoreInitResult> {
    store.ensure()?;
    Ok(ModelStoreInitResult {
        layout: model_store_layout_for(store),
    })
}

fn model_store_layout_for(store: &ModelStore) -> ModelStoreLayout {
    ModelStoreLayout {
        root: path_string(store.root()),
        manifests: path_string(&store.manifests_dir()),
        weights: path_string(&store.weights_dir()),
        runtimes: path_string(&store.runtimes_dir()),
        cache: path_string(&store.cache_dir()),
        logs: path_string(&store.logs_dir()),
        config: path_string(&store.config_file()),
    }
}

pub fn list_cached_models() -> anyhow::Result<ModelListResult> {
    list_cached_models_matching(ModelListRequest::default())
}

pub fn list_cached_models_in_store(store: &ModelStore) -> anyhow::Result<ModelListResult> {
    list_cached_models_matching_in_store(store, ModelListRequest::default())
}

pub fn list_cached_models_matching(request: ModelListRequest) -> anyhow::Result<ModelListResult> {
    list_cached_models_matching_in_store(&ModelStore::default(), request)
}

pub fn list_cached_models_matching_in_store(
    store: &ModelStore,
    request: ModelListRequest,
) -> anyhow::Result<ModelListResult> {
    let mut packages = store
        .cached_manifests()?
        .iter()
        .filter(|manifest| model_manifest_matches_query(manifest, &request.query))
        .map(|manifest| cached_model_package(store, manifest))
        .collect::<anyhow::Result<Vec<_>>>()?;
    packages.sort_by(|left, right| {
        left.package
            .name
            .cmp(&right.package.name)
            .then_with(|| left.package.version.cmp(&right.package.version))
    });
    Ok(ModelListResult { packages })
}

pub fn list_model_catalog(
    config: &Config,
    request: ModelCatalogListRequest,
) -> anyhow::Result<ModelCatalogListResult> {
    list_model_catalog_in_store(config, &ModelStore::default(), request)
}

pub fn list_model_catalog_in_store(
    config: &Config,
    store: &ModelStore,
    request: ModelCatalogListRequest,
) -> anyhow::Result<ModelCatalogListResult> {
    let catalog = ModelCatalog::load_all_sources(config)?;
    let packages = catalog
        .search(&request.query)
        .into_iter()
        .map(|package| available_model_package(store, package))
        .collect::<anyhow::Result<Vec<_>>>()?;
    Ok(ModelCatalogListResult { packages })
}

pub fn validate_model_manifest(
    request: ModelManifestValidationRequest,
) -> anyhow::Result<ModelManifestValidationResult> {
    let manifest = ModelManifest::from_toml_str(&request.manifest_toml)?;
    Ok(ModelManifestValidationResult {
        package: model_manifest_summary(&manifest),
    })
}

pub fn cache_model_manifest(
    request: ModelManifestCacheRequest,
) -> Result<ModelManifestCacheResult, ModelManifestCacheError> {
    cache_model_manifest_in_store(&ModelStore::default(), request)
}

pub fn cache_model_catalog_manifest(
    config: &Config,
    name: String,
    version: String,
) -> Result<ModelManifestCacheResult, ModelManifestCacheError> {
    cache_model_catalog_manifest_in_store(config, &ModelStore::default(), name, version)
}

pub fn cache_model_catalog_manifest_in_store(
    config: &Config,
    store: &ModelStore,
    name: String,
    version: String,
) -> Result<ModelManifestCacheResult, ModelManifestCacheError> {
    let catalog = ModelCatalog::load_all_sources(config).map_err(ModelManifestCacheError::Store)?;
    let package = catalog.find(&name, Some(&version)).ok_or_else(|| {
        ModelManifestCacheError::Invalid(format!(
            "model package not found in configured registries: {name}@{version}"
        ))
    })?;
    cache_model_manifest_in_store(
        store,
        ModelManifestCacheRequest {
            manifest_toml: package.manifest_toml.clone(),
        },
    )
}

pub fn cache_model_manifest_in_store(
    store: &ModelStore,
    request: ModelManifestCacheRequest,
) -> Result<ModelManifestCacheResult, ModelManifestCacheError> {
    let manifest = ModelManifest::from_toml_str(&request.manifest_toml)
        .map_err(|error| ModelManifestCacheError::Invalid(error.to_string()))?;
    let write = store
        .cache_manifest(&manifest, &request.manifest_toml)
        .map_err(ModelManifestCacheError::Store)?;
    Ok(ModelManifestCacheResult {
        model: cached_model_package(store, &manifest).map_err(ModelManifestCacheError::Store)?,
        manifest_path: path_string(&write.path),
        replaced: write.replaced,
    })
}

pub fn pull_cached_model_weights(
    name: String,
    version: String,
) -> anyhow::Result<ModelWeightPullResult> {
    pull_cached_model_weights_with_cancellation(name, version, &NoopCancellationToken)
}

pub fn pull_cached_model_weights_with_cancellation(
    name: String,
    version: String,
    cancellation: &(impl CancellationToken + ?Sized),
) -> anyhow::Result<ModelWeightPullResult> {
    pull_cached_model_weights_in_store_with_cancellation(
        &ModelStore::default(),
        &name,
        &version,
        cancellation,
    )
}

pub fn pull_cached_model_weights_with_observer(
    name: String,
    version: String,
    observer: &mut (impl ModelWeightPullObserver + ?Sized),
) -> anyhow::Result<ModelWeightPullResult> {
    pull_cached_model_weights_in_store_with_observer(
        &ModelStore::default(),
        &name,
        &version,
        observer,
    )
}

pub fn pull_cached_model_weights_in_store(
    store: &ModelStore,
    name: &str,
    version: &str,
) -> anyhow::Result<ModelWeightPullResult> {
    pull_cached_model_weights_in_store_with_cancellation(
        store,
        name,
        version,
        &NoopCancellationToken,
    )
}

pub fn pull_cached_model_weights_in_store_with_cancellation(
    store: &ModelStore,
    name: &str,
    version: &str,
    cancellation: &(impl CancellationToken + ?Sized),
) -> anyhow::Result<ModelWeightPullResult> {
    ensure_not_cancelled(cancellation)?;
    let manifest_path = store.cached_manifest_path(name, version)?;
    let manifest = ModelManifest::from_path(&manifest_path)?;
    ensure_not_cancelled(cancellation)?;
    pull_model_weights_with_cancellation(store, &manifest, cancellation)
}

pub fn pull_cached_model_weights_in_store_with_observer(
    store: &ModelStore,
    name: &str,
    version: &str,
    observer: &mut (impl ModelWeightPullObserver + ?Sized),
) -> anyhow::Result<ModelWeightPullResult> {
    ensure_not_cancelled(observer)?;
    let manifest_path = store.cached_manifest_path(name, version)?;
    let manifest = ModelManifest::from_path(&manifest_path)?;
    ensure_not_cancelled(observer)?;
    pull_model_weights_with_observer(store, &manifest, observer)
}

pub fn install_cached_model_package(
    name: String,
    version: String,
) -> anyhow::Result<ModelInstallResult> {
    install_cached_model_package_with_cancellation(name, version, &NoopCancellationToken)
}

pub fn install_cached_model_package_with_cancellation(
    name: String,
    version: String,
    cancellation: &(impl CancellationToken + ?Sized),
) -> anyhow::Result<ModelInstallResult> {
    install_cached_model_package_in_store_with_cancellation(
        &ModelStore::default(),
        &name,
        &version,
        cancellation,
    )
}

pub fn install_cached_model_package_in_store(
    store: &ModelStore,
    name: &str,
    version: &str,
) -> anyhow::Result<ModelInstallResult> {
    install_cached_model_package_in_store_with_cancellation(
        store,
        name,
        version,
        &NoopCancellationToken,
    )
}

pub fn install_cached_model_package_in_store_with_cancellation(
    store: &ModelStore,
    name: &str,
    version: &str,
    cancellation: &(impl CancellationToken + ?Sized),
) -> anyhow::Result<ModelInstallResult> {
    install_cached_model_with_cancellation(store, name, version, cancellation)
}

pub fn remove_cached_model_package(
    name: String,
    version: String,
) -> anyhow::Result<ModelRemoveResult> {
    remove_cached_model_package_in_store(&ModelStore::default(), &name, &version)
}

pub fn remove_cached_model_package_in_store(
    store: &ModelStore,
    name: &str,
    version: &str,
) -> anyhow::Result<ModelRemoveResult> {
    remove_cached_model(store, name, version)
}

pub fn plan_cached_model_run(
    name: String,
    version: String,
    request: ModelRunPlanRequest,
) -> anyhow::Result<ModelRunPlan> {
    plan_cached_model_run_in_store(&ModelStore::default(), &name, &version, request)
}

pub fn plan_cached_model_run_in_store(
    store: &ModelStore,
    name: &str,
    version: &str,
    request: ModelRunPlanRequest,
) -> anyhow::Result<ModelRunPlan> {
    plan_model_run(store, name, version, request)
}

pub fn run_cached_model(
    name: String,
    version: String,
    request: ModelRunPlanRequest,
) -> anyhow::Result<ModelRunResult> {
    run_cached_model_in_store(&ModelStore::default(), &name, &version, request)
}

pub fn run_cached_model_with_observer(
    name: String,
    version: String,
    request: ModelRunPlanRequest,
    observer: &mut (impl ModelRunObserver + ?Sized),
) -> anyhow::Result<ModelRunResult> {
    run_cached_model_in_store_with_observer(
        &ModelStore::default(),
        &name,
        &version,
        request,
        observer,
    )
}

pub fn run_cached_model_in_store(
    store: &ModelStore,
    name: &str,
    version: &str,
    request: ModelRunPlanRequest,
) -> anyhow::Result<ModelRunResult> {
    run_model(store, name, version, request)
}

pub fn run_cached_model_in_store_with_observer(
    store: &ModelStore,
    name: &str,
    version: &str,
    request: ModelRunPlanRequest,
    observer: &mut (impl ModelRunObserver + ?Sized),
) -> anyhow::Result<ModelRunResult> {
    run_model_with_observer(store, name, version, request, observer)
}

pub fn plan_cached_model_chain(request: ModelChainPlanRequest) -> anyhow::Result<ModelChainPlan> {
    plan_cached_model_chain_in_store(&ModelStore::default(), request)
}

pub fn plan_cached_model_chain_in_store(
    store: &ModelStore,
    request: ModelChainPlanRequest,
) -> anyhow::Result<ModelChainPlan> {
    plan_model_chain(store, request)
}

#[derive(Debug, thiserror::Error)]
pub enum ModelManifestCacheError {
    #[error("{0}")]
    Invalid(String),
    #[error(transparent)]
    Store(#[from] anyhow::Error),
}

fn cached_model_package(
    store: &ModelStore,
    manifest: &ModelManifest,
) -> anyhow::Result<CachedModelPackage> {
    Ok(CachedModelPackage {
        package: model_manifest_summary(manifest),
        runtime_entry: manifest.runtime.entry.clone(),
        weights: model_weights_summary(store, manifest),
        params: model_parameter_summaries(manifest)?,
    })
}

fn available_model_package(
    store: &ModelStore,
    package: &ModelCatalogPackage,
) -> anyhow::Result<AvailableModelPackage> {
    let manifest = &package.manifest;
    Ok(AvailableModelPackage {
        package: model_manifest_summary(manifest),
        source_name: package.source_name.clone(),
        manifest_path: path_string(&package.path),
        manifest_cached: store
            .cached_manifest_path(&manifest.package.name, &manifest.package.version)?
            .exists(),
        runtime_entry: manifest.runtime.entry.clone(),
        weights: model_weights_summary(store, manifest),
        params: model_parameter_summaries(manifest)?,
    })
}

fn model_manifest_summary(manifest: &ModelManifest) -> ModelManifestSummary {
    ModelManifestSummary {
        package_id: manifest.package_id(),
        name: manifest.package.name.clone(),
        version: manifest.package.version.clone(),
        description: manifest.package.description.clone(),
        publisher: manifest.package.publisher.clone(),
        runtime_mode: manifest.runtime.mode,
        input: manifest.io.input,
        output: manifest.io.output,
        parameter_count: manifest.params.len(),
        min_memory_gb: manifest.hardware.min_memory_gb,
        commercial_license: manifest.license.commercial,
    }
}

fn model_parameter_summary(param: &Parameter) -> anyhow::Result<ModelParameterSummary> {
    let default = param
        .default
        .clone()
        .map(ModelRunParamValue::from_toml)
        .transpose()?;
    Ok(ModelParameterSummary {
        name: param.name.clone(),
        param_type: param.param_type,
        values: param.values.clone(),
        min: param.min,
        max: param.max,
        default,
    })
}

fn model_parameter_summaries(
    manifest: &ModelManifest,
) -> anyhow::Result<Vec<ModelParameterSummary>> {
    manifest
        .params
        .iter()
        .map(model_parameter_summary)
        .collect()
}

fn model_weights_summary(store: &ModelStore, manifest: &ModelManifest) -> ModelWeightsSummary {
    ModelWeightsSummary {
        source: manifest.weights.source.clone(),
        sha256: manifest.weights.sha256.clone(),
        format: manifest.weights.format.clone(),
        cached: store.weight_path(&manifest.weights.sha256).exists(),
    }
}

fn path_string(path: &Path) -> String {
    path.display().to_string()
}
