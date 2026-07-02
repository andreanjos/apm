use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};

use super::runtime::{bind_run_params, blocked_execution_readiness, prepared_runtime_adapter};
use super::{
    IoType, ModelManifest, ModelRunExecutionReadiness, ModelRunParamBinding, ModelRunParamValue,
    ModelStore, RuntimeMode,
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelChainPlanRequest {
    pub input_path: String,
    pub output_path: String,
    pub steps: Vec<ModelChainStepRequest>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelChainStepRequest {
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub params: BTreeMap<String, ModelRunParamValue>,
}

impl ModelChainStepRequest {
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
            params: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelChainPlan {
    pub status: ModelChainPlanStatus,
    pub input_path: String,
    pub output_path: String,
    pub input: IoType,
    pub output: IoType,
    pub steps: Vec<ModelChainStepPlan>,
    pub edges: Vec<ModelChainEdgePlan>,
    pub execution: ModelChainExecutionReadiness,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelChainPlanStatus {
    Planned,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelChainStepPlan {
    pub step_index: usize,
    pub package_id: String,
    pub runtime_mode: RuntimeMode,
    pub runtime_entry: String,
    pub adapter: String,
    pub input: IoType,
    pub output: IoType,
    pub weights_path: String,
    pub runtime_dir: String,
    pub adapter_manifest_path: String,
    #[serde(default)]
    pub params: Vec<ModelRunParamBinding>,
    pub execution: ModelRunExecutionReadiness,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelChainEdgePlan {
    pub from_step_index: usize,
    pub to_step_index: usize,
    pub from_output: IoType,
    pub to_input: IoType,
    pub binding: ModelChainIoBinding,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelChainIoBinding {
    Direct,
    StemSelectionRequired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ModelChainExecutionReadiness {
    Blocked {
        blocker: ModelChainExecutionBlocker,
        message: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelChainExecutionBlocker {
    ChainRunnerUnavailable,
}

pub fn plan_model_chain(
    store: &ModelStore,
    request: ModelChainPlanRequest,
) -> Result<ModelChainPlan> {
    if request.input_path.trim().is_empty() {
        bail!("model chain input_path must not be empty");
    }
    if request.output_path.trim().is_empty() {
        bail!("model chain output_path must not be empty");
    }
    if request.steps.is_empty() {
        bail!("model chain must include at least one step");
    }

    let mut steps = Vec::with_capacity(request.steps.len());
    for (index, step) in request.steps.into_iter().enumerate() {
        steps.push(plan_chain_step(store, index, step)?);
    }

    let edges = chain_edges(&steps)?;
    let input = steps
        .first()
        .expect("empty chain rejected before planning")
        .input;
    let output = steps
        .last()
        .expect("empty chain rejected before planning")
        .output;
    let step_count = steps.len();
    let edge_count = edges.len();

    Ok(ModelChainPlan {
        status: ModelChainPlanStatus::Planned,
        input_path: request.input_path,
        output_path: request.output_path,
        input,
        output,
        steps,
        edges,
        execution: blocked_chain_execution_readiness(step_count),
        message: format!(
            "Runtime chain execution is pending; this plan validates {step_count} prepared step{} and {edge_count} IO edge{}.",
            if step_count == 1 { "" } else { "s" },
            if edge_count == 1 { "" } else { "s" },
        ),
    })
}

fn plan_chain_step(
    store: &ModelStore,
    step_index: usize,
    request: ModelChainStepRequest,
) -> Result<ModelChainStepPlan> {
    let manifest_path = store.cached_manifest_path(&request.name, &request.version)?;
    let manifest = ModelManifest::from_path(&manifest_path).with_context(|| {
        format!(
            "Cannot read cached model manifest for chain step {step_index}: {}",
            manifest_path.display()
        )
    })?;
    let params = bind_run_params(&manifest, request.params)?;
    let weights_path = store.weight_path(&manifest.weights.sha256);
    if !weights_path.exists() {
        bail!(
            "model weights are not cached for chain step {step_index} {}; run `apm model install {}` first",
            manifest.package_id(),
            manifest.package_id()
        );
    }

    let package_id = manifest.package_id();
    let runtime = prepared_runtime_adapter(store, &manifest, &weights_path).map_err(|error| {
        anyhow!("Cannot bind runtime adapter metadata for chain step {step_index} {package_id}: {error}")
    })?;
    let adapter = runtime.adapter;
    Ok(ModelChainStepPlan {
        step_index,
        package_id: package_id.clone(),
        runtime_mode: manifest.runtime.mode,
        runtime_entry: manifest.runtime.entry,
        adapter: adapter.clone(),
        input: manifest.io.input,
        output: manifest.io.output,
        weights_path: path_string(&weights_path),
        runtime_dir: path_string(&runtime.runtime_dir),
        adapter_manifest_path: path_string(&runtime.adapter_manifest_path),
        params,
        execution: blocked_execution_readiness(&package_id, &adapter),
    })
}

fn chain_edges(steps: &[ModelChainStepPlan]) -> Result<Vec<ModelChainEdgePlan>> {
    let mut edges = Vec::new();
    for pair in steps.windows(2) {
        edges.push(chain_edge(&pair[0], &pair[1])?);
    }
    Ok(edges)
}

fn chain_edge(left: &ModelChainStepPlan, right: &ModelChainStepPlan) -> Result<ModelChainEdgePlan> {
    let Some(binding) = chain_io_binding(left.output, right.input) else {
        bail!(
            "model chain IO mismatch: step {} {} outputs {}, but step {} {} requires {}",
            left.step_index,
            left.package_id,
            left.output,
            right.step_index,
            right.package_id,
            right.input
        );
    };

    let message = match binding {
        ModelChainIoBinding::Direct => format!(
            "Step {} {} output feeds step {} {} input directly.",
            left.step_index, left.output, right.step_index, right.input
        ),
        ModelChainIoBinding::StemSelectionRequired => format!(
            "Step {} outputs stems; select one audio stem before step {}.",
            left.step_index, right.step_index
        ),
    };

    Ok(ModelChainEdgePlan {
        from_step_index: left.step_index,
        to_step_index: right.step_index,
        from_output: left.output,
        to_input: right.input,
        binding,
        message,
    })
}

fn chain_io_binding(output: IoType, input: IoType) -> Option<ModelChainIoBinding> {
    if output == input {
        return Some(ModelChainIoBinding::Direct);
    }
    if output == IoType::Stems && input == IoType::Audio {
        return Some(ModelChainIoBinding::StemSelectionRequired);
    }
    None
}

fn blocked_chain_execution_readiness(step_count: usize) -> ModelChainExecutionReadiness {
    ModelChainExecutionReadiness::Blocked {
        blocker: ModelChainExecutionBlocker::ChainRunnerUnavailable,
        message: format!(
            "Chain execution for {step_count} prepared step{} is not implemented yet; this plan is review-only.",
            if step_count == 1 { "" } else { "s" },
        ),
    }
}

fn path_string(path: &Path) -> String {
    path.display().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{provision_runtime_adapter, ModelWeightPullResult, ModelWeightPullStatus};

    const TEST_WEIGHT_BYTES: &[u8] = b"weights";

    #[test]
    fn plans_prepared_chain_with_stem_selection_edge() {
        let temp = tempfile::tempdir().expect("temp dir");
        let store = ModelStore::new(temp.path());
        cache_prepared_manifest(
            &store,
            &test_manifest(
                "demucs",
                "4.0.1",
                IoType::Audio,
                IoType::Stems,
                "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
            ),
        );
        cache_prepared_manifest(
            &store,
            &test_manifest(
                "whisper",
                "3.0.0",
                IoType::Audio,
                IoType::Text,
                "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210",
            ),
        );

        let mut whisper = ModelChainStepRequest::new("whisper", "3.0.0");
        whisper.params.insert(
            "language".to_string(),
            ModelRunParamValue::String("en".to_string()),
        );
        let plan = plan_model_chain(
            &store,
            ModelChainPlanRequest {
                input_path: "mix.wav".to_string(),
                output_path: "lyrics.txt".to_string(),
                steps: vec![ModelChainStepRequest::new("demucs", "4.0.1"), whisper],
            },
        )
        .expect("plan chain");

        assert_eq!(plan.status, ModelChainPlanStatus::Planned);
        assert_eq!(plan.input, IoType::Audio);
        assert_eq!(plan.output, IoType::Text);
        assert_eq!(plan.steps.len(), 2);
        assert_eq!(plan.steps[0].package_id, "demucs@4.0.1");
        assert_eq!(plan.steps[1].package_id, "whisper@3.0.0");
        assert_eq!(plan.steps[1].params[0].name, "language");
        assert_eq!(
            plan.steps[1].params[0].value,
            ModelRunParamValue::String("en".to_string())
        );
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

    #[test]
    fn rejects_incompatible_chain_io() {
        let temp = tempfile::tempdir().expect("temp dir");
        let store = ModelStore::new(temp.path());
        cache_prepared_manifest(
            &store,
            &test_manifest(
                "demucs",
                "4.0.1",
                IoType::Audio,
                IoType::Stems,
                "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
            ),
        );
        cache_prepared_manifest(
            &store,
            &test_manifest(
                "midi",
                "1.0.0",
                IoType::Midi,
                IoType::Text,
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            ),
        );

        let error = plan_model_chain(
            &store,
            ModelChainPlanRequest {
                input_path: "mix.wav".to_string(),
                output_path: "notes.txt".to_string(),
                steps: vec![
                    ModelChainStepRequest::new("demucs", "4.0.1"),
                    ModelChainStepRequest::new("midi", "1.0.0"),
                ],
            },
        )
        .expect_err("mismatched chain should fail");

        assert!(error.to_string().contains("IO mismatch"));
    }

    #[test]
    fn requires_prepared_runtime_metadata() {
        let temp = tempfile::tempdir().expect("temp dir");
        let store = ModelStore::new(temp.path());
        let manifest = ModelManifest::from_toml_str(&test_manifest(
            "demucs",
            "4.0.1",
            IoType::Audio,
            IoType::Stems,
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        ))
        .expect("manifest");
        store
            .cache_manifest(
                &manifest,
                &test_manifest(
                    "demucs",
                    "4.0.1",
                    IoType::Audio,
                    IoType::Stems,
                    "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
                ),
            )
            .expect("cache manifest");
        std::fs::create_dir_all(store.weights_dir()).expect("weights dir");
        std::fs::write(
            store.weight_path(&manifest.weights.sha256),
            TEST_WEIGHT_BYTES,
        )
        .expect("write weight");

        let error = plan_model_chain(
            &store,
            ModelChainPlanRequest {
                input_path: "mix.wav".to_string(),
                output_path: "stems/".to_string(),
                steps: vec![ModelChainStepRequest::new("demucs", "4.0.1")],
            },
        )
        .expect_err("missing runtime metadata should fail");

        assert!(error.to_string().contains("runtime adapter metadata"));
    }

    fn cache_prepared_manifest(store: &ModelStore, manifest_toml: &str) -> ModelManifest {
        let manifest = ModelManifest::from_toml_str(manifest_toml).expect("manifest");
        store
            .cache_manifest(&manifest, manifest_toml)
            .expect("cache manifest");
        std::fs::create_dir_all(store.weights_dir()).expect("weights dir");
        std::fs::write(
            store.weight_path(&manifest.weights.sha256),
            TEST_WEIGHT_BYTES,
        )
        .expect("write weight");
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
        manifest
    }

    fn test_manifest(
        name: &str,
        version: &str,
        input: IoType,
        output: IoType,
        sha256: &str,
    ) -> String {
        let params = if name == "whisper" {
            r#"
[[params]]
name = "language"
type = "string"
default = "auto"
"#
        } else {
            ""
        };
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
{params}
[license]
spdx = "MIT"
commercial = true

[hardware]
min_memory_gb = 8
"#
        )
    }
}
