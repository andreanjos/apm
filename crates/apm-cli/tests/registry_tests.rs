// Integration tests for registry TOML loading and search logic.
// These tests operate directly on the TOML files in tests/fixtures/plugins/
// using the toml + serde crates, and replicate the registry search logic
// so behaviour can be verified without importing the binary crate.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

// ── Registry types (mirrors src/registry/types.rs) ────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum PluginFormat {
    Au,
    Vst3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum InstallType {
    Dmg,
    Pkg,
    Zip,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FormatSource {
    url: String,
    sha256: String,
    install_type: InstallType,
    bundle_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PluginDefinition {
    slug: String,
    name: String,
    vendor: String,
    version: String,
    description: String,
    category: String,
    subcategory: Option<String>,
    license: String,
    #[serde(default)]
    tags: Vec<String>,
    formats: HashMap<PluginFormat, FormatSource>,
    homepage: Option<String>,
}

// ── Registry (mirrors src/registry/mod.rs) ────────────────────────────────────

struct Registry {
    plugins: HashMap<String, PluginDefinition>,
}

impl Registry {
    fn new() -> Self {
        Self { plugins: HashMap::new() }
    }

    fn load_from_cache(cache_dir: &Path) -> anyhow::Result<Self> {
        let plugins_dir = cache_dir.join("plugins");
        let mut registry = Self::new();

        if !plugins_dir.exists() {
            return Ok(registry);
        }

        for entry in std::fs::read_dir(&plugins_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("toml") {
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
        self.plugins.values().find(|p| p.slug.to_lowercase() == lower)
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

// ── Registry loading ──────────────────────────────────────────────────────────

#[test]
fn test_load_registry_from_fixture_directory() {
    let registry = Registry::load_from_cache(&fixtures_dir())
        .expect("should load registry from fixtures");
    assert_eq!(registry.len(), 3, "expected 3 fixture plugins");
}

#[test]
fn test_registry_not_empty_after_load() {
    let registry = Registry::load_from_cache(&fixtures_dir())
        .expect("should load registry");
    assert!(!registry.is_empty());
}

#[test]
fn test_load_single_plugin_toml() {
    let registry = Registry::load_from_cache(&fixtures_dir())
        .expect("should load registry");

    let plugin = registry.find("test-reverb").expect("test-reverb should exist");
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
    let registry = Registry::load_from_cache(&fixtures_dir())
        .expect("should load registry");

    let plugin = registry.find("test-synth").expect("test-synth should exist");
    assert_eq!(plugin.formats.len(), 2, "test-synth should have VST3 and AU formats");
}

#[test]
fn test_load_compressor_plugin() {
    let registry = Registry::load_from_cache(&fixtures_dir())
        .expect("should load registry");

    let plugin = registry.find("test-compressor").expect("test-compressor should exist");
    assert_eq!(plugin.vendor, "Dynamics Corp");
    assert_eq!(plugin.category, "effects");
}

#[test]
fn test_registry_find_nonexistent_returns_none() {
    let registry = Registry::load_from_cache(&fixtures_dir())
        .expect("should load registry");
    assert!(registry.find("does-not-exist").is_none());
}

#[test]
fn test_registry_find_case_insensitive() {
    let registry = Registry::load_from_cache(&fixtures_dir())
        .expect("should load registry");
    let plugin = registry.find("TEST-REVERB");
    assert!(plugin.is_some(), "case-insensitive find should work for TEST-REVERB");
}

#[test]
fn test_load_empty_plugins_directory_returns_empty_registry() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    // Create an empty plugins/ sub-directory (what load_from_cache expects).
    std::fs::create_dir(tmp.path().join("plugins")).unwrap();

    let registry = Registry::load_from_cache(tmp.path())
        .expect("should succeed even with empty dir");
    assert!(registry.is_empty());
    assert_eq!(registry.len(), 0);
}

#[test]
fn test_load_nonexistent_directory_returns_empty_registry() {
    let path = PathBuf::from("/tmp/apm-test-nonexistent-registry-dir-xyz");
    let registry = Registry::load_from_cache(&path)
        .expect("should succeed with nonexistent dir");
    assert!(registry.is_empty());
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
    assert!(!results.is_empty(), "should find results for vendor 'Dynamics Corp'");
    assert!(results.iter().any(|p| p.slug == "test-compressor"));
}

#[test]
fn test_search_by_tag() {
    let registry = Registry::load_from_cache(&fixtures_dir()).unwrap();
    let results = search(&registry, "synthesizer", None, None);
    assert!(!results.is_empty(), "should find results for tag 'synthesizer'");
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
    assert!(results.is_empty(), "should find no results for gibberish query");
}

#[test]
fn test_search_case_insensitive() {
    let registry = Registry::load_from_cache(&fixtures_dir()).unwrap();
    let results_lower = search(&registry, "reverb", None, None);
    let results_upper = search(&registry, "REVERB", None, None);
    let results_mixed = search(&registry, "ReVerb", None, None);
    assert_eq!(results_lower.len(), results_upper.len(), "search should be case-insensitive");
    assert_eq!(results_lower.len(), results_mixed.len());
}

#[test]
fn test_search_empty_query_returns_all() {
    let registry = Registry::load_from_cache(&fixtures_dir()).unwrap();
    let results = search(&registry, "", None, None);
    assert_eq!(results.len(), registry.len(), "empty query should return all plugins");
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
    let vst3 = plugin.formats.get(&PluginFormat::Vst3).expect("should have VST3 format");
    assert_eq!(vst3.url, "https://example.com/test-reverb.zip");
    assert_eq!(vst3.sha256, "abc123");
    assert_eq!(vst3.install_type, InstallType::Zip);
    assert_eq!(vst3.bundle_path.as_deref(), Some("TestReverb.vst3"));
}
