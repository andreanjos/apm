use crate::config::Config;
use crate::engine::InstallPackageRequest;
use crate::model::{
    provision_runtime_adapter, IoType, ModelRunExecutionBlocker, ModelRunExecutionReadiness,
    ModelRunParamSource, ModelRunParamValue, ModelRunPlanRequest, ModelStore,
    ModelWeightPullResult, ModelWeightPullStatus, ParamType, RuntimeMode,
};

use super::*;

const TEST_WEIGHT_BYTES: &[u8] = b"weights";
const TEST_WEIGHT_SHA256: &str = "9a129038d9a00aed0cf6a7ea059ca50a813449061ab87848cf1a13eafdf33b2c";

mod chain;
mod contract;
mod privileged;

#[test]
fn list_cached_models_returns_gui_renderable_metadata() {
    let temp = tempfile::tempdir().expect("temp dir");
    let store = ModelStore::new(temp.path());
    let manifest_path = store.manifest_path("demucs", "4.0.1");
    std::fs::create_dir_all(manifest_path.parent().expect("manifest parent"))
        .expect("create manifest parent");
    std::fs::create_dir_all(store.weights_dir()).expect("create weights dir");
    std::fs::write(
        &manifest_path,
        valid_model_manifest_with_sha(TEST_WEIGHT_SHA256),
    )
    .expect("write model manifest");
    std::fs::write(store.weight_path(TEST_WEIGHT_SHA256), TEST_WEIGHT_BYTES)
        .expect("write cached weight blob");

    let result = list_cached_models_in_store(&store).expect("list cached models");

    assert_eq!(result.packages.len(), 1);
    let package = &result.packages[0];
    assert_eq!(package.package.package_id, "demucs@4.0.1");
    assert_eq!(package.package.input, IoType::Audio);
    assert_eq!(package.package.output, IoType::Stems);
    assert_eq!(package.runtime_entry, "demucs_mlx.Separator");
    assert!(package.weights.cached);
    assert_eq!(package.weights.format, "safetensors");
    assert_eq!(package.params.len(), 1);
    assert_eq!(package.params[0].name, "stems");
    assert_eq!(package.params[0].param_type, ParamType::Enum);
    assert_eq!(
        package.params[0].values.as_ref(),
        Some(&vec!["2".to_string(), "4".to_string(), "6".to_string()])
    );
}

#[test]
fn list_cached_models_filters_by_query() {
    let temp = tempfile::tempdir().expect("temp dir");
    let store = ModelStore::new(temp.path());
    let demucs = store.manifest_path("demucs", "4.0.1");
    let whisper = store.manifest_path("whisper", "1.0.0");
    std::fs::create_dir_all(demucs.parent().expect("demucs manifest parent"))
        .expect("create demucs manifest parent");
    std::fs::create_dir_all(whisper.parent().expect("whisper manifest parent"))
        .expect("create whisper manifest parent");
    std::fs::write(&demucs, valid_model_manifest()).expect("write demucs manifest");
    std::fs::write(
        &whisper,
        valid_model_manifest()
            .replace("name = \"demucs\"", "name = \"whisper\"")
            .replace("version = \"4.0.1\"", "version = \"1.0.0\"")
            .replace(
                "description = \"Music source separation into stems\"",
                "description = \"Speech to text\"",
            )
            .replace("output = \"stems\"", "output = \"text\""),
    )
    .expect("write whisper manifest");

    let result = list_cached_models_matching_in_store(
        &store,
        ModelListRequest {
            query: "whisper".to_string(),
        },
    )
    .expect("search cached models");

    assert_eq!(result.packages.len(), 1);
    assert_eq!(result.packages[0].package.package_id, "whisper@1.0.0");
}

#[test]
fn list_model_catalog_searches_configured_registry_sources() {
    let temp = tempfile::tempdir().expect("temp dir");
    let manifest_path = temp.path().join("models/demucs/4.0.1.toml");
    std::fs::create_dir_all(manifest_path.parent().expect("manifest parent"))
        .expect("create manifest parent");
    std::fs::write(&manifest_path, valid_model_manifest()).expect("write registry manifest");
    let store_root = tempfile::tempdir().expect("store dir");
    let store = ModelStore::new(store_root.path());
    let config = Config {
        default_registry_url: temp.path().display().to_string(),
        ..Config::default()
    };

    let result = list_model_catalog_in_store(
        &config,
        &store,
        ModelCatalogListRequest {
            query: "stems".to_string(),
        },
    )
    .expect("list model catalog");

    assert_eq!(result.packages.len(), 1);
    let package = &result.packages[0];
    assert_eq!(package.package.package_id, "demucs@4.0.1");
    assert_eq!(package.source_name.as_deref(), Some("official"));
    assert_eq!(package.package.output, IoType::Stems);
    assert_eq!(package.runtime_entry, "demucs_mlx.Separator");
    assert!(!package.manifest_cached);
    assert!(package.manifest_path.ends_with("models/demucs/4.0.1.toml"));

    let cached_path = store.manifest_path("demucs", "4.0.1");
    std::fs::create_dir_all(cached_path.parent().expect("cached manifest parent"))
        .expect("create cached manifest parent");
    std::fs::write(cached_path, valid_model_manifest()).expect("write cached manifest");

    let cached = list_model_catalog_in_store(
        &config,
        &store,
        ModelCatalogListRequest {
            query: "stems".to_string(),
        },
    )
    .expect("list cached catalog state");

    assert!(cached.packages[0].manifest_cached);
}

#[test]
fn initialize_model_store_creates_expected_directories() {
    let temp = tempfile::tempdir().expect("temp dir");
    let store = ModelStore::new(temp.path().join(".apm"));

    let result = initialize_model_store_in_store(&store).expect("initialize model store");

    assert_eq!(result.layout.root, store.root().display().to_string());
    assert!(store.manifests_dir().is_dir());
    assert!(store.weights_dir().is_dir());
    assert!(store.runtimes_dir().is_dir());
    assert!(store.cache_dir().is_dir());
    assert!(store.logs_dir().is_dir());
}

#[test]
fn cache_model_catalog_manifest_writes_registry_manifest_to_store() {
    let temp = tempfile::tempdir().expect("temp dir");
    let manifest_path = temp.path().join("models/demucs/4.0.1.toml");
    std::fs::create_dir_all(manifest_path.parent().expect("manifest parent"))
        .expect("create manifest parent");
    std::fs::write(&manifest_path, valid_model_manifest()).expect("write registry manifest");
    let config = Config {
        default_registry_url: temp.path().display().to_string(),
        ..Config::default()
    };
    let store_root = tempfile::tempdir().expect("store dir");
    let store = ModelStore::new(store_root.path());

    let result = cache_model_catalog_manifest_in_store(
        &config,
        &store,
        "demucs".to_string(),
        "4.0.1".to_string(),
    )
    .expect("cache catalog model");

    assert_eq!(result.model.package.package_id, "demucs@4.0.1");
    assert!(result
        .manifest_path
        .ends_with("/manifests/demucs/4.0.1.toml"));
    assert!(store.manifest_path("demucs", "4.0.1").exists());
}

#[test]
fn validate_model_manifest_returns_gui_safe_summary() {
    let result = validate_model_manifest(ModelManifestValidationRequest {
        manifest_toml: valid_model_manifest().to_string(),
    })
    .expect("valid manifest should validate");

    assert_eq!(result.package.package_id, "demucs@4.0.1");
    assert_eq!(result.package.runtime_mode, RuntimeMode::NativeMlx);
    assert_eq!(result.package.input, IoType::Audio);
    assert_eq!(result.package.output, IoType::Stems);
    assert_eq!(result.package.parameter_count, 1);
    assert_eq!(result.package.min_memory_gb, 8);
    assert!(result.package.commercial_license);
}

#[test]
fn validate_model_manifest_rejects_invalid_manifest() {
    let error = validate_model_manifest(ModelManifestValidationRequest {
        manifest_toml: "[package]\nname = \"broken\"".to_string(),
    })
    .expect_err("incomplete manifest should fail validation");

    assert!(error
        .to_string()
        .contains("Failed to parse model manifest TOML"));
}

#[test]
fn cache_model_manifest_writes_manifest_and_returns_cached_model() {
    let temp = tempfile::tempdir().expect("temp dir");
    let store = ModelStore::new(temp.path());
    std::fs::create_dir_all(store.weights_dir()).expect("create weights dir");
    std::fs::write(store.weight_path(TEST_WEIGHT_SHA256), TEST_WEIGHT_BYTES)
        .expect("write cached weight blob");

    let result = cache_model_manifest_in_store(
        &store,
        ModelManifestCacheRequest {
            manifest_toml: valid_model_manifest_with_sha(TEST_WEIGHT_SHA256),
        },
    )
    .expect("cache valid model manifest");

    assert_eq!(result.model.package.package_id, "demucs@4.0.1");
    assert_eq!(result.model.package.runtime_mode, RuntimeMode::NativeMlx);
    assert!(result.model.weights.cached);
    assert!(!result.replaced);
    assert!(result
        .manifest_path
        .ends_with("/manifests/demucs/4.0.1.toml"));
    assert!(store.manifest_path("demucs", "4.0.1").exists());

    let listed = list_cached_models_in_store(&store).expect("list cached models");
    assert_eq!(listed.packages.len(), 1);
    assert_eq!(listed.packages[0].package.package_id, "demucs@4.0.1");
}

#[test]
fn cache_model_manifest_reports_replaced_manifest() {
    let temp = tempfile::tempdir().expect("temp dir");
    let store = ModelStore::new(temp.path());

    cache_model_manifest_in_store(
        &store,
        ModelManifestCacheRequest {
            manifest_toml: valid_model_manifest().to_string(),
        },
    )
    .expect("cache first manifest");
    let result = cache_model_manifest_in_store(
        &store,
        ModelManifestCacheRequest {
            manifest_toml: valid_model_manifest().to_string(),
        },
    )
    .expect("replace manifest");

    assert!(result.replaced);
}

#[test]
fn plan_cached_model_run_returns_prepared_runtime_binding() {
    let temp = tempfile::tempdir().expect("temp dir");
    let store = ModelStore::new(temp.path());
    let manifest_toml = valid_model_manifest_with_sha(TEST_WEIGHT_SHA256);
    let manifest =
        crate::model::ModelManifest::from_toml_str(&manifest_toml).expect("valid model manifest");
    cache_model_manifest_in_store(
        &store,
        ModelManifestCacheRequest {
            manifest_toml: manifest_toml.clone(),
        },
    )
    .expect("cache model manifest");
    std::fs::create_dir_all(store.weights_dir()).expect("create weights dir");
    std::fs::write(store.weight_path(TEST_WEIGHT_SHA256), TEST_WEIGHT_BYTES)
        .expect("write weights");
    provision_runtime_adapter(
        &store,
        &manifest,
        &ModelWeightPullResult {
            package_id: "demucs@4.0.1".to_string(),
            source: "hf:mlx-community/demucs-mlx-fp16".to_string(),
            resolved_url: "hf:mlx-community/demucs-mlx-fp16".to_string(),
            sha256: TEST_WEIGHT_SHA256.to_string(),
            path: store.weight_path(TEST_WEIGHT_SHA256).display().to_string(),
            bytes: TEST_WEIGHT_BYTES.len() as u64,
            status: ModelWeightPullStatus::Cached,
        },
    )
    .expect("provision runtime");

    let plan = plan_cached_model_run_in_store(
        &store,
        "demucs",
        "4.0.1",
        ModelRunPlanRequest::new("mix.wav", "stems/"),
    )
    .expect("plan cached model run");

    assert_eq!(plan.package_id, "demucs@4.0.1");
    assert_eq!(plan.adapter, "native-mlx");
    assert_eq!(plan.input_path, "mix.wav");
    assert_eq!(plan.output_path, "stems/");
    assert!(matches!(
        plan.execution,
        ModelRunExecutionReadiness::Blocked {
            blocker: ModelRunExecutionBlocker::AdapterRunnerUnavailable,
            ..
        }
    ));
    assert_eq!(plan.params[0].name, "stems");
    assert_eq!(
        plan.params[0].value,
        ModelRunParamValue::String("4".to_string())
    );
    assert_eq!(plan.params[0].source, ModelRunParamSource::Default);
    assert!(plan.adapter_manifest_path.ends_with("adapter.toml"));
}

#[test]
fn cache_model_manifest_rejects_invalid_manifest() {
    let temp = tempfile::tempdir().expect("temp dir");
    let store = ModelStore::new(temp.path());
    let error = cache_model_manifest_in_store(
        &store,
        ModelManifestCacheRequest {
            manifest_toml: "[package]\nname = \"../demucs\"".to_string(),
        },
    )
    .expect_err("invalid manifest should fail");

    assert!(matches!(error, ModelManifestCacheError::Invalid(_)));
}

#[test]
fn service_health_uses_runtime_bind_and_paths() {
    let temp = tempfile::tempdir().expect("temp dir");
    let config = Config {
        data_dir: Some(temp.path().join("data")),
        cache_dir: Some(temp.path().join("cache")),
        ..Config::default()
    };

    let health = service_health(
        &config,
        RuntimeBind {
            host: "127.0.0.1".to_string(),
            port: 4767,
        },
    );

    assert_eq!(health.status, "ok");
    assert_eq!(health.bind.host, "127.0.0.1");
    assert_eq!(health.bind.port, 4767);
    assert!(health.data_dir.ends_with("/data"));
    assert!(health.cache_dir.ends_with("/cache"));
    assert!(health.model_store.root.ends_with(".apm"));
    assert!(health.auth.required);
    assert_eq!(health.auth.header, LOOPBACK_TOKEN_HEADER);
    assert!(health.auth.token_file.ends_with("/data/service/token.json"));
}

#[test]
fn operation_accepted_points_to_status_route() {
    let accepted = operation_accepted("op-1".to_string(), OperationKind::RegistrySync);

    assert_eq!(accepted.operation_id, "op-1");
    assert_eq!(accepted.kind, OperationKind::RegistrySync);
    assert_eq!(accepted.status_url, "/v1/operations/op-1");
}

#[test]
fn operation_request_reports_matching_kind() {
    assert_eq!(
        OperationRequest::RegistrySync.kind(),
        OperationKind::RegistrySync
    );
    assert_eq!(
        OperationRequest::LibraryScan.kind(),
        OperationKind::LibraryScan
    );
    assert_eq!(
        (OperationRequest::InstallUrl {
            request: InstallPackageRequest::default(),
        })
        .kind(),
        OperationKind::InstallUrl
    );
    assert_eq!(
        (OperationRequest::InstallArchive {
            request: InstallPackageRequest::default(),
        })
        .kind(),
        OperationKind::InstallArchive
    );
    assert_eq!(
        (OperationRequest::PackageUpdate {
            slug: "surge-xt".to_string(),
            body: PackageUpdateBody::default(),
        })
        .kind(),
        OperationKind::PackageUpdate
    );
    assert_eq!(
        (OperationRequest::PackageRemove {
            slug: "surge-xt".to_string(),
            body: PackageRemoveBody::default(),
        })
        .kind(),
        OperationKind::PackageRemove
    );
    assert_eq!(
        (OperationRequest::ModelWeightPull {
            name: "demucs".to_string(),
            version: "4.0.1".to_string(),
        })
        .kind(),
        OperationKind::ModelWeightPull
    );
    assert_eq!(
        (OperationRequest::ModelInstall {
            name: "demucs".to_string(),
            version: "4.0.1".to_string(),
        })
        .kind(),
        OperationKind::ModelInstall
    );
    assert_eq!(
        (OperationRequest::ModelRun {
            name: "demucs".to_string(),
            version: "4.0.1".to_string(),
            request: ModelRunPlanRequest::new("mix.wav", "stems/"),
        })
        .kind(),
        OperationKind::ModelRun
    );
}

fn valid_model_manifest() -> String {
    valid_model_manifest_with_sha(
        "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
    )
}

fn valid_model_manifest_with_sha(sha256: &str) -> String {
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
sha256 = "{sha256}"
format = "safetensors"

[io]
input = "audio"
output = "stems"

[[params]]
name = "stems"
type = "enum"
values = ["2", "4", "6"]
default = "4"

[license]
spdx = "MIT"
commercial = true

[hardware]
min_memory_gb = 8
"#
    )
}
