use std::path::PathBuf;

use super::*;
use crate::registry::{DownloadType, FormatSource, InstallType, PluginDefinition, ProductType};

fn test_config() -> (tempfile::TempDir, Config) {
    let temp = tempfile::tempdir().expect("temp dir");
    let config = Config {
        data_dir: Some(temp.path().join("data")),
        cache_dir: Some(temp.path().join("cache")),
        ..Config::default()
    };
    (temp, config)
}

fn scanned_plugin(
    name: &str,
    vendor: &str,
    format: scanner::PluginFormat,
    path: PathBuf,
) -> scanner::ScannedPlugin {
    scanner::ScannedPlugin {
        name: name.to_string(),
        version: "1.2.3".to_string(),
        vendor: vendor.to_string(),
        bundle_id: "com.example.test-reverb".to_string(),
        format,
        scope: scanner::InstallScope::User,
        path,
    }
}

fn matched_slug(slug: &str) -> Matched {
    Matched {
        slug: Some(slug.to_string()),
        method: Some(ScanMatchMethod::NameVendor),
    }
}

fn registry_with(definition: PluginDefinition) -> Registry {
    let mut registry = Registry::new();
    registry.plugins.insert(definition.slug.clone(), definition);
    registry
}

fn plugin_definition(slug: &str, formats: &[PluginFormat]) -> PluginDefinition {
    PluginDefinition {
        slug: slug.to_string(),
        name: "Test Reverb".to_string(),
        vendor: "Test Vendor".to_string(),
        version: "1.0.0".to_string(),
        description: "Test plugin".to_string(),
        category: "effects".to_string(),
        product_type: ProductType::Plugin,
        subcategory: Some("reverb".to_string()),
        license: "freeware".to_string(),
        tags: vec!["test".to_string()],
        aliases: Vec::new(),
        installer: None,
        formats: formats
            .iter()
            .copied()
            .map(|format| (format, format_source()))
            .collect(),
        releases: Vec::new(),
        homepage: None,
        purchase_url: None,
        bundle_ids: Vec::new(),
        is_paid: false,
        price_cents: None,
        currency: None,
        source_name: Some("official".to_string()),
    }
}

fn format_source() -> FormatSource {
    FormatSource {
        url: "https://example.test/TestReverb.zip".to_string(),
        sha256: "0".repeat(64),
        install_type: InstallType::Zip,
        bundle_path: None,
        download_type: DownloadType::Direct,
    }
}

fn vst3_bundle(temp: &tempfile::TempDir, name: &str) -> PathBuf {
    temp.path().join("Library/Audio/Plug-Ins/VST3").join(name)
}

#[test]
fn adopt_external_matches_records_declared_format() {
    let (temp, config) = test_config();
    let bundle = vst3_bundle(&temp, "TestReverb.vst3");
    let plugin = scanned_plugin(
        "Test Reverb",
        "Test Vendor",
        scanner::PluginFormat::Vst3,
        bundle.clone(),
    );
    let registry = registry_with(plugin_definition("test-reverb", &[PluginFormat::Vst3]));
    let mut state = InstallState::default();

    let adopted = adopt_external_matches(
        &config,
        &[plugin],
        &[matched_slug("test-reverb")],
        Some(&registry),
        &mut state,
    )
    .expect("adopt external match");

    assert_eq!(adopted, 1);
    let installed = state.find("test-reverb").expect("recorded install");
    assert_eq!(installed.origin, InstallOrigin::External);
    assert_eq!(installed.vendor, "Test Vendor");
    assert_eq!(installed.formats.len(), 1);
    assert_eq!(installed.formats[0].format, PluginFormat::Vst3);
    assert_eq!(installed.formats[0].path, bundle);
}

#[test]
fn adopt_external_matches_ignores_undeclared_registry_format() {
    let (temp, config) = test_config();
    let plugin = scanned_plugin(
        "Test Reverb",
        "Test Vendor",
        scanner::PluginFormat::Vst3,
        vst3_bundle(&temp, "TestReverb.vst3"),
    );
    let registry = registry_with(plugin_definition("test-reverb", &[PluginFormat::Au]));
    let mut state = InstallState::default();

    let adopted = adopt_external_matches(
        &config,
        &[plugin],
        &[matched_slug("test-reverb")],
        Some(&registry),
        &mut state,
    )
    .expect("adopt external match");

    assert_eq!(adopted, 0);
    assert!(
        state.plugins.is_empty(),
        "scan should not invent VST3 state for an AU-only registry record"
    );
}
