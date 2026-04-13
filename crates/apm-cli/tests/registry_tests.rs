// Integration tests for registry TOML loading and search logic.
// These tests operate directly on the TOML files in tests/fixtures/plugins/
// using the toml + serde crates, and replicate the registry search logic
// so behaviour can be verified without importing the binary crate.

use std::collections::HashMap;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

// ── Registry types (mirrors src/registry/types.rs) ────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum PluginFormat {
    Au,
    Vst3,
    App,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum InstallType {
    Dmg,
    Pkg,
    Zip,
    Mas,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum DownloadType {
    Direct,
    Manual,
    Managed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ProductType {
    Plugin,
    Bundle,
    Expansion,
    PresetPack,
    SampleLibrary,
    Daw,
    Utility,
    Upgrade,
    Subscription,
    Template,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FormatSource {
    url: String,
    sha256: String,
    install_type: InstallType,
    bundle_path: Option<String>,
    #[serde(default)]
    download_type: Option<DownloadType>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PluginRelease {
    version: String,
    formats: HashMap<PluginFormat, FormatSource>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PluginDefinition {
    slug: String,
    name: String,
    vendor: String,
    version: String,
    description: String,
    category: String,
    #[serde(default)]
    product_type: Option<ProductType>,
    subcategory: Option<String>,
    license: String,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    aliases: Vec<String>,
    #[serde(default)]
    installer: Option<String>,
    formats: HashMap<PluginFormat, FormatSource>,
    #[serde(default)]
    releases: Vec<PluginRelease>,
    homepage: Option<String>,
    #[serde(default)]
    is_paid: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct InstallerDefinition {
    name: String,
    vendor: String,
    app_paths: Vec<String>,
    download_url: String,
    homepage: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BundleDefinition {
    slug: String,
    name: String,
    description: String,
    plugins: Vec<String>,
}

// ── Registry (mirrors src/registry/mod.rs) ────────────────────────────────────

struct Registry {
    plugins: HashMap<String, PluginDefinition>,
}

impl Registry {
    fn new() -> Self {
        Self {
            plugins: HashMap::new(),
        }
    }

    fn load_from_cache(cache_dir: &Path) -> anyhow::Result<Self> {
        let plugins_dir = cache_dir.join("plugins");
        let mut registry = Self::new();

        if !plugins_dir.exists() {
            return Ok(registry);
        }

        for entry in WalkDir::new(&plugins_dir) {
            let entry = entry?;
            let path = entry.path();
            if !entry.file_type().is_file()
                || path.extension().and_then(|e| e.to_str()) != Some("toml")
            {
                continue;
            }
            let raw = std::fs::read_to_string(&path)?;
            if let Ok(plugin) = toml::from_str::<PluginDefinition>(&raw) {
                registry.plugins.insert(plugin.slug.clone(), plugin);
            }
        }
        Ok(registry)
    }

    fn find(&self, slug: &str) -> Option<&PluginDefinition> {
        if let Some(p) = self.plugins.get(slug) {
            return Some(p);
        }
        let lower = slug.to_lowercase();
        self.plugins.values().find(|p| {
            p.slug.to_lowercase() == lower
                || p.aliases.iter().any(|alias| alias.to_lowercase() == lower)
        })
    }

    fn len(&self) -> usize {
        self.plugins.len()
    }

    fn is_empty(&self) -> bool {
        self.plugins.is_empty()
    }
}

// ── Search (mirrors src/registry/search.rs) ───────────────────────────────────

fn text_matches(p: &PluginDefinition, query: &str) -> bool {
    p.slug.to_lowercase().contains(query)
        || p.name.to_lowercase().contains(query)
        || p.vendor.to_lowercase().contains(query)
        || p.description.to_lowercase().contains(query)
        || p.category.to_lowercase().contains(query)
        || p.subcategory
            .as_deref()
            .map(|s| s.to_lowercase().contains(query))
            .unwrap_or(false)
        || p.tags.iter().any(|t| t.to_lowercase().contains(query))
        || p.aliases
            .iter()
            .any(|alias| alias.to_lowercase().contains(query))
}

fn search<'r>(
    registry: &'r Registry,
    query: &str,
    category: Option<&str>,
    vendor: Option<&str>,
) -> Vec<&'r PluginDefinition> {
    let query_lower = query.to_lowercase();
    let category_lower = category.map(|c| c.to_lowercase());
    let vendor_lower = vendor.map(|v| v.to_lowercase());

    let mut results: Vec<&PluginDefinition> = registry
        .plugins
        .values()
        .filter(|p| {
            if let Some(ref cat) = category_lower {
                let cat_match = p.category.to_lowercase().contains(cat.as_str())
                    || p.subcategory
                        .as_deref()
                        .map(|s| s.to_lowercase().contains(cat.as_str()))
                        .unwrap_or(false);
                if !cat_match {
                    return false;
                }
            }
            if let Some(ref ven) = vendor_lower {
                if !p.vendor.to_lowercase().contains(ven.as_str()) {
                    return false;
                }
            }
            if query_lower.is_empty() {
                return true;
            }
            text_matches(p, &query_lower)
        })
        .collect();

    results.sort_by_key(|p| p.name.to_lowercase());
    results
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn fixtures_dir() -> PathBuf {
    let mut d = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    d.push("tests/fixtures");
    d
}

fn published_registry_dir() -> PathBuf {
    let mut d = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    d.pop();
    d.pop();
    d.push("registry");
    d
}

fn published_installers_path() -> PathBuf {
    published_registry_dir().join("installers.toml")
}

fn published_bundles_dir() -> PathBuf {
    published_registry_dir().join("bundles")
}

fn is_placeholder_sha256(sha256: &str) -> bool {
    let value = sha256.trim();
    value.is_empty() || value == "manual" || value.chars().all(|c| c == '0')
}

// ── Registry loading ──────────────────────────────────────────────────────────

#[test]
fn test_load_registry_from_fixture_directory() {
    let registry =
        Registry::load_from_cache(&fixtures_dir()).expect("should load registry from fixtures");
    assert_eq!(registry.len(), 3, "expected 3 fixture plugins");
}

#[test]
fn test_registry_not_empty_after_load() {
    let registry = Registry::load_from_cache(&fixtures_dir()).expect("should load registry");
    assert!(!registry.is_empty());
}

#[test]
fn test_load_single_plugin_toml() {
    let registry = Registry::load_from_cache(&fixtures_dir()).expect("should load registry");

    let plugin = registry
        .find("test-reverb")
        .expect("test-reverb should exist");
    assert_eq!(plugin.slug, "test-reverb");
    assert_eq!(plugin.name, "Test Reverb");
    assert_eq!(plugin.vendor, "Test Vendor");
    assert_eq!(plugin.version, "1.0.0");
    assert_eq!(plugin.category, "effects");
    assert_eq!(plugin.subcategory.as_deref(), Some("reverb"));
    assert_eq!(plugin.license, "freeware");
    assert_eq!(plugin.homepage.as_deref(), Some("https://example.com"));
}

#[test]
fn test_load_synth_plugin_has_two_formats() {
    let registry = Registry::load_from_cache(&fixtures_dir()).expect("should load registry");

    let plugin = registry
        .find("test-synth")
        .expect("test-synth should exist");
    assert_eq!(
        plugin.formats.len(),
        2,
        "test-synth should have VST3 and AU formats"
    );
}

#[test]
fn test_load_synth_plugin_release_history() {
    let registry = Registry::load_from_cache(&fixtures_dir()).expect("should load registry");

    let plugin = registry
        .find("test-synth")
        .expect("test-synth should exist");

    assert_eq!(plugin.releases.len(), 2, "expected two historical releases");
    assert_eq!(plugin.releases[0].version, "2.0.0");
    assert_eq!(plugin.releases[1].version, "1.5.0");
}

#[test]
fn test_load_compressor_plugin() {
    let registry = Registry::load_from_cache(&fixtures_dir()).expect("should load registry");

    let plugin = registry
        .find("test-compressor")
        .expect("test-compressor should exist");
    assert_eq!(plugin.vendor, "Dynamics Corp");
    assert_eq!(plugin.category, "effects");
}

#[test]
fn test_registry_find_nonexistent_returns_none() {
    let registry = Registry::load_from_cache(&fixtures_dir()).expect("should load registry");
    assert!(registry.find("does-not-exist").is_none());
}

#[test]
fn test_registry_find_case_insensitive() {
    let registry = Registry::load_from_cache(&fixtures_dir()).expect("should load registry");
    let plugin = registry.find("TEST-REVERB");
    assert!(
        plugin.is_some(),
        "case-insensitive find should work for TEST-REVERB"
    );
}

#[test]
fn test_load_empty_plugins_directory_returns_empty_registry() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    // Create an empty plugins/ sub-directory (what load_from_cache expects).
    std::fs::create_dir(tmp.path().join("plugins")).unwrap();

    let registry =
        Registry::load_from_cache(tmp.path()).expect("should succeed even with empty dir");
    assert!(registry.is_empty());
    assert_eq!(registry.len(), 0);
}

#[test]
fn test_load_nonexistent_directory_returns_empty_registry() {
    let path = PathBuf::from("/tmp/apm-test-nonexistent-registry-dir-xyz");
    let registry = Registry::load_from_cache(&path).expect("should succeed with nonexistent dir");
    assert!(registry.is_empty());
}

#[test]
fn test_loads_nested_vendor_plugin_directory() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let nested = tmp.path().join("plugins/test-vendor");
    std::fs::create_dir_all(&nested).unwrap();
    std::fs::write(
        nested.join("test-nested.toml"),
        r#"
slug = "test-nested"
name = "Test Nested"
vendor = "Test Vendor"
version = "1.0.0"
description = "Nested fixture"
category = "effects"
license = "freeware"

[formats.au]
url = "https://example.com/test-nested.zip"
sha256 = "abc"
install_type = "zip"
"#,
    )
    .unwrap();

    let registry = Registry::load_from_cache(tmp.path()).expect("nested directory should load");
    assert!(registry.find("test-nested").is_some());
}

// ── Tags and fields ───────────────────────────────────────────────────────────

#[test]
fn test_reverb_plugin_has_expected_tags() {
    let registry = Registry::load_from_cache(&fixtures_dir()).unwrap();
    let plugin = registry.find("test-reverb").unwrap();
    assert!(plugin.tags.contains(&"reverb".to_string()));
    assert!(plugin.tags.contains(&"test".to_string()));
}

#[test]
fn test_synth_plugin_tags_include_synthesizer() {
    let registry = Registry::load_from_cache(&fixtures_dir()).unwrap();
    let plugin = registry.find("test-synth").unwrap();
    assert!(plugin.tags.contains(&"synthesizer".to_string()));
}

// ── Search ────────────────────────────────────────────────────────────────────

#[test]
fn test_search_by_name() {
    let registry = Registry::load_from_cache(&fixtures_dir()).unwrap();
    let results = search(&registry, "reverb", None, None);
    assert!(!results.is_empty(), "should find results for 'reverb'");
    assert!(
        results.iter().any(|p| p.slug == "test-reverb"),
        "test-reverb should appear in results"
    );
}

#[test]
fn test_search_by_vendor() {
    let registry = Registry::load_from_cache(&fixtures_dir()).unwrap();
    let results = search(&registry, "Dynamics Corp", None, None);
    assert!(
        !results.is_empty(),
        "should find results for vendor 'Dynamics Corp'"
    );
    assert!(results.iter().any(|p| p.slug == "test-compressor"));
}

#[test]
fn test_search_by_tag() {
    let registry = Registry::load_from_cache(&fixtures_dir()).unwrap();
    let results = search(&registry, "synthesizer", None, None);
    assert!(
        !results.is_empty(),
        "should find results for tag 'synthesizer'"
    );
    assert!(results.iter().any(|p| p.slug == "test-synth"));
}

#[test]
fn test_search_with_category_filter_effects() {
    let registry = Registry::load_from_cache(&fixtures_dir()).unwrap();
    let results = search(&registry, "", Some("effects"), None);
    assert_eq!(results.len(), 2, "should find exactly 2 effects plugins");
    assert!(results.iter().all(|p| p.category == "effects"));
}

#[test]
fn test_search_with_category_filter_instruments() {
    let registry = Registry::load_from_cache(&fixtures_dir()).unwrap();
    let results = search(&registry, "", Some("instruments"), None);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].slug, "test-synth");
}

#[test]
fn test_search_no_results() {
    let registry = Registry::load_from_cache(&fixtures_dir()).unwrap();
    let results = search(&registry, "zyxwvutsrqponmlkjihgfedcba", None, None);
    assert!(
        results.is_empty(),
        "should find no results for gibberish query"
    );
}

#[test]
fn test_search_case_insensitive() {
    let registry = Registry::load_from_cache(&fixtures_dir()).unwrap();
    let results_lower = search(&registry, "reverb", None, None);
    let results_upper = search(&registry, "REVERB", None, None);
    let results_mixed = search(&registry, "ReVerb", None, None);
    assert_eq!(
        results_lower.len(),
        results_upper.len(),
        "search should be case-insensitive"
    );
    assert_eq!(results_lower.len(), results_mixed.len());
}

#[test]
fn test_search_empty_query_returns_all() {
    let registry = Registry::load_from_cache(&fixtures_dir()).unwrap();
    let results = search(&registry, "", None, None);
    assert_eq!(
        results.len(),
        registry.len(),
        "empty query should return all plugins"
    );
}

#[test]
fn test_search_with_vendor_filter() {
    let registry = Registry::load_from_cache(&fixtures_dir()).unwrap();
    let results = search(&registry, "", None, Some("Synth Vendor"));
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].slug, "test-synth");
}

#[test]
fn test_search_vendor_filter_no_match() {
    let registry = Registry::load_from_cache(&fixtures_dir()).unwrap();
    let results = search(&registry, "", None, Some("Nonexistent Vendor XYZ"));
    assert!(results.is_empty());
}

#[test]
fn test_search_by_subcategory() {
    let registry = Registry::load_from_cache(&fixtures_dir()).unwrap();
    let results = search(&registry, "dynamics", None, None);
    assert!(!results.is_empty());
    assert!(results.iter().any(|p| p.slug == "test-compressor"));
}

#[test]
fn test_reverb_plugin_vst3_format_has_correct_url() {
    let registry = Registry::load_from_cache(&fixtures_dir()).unwrap();
    let plugin = registry.find("test-reverb").unwrap();
    let vst3 = plugin
        .formats
        .get(&PluginFormat::Vst3)
        .expect("should have VST3 format");
    assert_eq!(vst3.url, "https://example.com/test-reverb.zip");
    assert_eq!(vst3.sha256, "abc123");
    assert_eq!(vst3.install_type, InstallType::Zip);
    assert_eq!(vst3.bundle_path.as_deref(), Some("TestReverb.vst3"));
}

#[test]
fn test_published_registry_has_no_unverified_direct_downloads() {
    let registry = Registry::load_from_cache(&published_registry_dir())
        .expect("published registry should load");
    assert!(
        registry.len() > 500,
        "published registry should include the full plugin set"
    );

    let mut offenders = Vec::new();
    for plugin in registry.plugins.values() {
        for (format, source) in &plugin.formats {
            let download_type = source.download_type.clone().unwrap_or(DownloadType::Direct);
            if download_type == DownloadType::Direct && is_placeholder_sha256(&source.sha256) {
                offenders.push(format!("{}:{format:?}", plugin.slug));
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "direct downloads must have real SHA256 checksums; offenders: {}",
        offenders.join(", ")
    );
}

#[test]
fn test_published_registry_declares_download_type_for_all_formats() {
    let registry = Registry::load_from_cache(&published_registry_dir())
        .expect("published registry should load");

    let mut missing = Vec::new();
    for plugin in registry.plugins.values() {
        for format in plugin.formats.keys() {
            if plugin.formats[format].download_type.is_none() {
                missing.push(format!("{}:{format:?}", plugin.slug));
            }
        }
        for release in &plugin.releases {
            for (format, source) in &release.formats {
                if source.download_type.is_none() {
                    missing.push(format!("{}@{}:{format:?}", plugin.slug, release.version));
                }
            }
        }
    }

    assert!(
        missing.is_empty(),
        "published format records must declare download_type explicitly; missing: {}",
        missing.join(", ")
    );
}

#[test]
fn test_published_registry_has_no_direct_downloads_for_catalog_only_records() {
    let registry = Registry::load_from_cache(&published_registry_dir())
        .expect("published registry should load");

    let mut offenders = Vec::new();
    for plugin in registry.plugins.values() {
        let product_type = plugin.product_type.as_ref().unwrap_or(&ProductType::Plugin);
        let installable_product = matches!(
            product_type,
            ProductType::Plugin | ProductType::Bundle | ProductType::Daw | ProductType::Utility
        );
        if installable_product {
            continue;
        }

        for (format, source) in &plugin.formats {
            if source.download_type == Some(DownloadType::Direct) {
                offenders.push(format!("{} ({product_type:?}):{format:?}", plugin.slug));
            }
        }
        for release in &plugin.releases {
            for (format, source) in &release.formats {
                if source.download_type == Some(DownloadType::Direct) {
                    offenders.push(format!(
                        "{}@{} ({product_type:?}):{format:?}",
                        plugin.slug, release.version
                    ));
                }
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "catalog-only records must not expose direct install sources; offenders: {}",
        offenders.join(", ")
    );
}

#[test]
fn test_published_registry_installer_references_are_valid() {
    let registry = Registry::load_from_cache(&published_registry_dir())
        .expect("published registry should load");
    let raw = std::fs::read_to_string(published_installers_path())
        .expect("published installers.toml should exist");
    let installers: HashMap<String, InstallerDefinition> =
        toml::from_str(&raw).expect("published installers.toml should parse");

    let mut issues = Vec::new();
    for (key, installer) in &installers {
        if installer.name.trim().is_empty() {
            issues.push(format!("{key}: missing name"));
        }
        if installer.vendor.trim().is_empty() {
            issues.push(format!("{key}: missing vendor"));
        }
        if installer.app_paths.is_empty() {
            issues.push(format!("{key}: missing app_paths"));
        }
        for path in &installer.app_paths {
            if !path.starts_with("/Applications/") || !path.ends_with(".app") {
                issues.push(format!("{key}: suspicious app path {path}"));
            }
        }
        if !installer.download_url.starts_with("https://") {
            issues.push(format!("{key}: download_url must be https"));
        }
        if !installer.homepage.starts_with("https://") {
            issues.push(format!("{key}: homepage must be https"));
        }
    }

    for plugin in registry.plugins.values() {
        if let Some(installer) = &plugin.installer {
            if !installers.contains_key(installer) {
                issues.push(format!("{}: unknown installer {installer}", plugin.slug));
            }
        }

        let has_managed = plugin
            .formats
            .values()
            .any(|source| source.download_type == Some(DownloadType::Managed))
            || plugin.releases.iter().any(|release| {
                release
                    .formats
                    .values()
                    .any(|source| source.download_type == Some(DownloadType::Managed))
            });
        if has_managed && plugin.installer.is_none() {
            issues.push(format!("{}: managed source missing installer", plugin.slug));
        }
    }

    assert!(
        issues.is_empty(),
        "published installer metadata must be complete and referenced keys must exist: {}",
        issues.join(", ")
    );
}

#[test]
fn test_published_registry_bundles_reference_installable_products() {
    let registry = Registry::load_from_cache(&published_registry_dir())
        .expect("published registry should load");
    let mut issues = Vec::new();
    let mut bundle_count = 0;

    for entry in std::fs::read_dir(published_bundles_dir()).expect("published bundles dir") {
        let path = entry.expect("bundle dir entry").path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("toml") {
            continue;
        }
        bundle_count += 1;
        let raw = std::fs::read_to_string(&path).expect("bundle file should be readable");
        let bundle: BundleDefinition = toml::from_str(&raw)
            .unwrap_or_else(|err| panic!("bundle file should parse: {}: {err}", path.display()));

        if bundle.slug.trim().is_empty()
            || bundle.name.trim().is_empty()
            || bundle.description.trim().is_empty()
        {
            issues.push(format!("{}: missing required bundle text", path.display()));
        }

        let mut seen = HashSet::new();
        for slug in &bundle.plugins {
            if !seen.insert(slug) {
                issues.push(format!("{}: duplicate member {slug}", bundle.slug));
            }
            let Some(plugin) = registry.find(slug) else {
                issues.push(format!("{}: missing member {slug}", bundle.slug));
                continue;
            };
            let product_type = plugin.product_type.as_ref().unwrap_or(&ProductType::Plugin);
            let installable_product = matches!(
                product_type,
                ProductType::Plugin | ProductType::Bundle | ProductType::Daw | ProductType::Utility
            );
            if !installable_product {
                issues.push(format!(
                    "{}: member {slug} is catalog-only ({product_type:?})",
                    bundle.slug
                ));
            }
        }
    }

    assert!(
        bundle_count > 0,
        "published registry should include bundles"
    );
    assert!(
        issues.is_empty(),
        "published bundles must reference existing installable products: {}",
        issues.join(", ")
    );
}

fn normalize_catalog_key(value: &str) -> String {
    value
        .to_lowercase()
        .replace('&', " and ")
        .replace("8211", " ")
        .replace("038", " ")
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[test]
fn test_published_registry_has_product_types_and_canonical_slugs() {
    let registry = Registry::load_from_cache(&published_registry_dir())
        .expect("published registry should load");
    assert!(
        registry.len() > 8_000,
        "published registry should include the promoted canonical catalog"
    );

    let live_slugs: HashSet<&str> = registry.plugins.keys().map(String::as_str).collect();
    let mut alias_slugs = HashSet::new();
    let mut catalog_keys = HashSet::new();
    let mut missing_product_type = Vec::new();
    let mut alias_collisions = Vec::new();
    let mut duplicate_catalog_keys = Vec::new();
    let mut unpaid_commercial = Vec::new();

    for plugin in registry.plugins.values() {
        if plugin.product_type.is_none() {
            missing_product_type.push(plugin.slug.clone());
        }
        if plugin.license.eq_ignore_ascii_case("commercial") && !plugin.is_paid {
            unpaid_commercial.push(plugin.slug.clone());
        }

        for alias in &plugin.aliases {
            if live_slugs.contains(alias.as_str()) {
                alias_collisions.push(format!("{} -> {alias}", plugin.slug));
            }
            alias_slugs.insert(alias.as_str());
        }

        let key = format!(
            "{}:{}",
            normalize_catalog_key(&plugin.vendor),
            normalize_catalog_key(&plugin.name)
        );
        if !catalog_keys.insert(key) {
            duplicate_catalog_keys.push(plugin.slug.clone());
        }
    }

    assert!(
        missing_product_type.is_empty(),
        "all published records must declare product_type; missing: {}",
        missing_product_type.join(", ")
    );
    assert!(
        alias_collisions.is_empty(),
        "aliases must not collide with live slugs; collisions: {}",
        alias_collisions.join(", ")
    );
    assert!(
        duplicate_catalog_keys.is_empty(),
        "same-vendor/same-name duplicate records must be canonicalized; duplicates: {}",
        duplicate_catalog_keys.join(", ")
    );
    assert!(
        unpaid_commercial.is_empty(),
        "commercial registry records must be marked is_paid; offenders: {}",
        unpaid_commercial.join(", ")
    );
    assert!(
        !alias_slugs.is_empty(),
        "canonicalized duplicate slugs should be preserved as aliases"
    );
}
