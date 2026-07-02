use super::*;

#[test]
fn provisions_native_mlx_runtime_metadata() {
    let temp = tempfile::tempdir().expect("temp dir");
    let store = ModelStore::new(temp.path());
    let manifest = ModelManifest::from_toml_str(&test_manifest("native-mlx", "")).unwrap();
    let weights = test_weights(&store);

    let result = provision_runtime_adapter(&store, &manifest, &weights).expect("provision runtime");

    assert_eq!(result.adapter, "native-mlx");
    assert_eq!(result.status, RuntimeProvisioningStatus::Prepared);
    assert_eq!(result.runtime_mode, RuntimeMode::NativeMlx);
    assert!(result
        .runtime_dir
        .ends_with("runtimes/native-mlx/demucs/4.0.1"));
    assert_eq!(result.files.len(), 1);
    let adapter_toml = std::fs::read_to_string(&result.files[0]).expect("adapter toml");
    assert!(adapter_toml.contains("package_id = \"demucs@4.0.1\""));
    assert!(adapter_toml.contains("adapter = \"native-mlx\""));
}

#[test]
fn provisions_python_requirements_metadata() {
    let temp = tempfile::tempdir().expect("temp dir");
    let store = ModelStore::new(temp.path());
    let manifest = ModelManifest::from_toml_str(&test_manifest(
        "python-env",
        r#"
repo = "https://example.test/model.git"
python_version = "3.11"
requirements = "torch==2.4.0"
"#,
    ))
    .unwrap();
    let weights = test_weights(&store);

    let result = provision_runtime_adapter(&store, &manifest, &weights).expect("provision runtime");

    assert_eq!(result.adapter, "python-env");
    assert_eq!(result.files.len(), 2);
    let requirements = result
        .files
        .iter()
        .find(|path| path.ends_with("requirements.txt"))
        .expect("requirements file");
    assert_eq!(
        std::fs::read_to_string(requirements).expect("requirements content"),
        "torch==2.4.0"
    );
}

#[test]
fn plans_run_from_prepared_runtime_metadata() {
    let temp = tempfile::tempdir().expect("temp dir");
    let store = ModelStore::new(temp.path());
    let manifest_toml = test_manifest("native-mlx", "");
    let manifest = ModelManifest::from_toml_str(&manifest_toml).unwrap();
    let weights = test_weights(&store);
    store
        .cache_manifest(&manifest, &manifest_toml)
        .expect("cache manifest");
    std::fs::create_dir_all(store.weights_dir()).expect("create weights dir");
    std::fs::write(store.weight_path(&weights.sha256), b"weights").expect("write weights");
    provision_runtime_adapter(&store, &manifest, &weights).expect("provision runtime");

    let plan = plan_model_run(
        &store,
        "demucs",
        "4.0.1",
        ModelRunPlanRequest::new("mix.wav", "stems/"),
    )
    .expect("plan model run");

    assert_eq!(plan.package_id, "demucs@4.0.1");
    assert_eq!(plan.status, ModelRunPlanStatus::Planned);
    assert_eq!(plan.runtime_mode, RuntimeMode::NativeMlx);
    assert_eq!(plan.runtime_entry, "demucs.Model");
    assert_eq!(plan.adapter, "native-mlx");
    assert!(plan.adapter_manifest_path.ends_with("adapter.toml"));
    assert_eq!(plan.input_path, "mix.wav");
    assert_eq!(plan.output_path, "stems/");
    assert!(matches!(
        plan.execution,
        ModelRunExecutionReadiness::Blocked {
            blocker: ModelRunExecutionBlocker::AdapterRunnerUnavailable,
            ..
        }
    ));
    assert!(plan.execution.message().contains("native-mlx execution"));
    assert!(plan.message.contains("Runtime execution is pending"));
}

#[test]
fn run_plan_rejects_stale_runtime_metadata() {
    let temp = tempfile::tempdir().expect("temp dir");
    let store = ModelStore::new(temp.path());
    let manifest_toml = test_manifest("native-mlx", "");
    let manifest = ModelManifest::from_toml_str(&manifest_toml).unwrap();
    let weights = test_weights(&store);
    store
        .cache_manifest(&manifest, &manifest_toml)
        .expect("cache manifest");
    std::fs::create_dir_all(store.weights_dir()).expect("create weights dir");
    std::fs::write(store.weight_path(&weights.sha256), b"weights").expect("write weights");
    let provisioning =
        provision_runtime_adapter(&store, &manifest, &weights).expect("provision runtime");
    let adapter_toml = std::fs::read_to_string(&provisioning.files[0]).expect("adapter toml");
    std::fs::write(
        &provisioning.files[0],
        adapter_toml.replace("adapter = \"native-mlx\"", "adapter = \"coreml\""),
    )
    .expect("tamper adapter");

    let error = plan_model_run(
        &store,
        "demucs",
        "4.0.1",
        ModelRunPlanRequest::new("mix.wav", "stems/"),
    )
    .expect_err("stale runtime metadata should fail");

    assert!(error.to_string().contains("adapter 'coreml'"));
    assert!(error.to_string().contains("expected 'native-mlx'"));
}

#[test]
fn plans_run_with_validated_param_bindings() {
    let temp = tempfile::tempdir().expect("temp dir");
    let store = ModelStore::new(temp.path());
    let manifest_toml = test_manifest("native-mlx", &test_params());
    let manifest = ModelManifest::from_toml_str(&manifest_toml).unwrap();
    let weights = test_weights(&store);
    store
        .cache_manifest(&manifest, &manifest_toml)
        .expect("cache manifest");
    std::fs::create_dir_all(store.weights_dir()).expect("create weights dir");
    std::fs::write(store.weight_path(&weights.sha256), b"weights").expect("write weights");
    provision_runtime_adapter(&store, &manifest, &weights).expect("provision runtime");
    let mut request = ModelRunPlanRequest::new("mix.wav", "stems/");
    request.params.insert(
        "stems".to_string(),
        ModelRunParamValue::String("2".to_string()),
    );
    request.params.insert(
        "shifts".to_string(),
        ModelRunParamValue::String("3".to_string()),
    );

    let plan = plan_model_run(&store, "demucs", "4.0.1", request).expect("plan model run");

    assert_eq!(plan.params.len(), 3);
    assert_eq!(plan.params[0].name, "stems");
    assert_eq!(
        plan.params[0].value,
        ModelRunParamValue::String("2".to_string())
    );
    assert_eq!(plan.params[0].source, ModelRunParamSource::Request);
    assert_eq!(plan.params[1].name, "shifts");
    assert_eq!(plan.params[1].value, ModelRunParamValue::Integer(3));
    assert_eq!(plan.params[1].source, ModelRunParamSource::Request);
    assert_eq!(plan.params[2].name, "normalize");
    assert_eq!(plan.params[2].value, ModelRunParamValue::Boolean(true));
    assert_eq!(plan.params[2].source, ModelRunParamSource::Default);
}

#[test]
fn run_plan_rejects_invalid_param_bindings() {
    let temp = tempfile::tempdir().expect("temp dir");
    let store = ModelStore::new(temp.path());
    let manifest_toml = test_manifest("native-mlx", &test_params());
    let manifest = ModelManifest::from_toml_str(&manifest_toml).unwrap();
    let weights = test_weights(&store);
    store
        .cache_manifest(&manifest, &manifest_toml)
        .expect("cache manifest");
    std::fs::create_dir_all(store.weights_dir()).expect("create weights dir");
    std::fs::write(store.weight_path(&weights.sha256), b"weights").expect("write weights");
    provision_runtime_adapter(&store, &manifest, &weights).expect("provision runtime");
    let mut request = ModelRunPlanRequest::new("mix.wav", "stems/");
    request
        .params
        .insert("shifts".to_string(), ModelRunParamValue::Integer(99));

    let error = plan_model_run(&store, "demucs", "4.0.1", request)
        .expect_err("plan should reject out-of-range params");

    assert!(error.to_string().contains("above max 10"));
}

#[test]
fn run_plan_requires_prepared_runtime_metadata() {
    let temp = tempfile::tempdir().expect("temp dir");
    let store = ModelStore::new(temp.path());
    let manifest_toml = test_manifest("native-mlx", "");
    let manifest = ModelManifest::from_toml_str(&manifest_toml).unwrap();
    let weights = test_weights(&store);
    store
        .cache_manifest(&manifest, &manifest_toml)
        .expect("cache manifest");
    std::fs::create_dir_all(store.weights_dir()).expect("create weights dir");
    std::fs::write(store.weight_path(&weights.sha256), b"weights").expect("write weights");

    let error = plan_model_run(
        &store,
        "demucs",
        "4.0.1",
        ModelRunPlanRequest::new("mix.wav", "stems/"),
    )
    .expect_err("plan should require runtime metadata");

    assert!(error.to_string().contains("runtime adapter metadata"));
}

fn test_params() -> String {
    r#"
[[params]]
name = "stems"
type = "enum"
values = ["2", "4", "6"]
default = "4"

[[params]]
name = "shifts"
type = "int"
min = 1
max = 10
default = 1

[[params]]
name = "normalize"
type = "bool"
default = true
"#
    .to_string()
}

fn test_weights(store: &ModelStore) -> ModelWeightPullResult {
    let sha256 = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    ModelWeightPullResult {
        package_id: "demucs@4.0.1".to_string(),
        source: "https://example.test/model.safetensors".to_string(),
        resolved_url: "https://example.test/model.safetensors".to_string(),
        sha256: sha256.to_string(),
        path: store.weight_path(sha256).display().to_string(),
        bytes: 7,
        status: super::super::ModelWeightPullStatus::Cached,
    }
}

fn test_manifest(mode: &str, extra_runtime: &str) -> String {
    format!(
        r#"
[package]
name = "demucs"
version = "4.0.1"
description = "Music source separation into stems"
publisher = "apm-core"

[runtime]
mode = "{mode}"
entry = "demucs.Model"
{extra_runtime}

[weights]
source = "https://example.test/model.safetensors"
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
"#
    )
}
