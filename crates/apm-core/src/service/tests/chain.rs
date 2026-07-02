use crate::model::{
    provision_runtime_adapter, IoType, ModelChainExecutionBlocker, ModelChainExecutionReadiness,
    ModelChainIoBinding, ModelChainPlanRequest, ModelChainStepRequest, ModelManifest, ModelStore,
    ModelWeightPullResult, ModelWeightPullStatus,
};

use super::*;

#[test]
fn contract_marks_model_chain_plan_as_available() {
    let contract = local_service_contract();
    let endpoint = contract
        .endpoints
        .iter()
        .find(|endpoint| endpoint.id == "model.chain.plan")
        .expect("model chain plan endpoint should exist");

    assert_eq!(endpoint.runtime, ServiceEndpointRuntime::Available);
    assert_eq!(endpoint.method, HttpMethod::Post);
    assert_eq!(endpoint.request.as_deref(), Some("ModelChainPlanRequest"));
    assert_eq!(endpoint.response, "ModelChainPlan");
}

#[test]
fn plan_cached_model_chain_validates_prepared_io_edges() {
    let temp = tempfile::tempdir().expect("temp dir");
    let store = ModelStore::new(temp.path());
    cache_and_prepare_model(&store, &valid_model_manifest_with_sha(TEST_WEIGHT_SHA256));
    cache_and_prepare_model(
        &store,
        &valid_model_manifest_for_io(
            "whisper",
            "3.0.0",
            IoType::Audio,
            IoType::Text,
            "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210",
        ),
    );

    let plan = plan_cached_model_chain_in_store(
        &store,
        ModelChainPlanRequest {
            input_path: "mix.wav".to_string(),
            output_path: "lyrics.txt".to_string(),
            steps: vec![
                ModelChainStepRequest::new("demucs", "4.0.1"),
                ModelChainStepRequest::new("whisper", "3.0.0"),
            ],
        },
    )
    .expect("plan cached model chain");

    assert_eq!(plan.input, IoType::Audio);
    assert_eq!(plan.output, IoType::Text);
    assert_eq!(plan.steps.len(), 2);
    assert_eq!(plan.steps[0].package_id, "demucs@4.0.1");
    assert_eq!(plan.steps[1].package_id, "whisper@3.0.0");
    assert_eq!(plan.edges.len(), 1);
    assert_eq!(
        plan.edges[0].binding,
        ModelChainIoBinding::StemSelectionRequired
    );
    assert!(matches!(
        plan.execution,
        ModelChainExecutionReadiness::Blocked {
            blocker: ModelChainExecutionBlocker::ChainRunnerUnavailable,
            ..
        }
    ));
}

fn valid_model_manifest_for_io(
    name: &str,
    version: &str,
    input: IoType,
    output: IoType,
    sha256: &str,
) -> String {
    format!(
        r#"
[package]
name = "{name}"
version = "{version}"
description = "{name} model"
publisher = "apm-core"

[runtime]
mode = "native-mlx"
entry = "{name}.Model"

[weights]
source = "https://example.test/{name}.safetensors"
sha256 = "{sha256}"
format = "safetensors"

[io]
input = "{input}"
output = "{output}"

[license]
spdx = "MIT"
commercial = true

[hardware]
min_memory_gb = 8
"#
    )
}

fn cache_and_prepare_model(store: &ModelStore, manifest_toml: &str) {
    let manifest = ModelManifest::from_toml_str(manifest_toml).expect("valid model manifest");
    cache_model_manifest_in_store(
        store,
        ModelManifestCacheRequest {
            manifest_toml: manifest_toml.to_string(),
        },
    )
    .expect("cache model manifest");
    std::fs::create_dir_all(store.weights_dir()).expect("create weights dir");
    std::fs::write(
        store.weight_path(&manifest.weights.sha256),
        TEST_WEIGHT_BYTES,
    )
    .expect("write weights");
    provision_runtime_adapter(
        store,
        &manifest,
        &ModelWeightPullResult {
            package_id: manifest.package_id(),
            source: manifest.weights.source.clone(),
            resolved_url: manifest.weights.source.clone(),
            sha256: manifest.weights.sha256.clone(),
            path: store
                .weight_path(&manifest.weights.sha256)
                .display()
                .to_string(),
            bytes: TEST_WEIGHT_BYTES.len() as u64,
            status: ModelWeightPullStatus::Cached,
        },
    )
    .expect("provision runtime");
}
