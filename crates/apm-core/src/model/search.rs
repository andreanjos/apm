use super::{ModelManifest, ParamType};

pub fn model_manifest_matches_query(manifest: &ModelManifest, query: &str) -> bool {
    let terms = query
        .split_whitespace()
        .map(|term| term.to_lowercase())
        .collect::<Vec<_>>();
    if terms.is_empty() {
        return true;
    }

    let haystack = searchable_text(manifest);
    terms.iter().all(|term| haystack.contains(term))
}

fn searchable_text(manifest: &ModelManifest) -> String {
    let mut fields = vec![
        manifest.package_id(),
        manifest.package.name.clone(),
        manifest.package.version.clone(),
        manifest.package.description.clone(),
        manifest.package.publisher.clone(),
        manifest.runtime.mode.to_string(),
        manifest.runtime.entry.clone(),
        manifest.io.input.to_string(),
        manifest.io.output.to_string(),
        manifest.weights.source.clone(),
        manifest.weights.sha256.clone(),
        manifest.weights.format.clone(),
        manifest.license.spdx.clone(),
    ];

    for param in &manifest.params {
        fields.push(param.name.clone());
        fields.push(param_type_label(param.param_type).to_string());
        if let Some(values) = &param.values {
            fields.extend(values.iter().cloned());
        }
    }

    fields.join("\n").to_lowercase()
}

fn param_type_label(param_type: ParamType) -> &'static str {
    match param_type {
        ParamType::Enum => "enum",
        ParamType::Int => "int",
        ParamType::Float => "float",
        ParamType::Bool => "bool",
        ParamType::String => "string",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_cached_model_manifest_fields() {
        let manifest = test_manifest();

        assert!(model_manifest_matches_query(&manifest, "demucs stems"));
        assert!(model_manifest_matches_query(&manifest, "native-mlx"));
        assert!(model_manifest_matches_query(&manifest, "publisher"));
        assert!(model_manifest_matches_query(&manifest, "shifts int"));
        assert!(!model_manifest_matches_query(&manifest, "whisper"));
    }

    #[test]
    fn empty_query_matches_all_models() {
        assert!(model_manifest_matches_query(&test_manifest(), "  "));
    }

    fn test_manifest() -> ModelManifest {
        ModelManifest::from_toml_str(
            r#"
[package]
name = "demucs"
version = "4.0.1"
description = "Music source separation into stems"
publisher = "apm-core publisher"

[runtime]
mode = "native-mlx"
entry = "demucs_mlx.Separator"

[weights]
source = "https://example.test/demucs.safetensors"
sha256 = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
format = "safetensors"

[io]
input = "audio"
output = "stems"

[[params]]
name = "shifts"
type = "int"
min = 1
max = 8
default = 2

[license]
spdx = "MIT"
commercial = true

[hardware]
min_memory_gb = 8
"#,
        )
        .expect("test manifest")
    }
}
