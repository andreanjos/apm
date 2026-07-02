use std::collections::HashSet;
use std::fmt;
use std::path::Path;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelManifest {
    pub package: PackageSection,
    pub runtime: RuntimeSection,
    pub weights: WeightsSection,
    pub io: IoSection,
    #[serde(default)]
    pub params: Vec<Parameter>,
    pub license: LicenseSection,
    pub hardware: HardwareSection,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageSection {
    pub name: String,
    pub version: String,
    pub description: String,
    pub publisher: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RuntimeMode {
    NativeMlx,
    Coreml,
    PythonEnv,
}

impl fmt::Display for RuntimeMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NativeMlx => write!(f, "native-mlx"),
            Self::Coreml => write!(f, "coreml"),
            Self::PythonEnv => write!(f, "python-env"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeSection {
    pub mode: RuntimeMode,
    pub entry: String,
    #[serde(default)]
    pub repo: Option<String>,
    #[serde(default)]
    pub python_version: Option<String>,
    #[serde(default)]
    pub requirements: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WeightsSection {
    pub source: String,
    pub sha256: String,
    pub format: String,
    #[serde(default)]
    pub convert: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum IoType {
    Audio,
    Stems,
    Midi,
    Text,
    Embedding,
    Spectrogram,
}

impl fmt::Display for IoType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Audio => write!(f, "audio"),
            Self::Stems => write!(f, "stems"),
            Self::Midi => write!(f, "midi"),
            Self::Text => write!(f, "text"),
            Self::Embedding => write!(f, "embedding"),
            Self::Spectrogram => write!(f, "spectrogram"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IoSection {
    pub input: IoType,
    pub output: IoType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ParamType {
    Enum,
    Int,
    Float,
    Bool,
    String,
}

impl fmt::Display for ParamType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Enum => write!(f, "enum"),
            Self::Int => write!(f, "int"),
            Self::Float => write!(f, "float"),
            Self::Bool => write!(f, "bool"),
            Self::String => write!(f, "string"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Parameter {
    pub name: String,
    #[serde(rename = "type")]
    pub param_type: ParamType,
    #[serde(default)]
    pub values: Option<Vec<String>>,
    #[serde(default)]
    pub min: Option<f64>,
    #[serde(default)]
    pub max: Option<f64>,
    #[serde(default)]
    pub default: Option<toml::Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LicenseSection {
    pub spdx: String,
    pub commercial: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HardwareSection {
    pub min_memory_gb: u16,
    #[serde(default)]
    pub requires: Vec<String>,
}

impl ModelManifest {
    pub fn from_toml_str(input: &str) -> Result<Self> {
        let manifest: Self =
            toml::from_str(input).context("Failed to parse model manifest TOML")?;
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn from_path(path: &Path) -> Result<Self> {
        let raw = std::fs::read_to_string(path)
            .with_context(|| format!("Cannot read model manifest: {}", path.display()))?;
        Self::from_toml_str(&raw)
            .with_context(|| format!("Invalid model manifest at {}", path.display()))
    }

    pub fn package_id(&self) -> String {
        format!("{}@{}", self.package.name, self.package.version)
    }

    pub fn validate(&self) -> Result<()> {
        ensure_non_empty("package.name", &self.package.name)?;
        ensure_non_empty("package.version", &self.package.version)?;
        ensure_package_segment("package.name", &self.package.name)?;
        ensure_package_segment("package.version", &self.package.version)?;
        ensure_non_empty("package.description", &self.package.description)?;
        ensure_non_empty("package.publisher", &self.package.publisher)?;
        ensure_non_empty("runtime.entry", &self.runtime.entry)?;
        ensure_non_empty("weights.source", &self.weights.source)?;
        ensure_non_empty("weights.format", &self.weights.format)?;
        ensure_sha256("weights.sha256", &self.weights.sha256)?;
        ensure_non_empty("license.spdx", &self.license.spdx)?;

        if self.hardware.min_memory_gb == 0 {
            bail!("hardware.min_memory_gb must be greater than 0");
        }
        for req in &self.hardware.requires {
            ensure_non_empty("hardware.requires[]", req)?;
        }

        match self.runtime.mode {
            RuntimeMode::PythonEnv => {
                ensure_option_non_empty("runtime.repo", self.runtime.repo.as_deref())?;
                ensure_option_non_empty(
                    "runtime.python_version",
                    self.runtime.python_version.as_deref(),
                )?;
                ensure_option_non_empty(
                    "runtime.requirements",
                    self.runtime.requirements.as_deref(),
                )?;
            }
            RuntimeMode::NativeMlx | RuntimeMode::Coreml => {}
        }

        let mut seen = HashSet::new();
        for param in &self.params {
            ensure_non_empty("params.name", &param.name)?;
            if !seen.insert(param.name.clone()) {
                bail!("duplicate parameter '{}'", param.name);
            }
            validate_parameter(param)?;
        }

        Ok(())
    }
}

fn validate_parameter(param: &Parameter) -> Result<()> {
    if let (Some(min), Some(max)) = (param.min, param.max) {
        if min > max {
            bail!("parameter '{}' has min greater than max", param.name);
        }
    }

    match param.param_type {
        ParamType::Enum => {
            let values = param
                .values
                .as_ref()
                .filter(|values| !values.is_empty())
                .with_context(|| format!("enum parameter '{}' must declare values", param.name))?;
            for value in values {
                ensure_non_empty("params.values[]", value)?;
            }
            if let Some(default) = &param.default {
                let default = default.as_str().with_context(|| {
                    format!("enum parameter '{}' default must be a string", param.name)
                })?;
                if !values.iter().any(|value| value == default) {
                    bail!(
                        "enum parameter '{}' default '{}' is not in values",
                        param.name,
                        default
                    );
                }
            }
        }
        ParamType::Int => {
            ensure_integer_bound("min", param.name.as_str(), param.min)?;
            ensure_integer_bound("max", param.name.as_str(), param.max)?;
            if let Some(default) = &param.default {
                let default = default.as_integer().with_context(|| {
                    format!("int parameter '{}' default must be an integer", param.name)
                })?;
                ensure_default_in_range(param, default as f64)?;
            }
        }
        ParamType::Float => {
            if let Some(default) = &param.default {
                let default = numeric_default(default).with_context(|| {
                    format!("float parameter '{}' default must be numeric", param.name)
                })?;
                ensure_default_in_range(param, default)?;
            }
        }
        ParamType::Bool => {
            if let Some(default) = &param.default {
                default.as_bool().with_context(|| {
                    format!("bool parameter '{}' default must be a boolean", param.name)
                })?;
            }
        }
        ParamType::String => {
            if let Some(default) = &param.default {
                default.as_str().with_context(|| {
                    format!("string parameter '{}' default must be a string", param.name)
                })?;
            }
        }
    }

    Ok(())
}

fn ensure_integer_bound(bound: &str, name: &str, value: Option<f64>) -> Result<()> {
    if let Some(value) = value {
        if value.fract() != 0.0 {
            bail!("int parameter '{name}' {bound} must be an integer");
        }
    }
    Ok(())
}

fn ensure_default_in_range(param: &Parameter, default: f64) -> Result<()> {
    if let Some(min) = param.min {
        if default < min {
            bail!(
                "parameter '{}' default is below min {}",
                param.name,
                trim_float(min)
            );
        }
    }
    if let Some(max) = param.max {
        if default > max {
            bail!(
                "parameter '{}' default is above max {}",
                param.name,
                trim_float(max)
            );
        }
    }
    Ok(())
}

fn numeric_default(value: &toml::Value) -> Option<f64> {
    value
        .as_float()
        .or_else(|| value.as_integer().map(|value| value as f64))
}

fn trim_float(value: f64) -> String {
    if value.fract() == 0.0 {
        format!("{}", value as i64)
    } else {
        value.to_string()
    }
}

fn ensure_non_empty(field: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        bail!("{field} must not be empty");
    }
    Ok(())
}

fn ensure_package_segment(field: &str, value: &str) -> Result<()> {
    let safe = value
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'+'));
    if safe && value != "." && value != ".." {
        Ok(())
    } else {
        bail!("{field} may only contain ASCII letters, numbers, '.', '-', '_', or '+'");
    }
}

fn ensure_option_non_empty(field: &str, value: Option<&str>) -> Result<()> {
    match value {
        Some(value) => ensure_non_empty(field, value),
        None => bail!("{field} is required"),
    }
}

fn ensure_sha256(field: &str, value: &str) -> Result<()> {
    if value.len() != 64 || !value.chars().all(|ch| ch.is_ascii_hexdigit()) {
        bail!("{field} must be a 64-character SHA256 hex digest");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEMUCS_MANIFEST: &str = r#"
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
sha256 = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
format = "safetensors"

[io]
input = "audio"
output = "stems"

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

[license]
spdx = "MIT"
commercial = true

[hardware]
min_memory_gb = 8
requires = ["apple-silicon"]
"#;

    #[test]
    fn parses_valid_manifest() {
        let manifest =
            ModelManifest::from_toml_str(DEMUCS_MANIFEST).expect("manifest should parse");

        assert_eq!(manifest.package_id(), "demucs@4.0.1");
        assert_eq!(manifest.runtime.mode, RuntimeMode::NativeMlx);
        assert_eq!(manifest.io.input, IoType::Audio);
        assert_eq!(manifest.io.output, IoType::Stems);
        assert_eq!(manifest.params.len(), 2);
    }

    #[test]
    fn rejects_invalid_sha256() {
        let invalid = DEMUCS_MANIFEST.replace(
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
            "not-a-sha",
        );

        let error = ModelManifest::from_toml_str(&invalid).expect_err("invalid digest should fail");
        assert!(format!("{error:#}").contains("weights.sha256"));
    }

    #[test]
    fn rejects_package_path_segments() {
        let invalid = DEMUCS_MANIFEST.replace("name = \"demucs\"", "name = \"../demucs\"");

        let error = ModelManifest::from_toml_str(&invalid).expect_err("unsafe name should fail");

        assert!(format!("{error:#}").contains("package.name"));
    }

    #[test]
    fn python_env_requires_environment_fields() {
        let invalid = DEMUCS_MANIFEST.replace("native-mlx", "python-env");

        let error = ModelManifest::from_toml_str(&invalid)
            .expect_err("python-env without repo should fail");
        assert!(format!("{error:#}").contains("runtime.repo"));
    }

    #[test]
    fn rejects_enum_default_outside_values() {
        let invalid = DEMUCS_MANIFEST.replace("default = \"4\"", "default = \"8\"");

        let error =
            ModelManifest::from_toml_str(&invalid).expect_err("bad enum default should fail");
        assert!(format!("{error:#}").contains("default '8' is not in values"));
    }

    #[test]
    fn rejects_numeric_default_outside_range() {
        let invalid = DEMUCS_MANIFEST.replace("default = 1", "default = 11");

        let error =
            ModelManifest::from_toml_str(&invalid).expect_err("bad int default should fail");
        assert!(format!("{error:#}").contains("default is above max 10"));
    }

    #[test]
    fn rejects_fractional_int_bounds() {
        let invalid = DEMUCS_MANIFEST.replace("min = 1", "min = 1.5");

        let error =
            ModelManifest::from_toml_str(&invalid).expect_err("fractional int min should fail");
        assert!(format!("{error:#}").contains("min must be an integer"));
    }
}
