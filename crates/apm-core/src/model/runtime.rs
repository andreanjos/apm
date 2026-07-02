use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::config::ensure_dir;

use super::{ModelManifest, ModelStore, ModelWeightPullResult, ParamType, Parameter, RuntimeMode};

pub trait RuntimeAdapter {
    fn adapter_id(&self) -> &'static str;

    fn provision(
        &self,
        store: &ModelStore,
        manifest: &ModelManifest,
        weights: &ModelWeightPullResult,
    ) -> Result<ModelRuntimeProvisioning>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelRuntimeProvisioning {
    pub adapter: String,
    pub status: RuntimeProvisioningStatus,
    pub runtime_mode: RuntimeMode,
    pub runtime_entry: String,
    pub runtime_dir: String,
    pub files: Vec<String>,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PreparedRuntimeAdapter {
    pub adapter: String,
    pub runtime_dir: PathBuf,
    pub adapter_manifest_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelRunPlanRequest {
    pub input_path: String,
    pub output_path: String,
    #[serde(default)]
    pub params: BTreeMap<String, ModelRunParamValue>,
}

impl ModelRunPlanRequest {
    pub fn new(input_path: impl Into<String>, output_path: impl Into<String>) -> Self {
        Self {
            input_path: input_path.into(),
            output_path: output_path.into(),
            params: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelRunPlan {
    pub package_id: String,
    pub status: ModelRunPlanStatus,
    pub runtime_mode: RuntimeMode,
    pub runtime_entry: String,
    pub adapter: String,
    pub runtime_dir: String,
    pub adapter_manifest_path: String,
    pub weights_path: String,
    pub input_path: String,
    pub output_path: String,
    #[serde(default)]
    pub params: Vec<ModelRunParamBinding>,
    pub execution: ModelRunExecutionReadiness,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ModelRunExecutionReadiness {
    Ready {
        message: String,
    },
    Blocked {
        blocker: ModelRunExecutionBlocker,
        message: String,
    },
}

impl ModelRunExecutionReadiness {
    pub fn message(&self) -> &str {
        match self {
            Self::Ready { message } => message,
            Self::Blocked { message, .. } => message,
        }
    }

    pub fn is_ready(&self) -> bool {
        matches!(self, Self::Ready { .. })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelRunExecutionBlocker {
    AdapterRunnerUnavailable,
}

impl fmt::Display for ModelRunExecutionBlocker {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AdapterRunnerUnavailable => f.write_str("adapter_runner_unavailable"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelRunParamBinding {
    pub name: String,
    pub value: ModelRunParamValue,
    pub source: ModelRunParamSource,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ModelRunParamValue {
    String(String),
    Integer(i64),
    Float(f64),
    Boolean(bool),
}

impl ModelRunParamValue {
    pub(crate) fn from_toml(value: toml::Value) -> Result<Self> {
        match value {
            toml::Value::String(value) => Ok(Self::String(value)),
            toml::Value::Integer(value) => Ok(Self::Integer(value)),
            toml::Value::Float(value) => Ok(Self::Float(value)),
            toml::Value::Boolean(value) => Ok(Self::Boolean(value)),
            other => bail!(
                "model run parameter default cannot be {}",
                toml_value_type(&other)
            ),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelRunParamSource {
    Default,
    Request,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelRunPlanStatus {
    Planned,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeProvisioningStatus {
    Prepared,
}

impl fmt::Display for RuntimeProvisioningStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Prepared => f.write_str("prepared"),
        }
    }
}

pub fn provision_runtime_adapter(
    store: &ModelStore,
    manifest: &ModelManifest,
    weights: &ModelWeightPullResult,
) -> Result<ModelRuntimeProvisioning> {
    match manifest.runtime.mode {
        RuntimeMode::NativeMlx => NativeMlxAdapter.provision(store, manifest, weights),
        RuntimeMode::Coreml => CoreMlAdapter.provision(store, manifest, weights),
        RuntimeMode::PythonEnv => PythonEnvAdapter.provision(store, manifest, weights),
    }
}

pub fn plan_model_run(
    store: &ModelStore,
    name: &str,
    version: &str,
    request: ModelRunPlanRequest,
) -> Result<ModelRunPlan> {
    if request.input_path.trim().is_empty() {
        bail!("model run input_path must not be empty");
    }
    if request.output_path.trim().is_empty() {
        bail!("model run output_path must not be empty");
    }

    let manifest_path = store.cached_manifest_path(name, version)?;
    let manifest = ModelManifest::from_path(&manifest_path).with_context(|| {
        format!(
            "Cannot read cached model manifest: {}",
            manifest_path.display()
        )
    })?;
    let params = bind_run_params(&manifest, request.params)?;
    let weights_path = store.weight_path(&manifest.weights.sha256);
    if !weights_path.exists() {
        bail!(
            "model weights are not cached for {}; run `apm model install {}` first",
            manifest.package_id(),
            manifest.package_id()
        );
    }

    let package_id = manifest.package_id();
    let runtime = prepared_runtime_adapter(store, &manifest, &weights_path)?;
    let adapter = runtime.adapter;
    Ok(ModelRunPlan {
        package_id: package_id.clone(),
        status: ModelRunPlanStatus::Planned,
        runtime_mode: manifest.runtime.mode,
        runtime_entry: manifest.runtime.entry,
        adapter: adapter.clone(),
        runtime_dir: path_string(&runtime.runtime_dir),
        adapter_manifest_path: path_string(&runtime.adapter_manifest_path),
        weights_path: path_string(&weights_path),
        input_path: request.input_path,
        output_path: request.output_path,
        params,
        execution: blocked_execution_readiness(&package_id, &adapter),
        message: format!(
            "Runtime execution is pending; this plan binds prepared adapter metadata for {package_id}."
        ),
    })
}

pub(crate) fn prepared_runtime_adapter(
    store: &ModelStore,
    manifest: &ModelManifest,
    weights_path: &Path,
) -> Result<PreparedRuntimeAdapter> {
    let runtime_dir = store.runtime_package_dir(
        &manifest.runtime.mode.to_string(),
        &manifest.package.name,
        &manifest.package.version,
    )?;
    let adapter_manifest_path = runtime_dir.join("adapter.toml");
    if !adapter_manifest_path.exists() {
        bail!(
            "runtime adapter metadata is not prepared for {}; run `apm model install {}` first",
            manifest.package_id(),
            manifest.package_id()
        );
    }

    let record = read_runtime_adapter_record(&adapter_manifest_path)?;
    validate_runtime_adapter_record(&record, manifest, weights_path)?;

    Ok(PreparedRuntimeAdapter {
        adapter: record.adapter,
        runtime_dir,
        adapter_manifest_path,
    })
}

pub(crate) fn blocked_execution_readiness(
    package_id: &str,
    adapter: &str,
) -> ModelRunExecutionReadiness {
    ModelRunExecutionReadiness::Blocked {
        blocker: ModelRunExecutionBlocker::AdapterRunnerUnavailable,
        message: format!(
            "{adapter} execution for {package_id} is not implemented yet; this plan is review-only."
        ),
    }
}

pub(crate) fn bind_run_params(
    manifest: &ModelManifest,
    mut requested: BTreeMap<String, ModelRunParamValue>,
) -> Result<Vec<ModelRunParamBinding>> {
    for name in requested.keys() {
        if !manifest.params.iter().any(|param| &param.name == name) {
            bail!("unknown model run parameter '{name}'");
        }
    }

    let mut bindings = Vec::new();
    for param in &manifest.params {
        if let Some(value) = requested.remove(&param.name) {
            bindings.push(bind_run_param(param, value, ModelRunParamSource::Request)?);
        } else if let Some(default) = param.default.clone() {
            let value = ModelRunParamValue::from_toml(default)?;
            bindings.push(bind_run_param(param, value, ModelRunParamSource::Default)?);
        }
    }
    Ok(bindings)
}

fn bind_run_param(
    param: &Parameter,
    value: ModelRunParamValue,
    source: ModelRunParamSource,
) -> Result<ModelRunParamBinding> {
    Ok(ModelRunParamBinding {
        name: param.name.clone(),
        value: normalize_run_param_value(param, value)?,
        source,
    })
}

fn normalize_run_param_value(
    param: &Parameter,
    value: ModelRunParamValue,
) -> Result<ModelRunParamValue> {
    match param.param_type {
        ParamType::Enum => {
            let value = string_param_value(param, value)?;
            let allowed = param.values.as_deref().unwrap_or(&[]);
            if !allowed.iter().any(|candidate| candidate == &value) {
                bail!(
                    "model run parameter '{}' value '{}' is not in {:?}",
                    param.name,
                    value,
                    allowed
                );
            }
            Ok(ModelRunParamValue::String(value))
        }
        ParamType::Int => {
            let value = integer_param_value(param, value)?;
            ensure_run_param_in_range(param, value as f64)?;
            Ok(ModelRunParamValue::Integer(value))
        }
        ParamType::Float => {
            let value = float_param_value(param, value)?;
            ensure_run_param_in_range(param, value)?;
            Ok(ModelRunParamValue::Float(value))
        }
        ParamType::Bool => Ok(ModelRunParamValue::Boolean(bool_param_value(param, value)?)),
        ParamType::String => Ok(ModelRunParamValue::String(string_param_value(
            param, value,
        )?)),
    }
}

fn string_param_value(param: &Parameter, value: ModelRunParamValue) -> Result<String> {
    match value {
        ModelRunParamValue::String(value) => Ok(value),
        other => {
            bail!(
                "model run parameter '{}' must be a string, got {}",
                param.name,
                value_type(&other)
            )
        }
    }
}

fn integer_param_value(param: &Parameter, value: ModelRunParamValue) -> Result<i64> {
    match value {
        ModelRunParamValue::Integer(value) => Ok(value),
        ModelRunParamValue::String(value) => value.parse::<i64>().with_context(|| {
            format!(
                "model run parameter '{}' must be an integer, got '{}'",
                param.name, value
            )
        }),
        other => {
            bail!(
                "model run parameter '{}' must be an integer, got {}",
                param.name,
                value_type(&other)
            )
        }
    }
}

fn float_param_value(param: &Parameter, value: ModelRunParamValue) -> Result<f64> {
    match value {
        ModelRunParamValue::Float(value) => Ok(value),
        ModelRunParamValue::Integer(value) => Ok(value as f64),
        ModelRunParamValue::String(value) => value.parse::<f64>().with_context(|| {
            format!(
                "model run parameter '{}' must be numeric, got '{}'",
                param.name, value
            )
        }),
        other => {
            bail!(
                "model run parameter '{}' must be numeric, got {}",
                param.name,
                value_type(&other)
            )
        }
    }
}

fn bool_param_value(param: &Parameter, value: ModelRunParamValue) -> Result<bool> {
    match value {
        ModelRunParamValue::Boolean(value) => Ok(value),
        ModelRunParamValue::String(value) => value.parse::<bool>().with_context(|| {
            format!(
                "model run parameter '{}' must be a boolean, got '{}'",
                param.name, value
            )
        }),
        other => {
            bail!(
                "model run parameter '{}' must be a boolean, got {}",
                param.name,
                value_type(&other)
            )
        }
    }
}

fn ensure_run_param_in_range(param: &Parameter, value: f64) -> Result<()> {
    if let Some(min) = param.min {
        if value < min {
            bail!(
                "model run parameter '{}' is below min {}",
                param.name,
                trim_float(min)
            );
        }
    }
    if let Some(max) = param.max {
        if value > max {
            bail!(
                "model run parameter '{}' is above max {}",
                param.name,
                trim_float(max)
            );
        }
    }
    Ok(())
}

fn value_type(value: &ModelRunParamValue) -> &'static str {
    match value {
        ModelRunParamValue::String(_) => "string",
        ModelRunParamValue::Integer(_) => "integer",
        ModelRunParamValue::Float(_) => "float",
        ModelRunParamValue::Boolean(_) => "boolean",
    }
}

fn toml_value_type(value: &toml::Value) -> &'static str {
    match value {
        toml::Value::String(_) => "string",
        toml::Value::Integer(_) => "integer",
        toml::Value::Float(_) => "float",
        toml::Value::Boolean(_) => "boolean",
        toml::Value::Datetime(_) => "datetime",
        toml::Value::Array(_) => "array",
        toml::Value::Table(_) => "table",
    }
}

fn trim_float(value: f64) -> String {
    if value.fract() == 0.0 {
        format!("{}", value as i64)
    } else {
        value.to_string()
    }
}

pub(crate) fn adapter_id_for_runtime_mode(runtime_mode: RuntimeMode) -> &'static str {
    match runtime_mode {
        RuntimeMode::NativeMlx => "native-mlx",
        RuntimeMode::Coreml => "coreml",
        RuntimeMode::PythonEnv => "python-env",
    }
}

struct NativeMlxAdapter;
struct CoreMlAdapter;
struct PythonEnvAdapter;

impl RuntimeAdapter for NativeMlxAdapter {
    fn adapter_id(&self) -> &'static str {
        adapter_id_for_runtime_mode(RuntimeMode::NativeMlx)
    }

    fn provision(
        &self,
        store: &ModelStore,
        manifest: &ModelManifest,
        weights: &ModelWeightPullResult,
    ) -> Result<ModelRuntimeProvisioning> {
        provision_adapter_metadata(self.adapter_id(), store, manifest, weights, None)
    }
}

impl RuntimeAdapter for CoreMlAdapter {
    fn adapter_id(&self) -> &'static str {
        adapter_id_for_runtime_mode(RuntimeMode::Coreml)
    }

    fn provision(
        &self,
        store: &ModelStore,
        manifest: &ModelManifest,
        weights: &ModelWeightPullResult,
    ) -> Result<ModelRuntimeProvisioning> {
        provision_adapter_metadata(self.adapter_id(), store, manifest, weights, None)
    }
}

impl RuntimeAdapter for PythonEnvAdapter {
    fn adapter_id(&self) -> &'static str {
        adapter_id_for_runtime_mode(RuntimeMode::PythonEnv)
    }

    fn provision(
        &self,
        store: &ModelStore,
        manifest: &ModelManifest,
        weights: &ModelWeightPullResult,
    ) -> Result<ModelRuntimeProvisioning> {
        provision_adapter_metadata(
            self.adapter_id(),
            store,
            manifest,
            weights,
            manifest.runtime.requirements.as_deref(),
        )
    }
}

fn provision_adapter_metadata(
    adapter: &str,
    store: &ModelStore,
    manifest: &ModelManifest,
    weights: &ModelWeightPullResult,
    requirements: Option<&str>,
) -> Result<ModelRuntimeProvisioning> {
    let runtime_dir = store.runtime_package_dir(
        &manifest.runtime.mode.to_string(),
        &manifest.package.name,
        &manifest.package.version,
    )?;
    ensure_dir(&runtime_dir)?;

    let adapter_path = runtime_dir.join("adapter.toml");
    let record = RuntimeAdapterRecord {
        package_id: manifest.package_id(),
        adapter,
        runtime_mode: manifest.runtime.mode,
        runtime_entry: &manifest.runtime.entry,
        weights_sha256: &weights.sha256,
        weights_path: &weights.path,
        repo: manifest.runtime.repo.as_deref(),
        python_version: manifest.runtime.python_version.as_deref(),
    };
    let encoded = toml::to_string_pretty(&record)?;
    std::fs::write(&adapter_path, encoded)?;

    let mut files = vec![path_string(&adapter_path)];
    if let Some(requirements) = requirements {
        let requirements_path = runtime_dir.join("requirements.txt");
        std::fs::write(&requirements_path, requirements)?;
        files.push(path_string(&requirements_path));
    }

    Ok(ModelRuntimeProvisioning {
        adapter: adapter.to_string(),
        status: RuntimeProvisioningStatus::Prepared,
        runtime_mode: manifest.runtime.mode,
        runtime_entry: manifest.runtime.entry.clone(),
        runtime_dir: path_string(&runtime_dir),
        files,
        message: format!("{adapter} runtime metadata prepared."),
    })
}

#[derive(Serialize)]
struct RuntimeAdapterRecord<'a> {
    package_id: String,
    adapter: &'a str,
    runtime_mode: RuntimeMode,
    runtime_entry: &'a str,
    weights_sha256: &'a str,
    weights_path: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    repo: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    python_version: Option<&'a str>,
}

#[derive(Deserialize)]
struct OwnedRuntimeAdapterRecord {
    package_id: String,
    adapter: String,
    runtime_mode: RuntimeMode,
    runtime_entry: String,
    weights_sha256: String,
    weights_path: String,
}

fn read_runtime_adapter_record(path: &Path) -> Result<OwnedRuntimeAdapterRecord> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("Cannot read runtime adapter metadata: {}", path.display()))?;
    toml::from_str(&raw)
        .with_context(|| format!("Cannot parse runtime adapter metadata: {}", path.display()))
}

fn validate_runtime_adapter_record(
    record: &OwnedRuntimeAdapterRecord,
    manifest: &ModelManifest,
    weights_path: &Path,
) -> Result<()> {
    let package_id = manifest.package_id();
    ensure_runtime_adapter_field(&package_id, "package_id", &record.package_id, &package_id)?;
    ensure_runtime_adapter_field(
        &package_id,
        "adapter",
        &record.adapter,
        adapter_id_for_runtime_mode(manifest.runtime.mode),
    )?;
    if record.runtime_mode != manifest.runtime.mode {
        bail!(
            "runtime adapter metadata for {package_id} has runtime_mode '{}', expected '{}'",
            record.runtime_mode,
            manifest.runtime.mode
        );
    }
    ensure_runtime_adapter_field(
        &package_id,
        "runtime_entry",
        &record.runtime_entry,
        &manifest.runtime.entry,
    )?;
    ensure_runtime_adapter_field(
        &package_id,
        "weights_sha256",
        &record.weights_sha256,
        &manifest.weights.sha256,
    )?;
    ensure_runtime_adapter_field(
        &package_id,
        "weights_path",
        &record.weights_path,
        &path_string(weights_path),
    )?;
    Ok(())
}

fn ensure_runtime_adapter_field(
    package_id: &str,
    field: &str,
    actual: &str,
    expected: &str,
) -> Result<()> {
    if actual != expected {
        bail!(
            "runtime adapter metadata for {package_id} has {field} '{actual}', expected '{expected}'"
        );
    }
    Ok(())
}

fn path_string(path: &Path) -> String {
    path.display().to_string()
}

#[cfg(test)]
mod tests;
