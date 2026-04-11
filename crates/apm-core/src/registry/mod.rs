pub mod installers;
pub mod matcher;
pub mod search;
pub mod sync;
pub mod types;

use installers::load_installers_toml;
use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result};
use tracing::debug;
use walkdir::WalkDir;

use crate::config::Config;

// Re-export all registry types at the crate-module boundary so that future
// phases can import them as `use apm::registry::PluginDefinition` etc. without
// digging into the internal `types` submodule.
pub use installers::InstallerDefinition;
pub use types::{
    DownloadType, FormatSource, InstallType, PluginBundle, PluginDefinition, PluginFormat,
    PluginRelease, ProductType, Source,
};

// ── Registry ──────────────────────────────────────────────────────────────────

/// An in-memory collection of plugin definitions loaded from one or more
/// registry sources. Keyed by plugin slug.
#[derive(Debug, Default)]
pub struct Registry {
    /// All known plugins, keyed by slug (e.g. `"valhalla-supermassive"`).
    pub plugins: HashMap<String, PluginDefinition>,

    /// Source-specific plugin views, keyed by source name then slug.
    pub plugins_by_source: HashMap<String, HashMap<String, PluginDefinition>>,

    /// All known bundles (meta-packages), keyed by bundle slug.
    pub bundles: HashMap<String, PluginBundle>,

    /// All known vendor installer definitions, keyed by installer slug.
    pub installers: HashMap<String, InstallerDefinition>,
}

impl Registry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            plugins: HashMap::new(),
            plugins_by_source: HashMap::new(),
            bundles: HashMap::new(),
            installers: HashMap::new(),
        }
    }

    /// Load all `.toml` files from `<cache_dir>/plugins/` into a `Registry`.
    ///
    /// Files that fail to parse are warned about and skipped; a partially-loaded
    /// registry is still useful.
    pub fn load_from_cache(cache_dir: &Path) -> Result<Self> {
        let plugins_dir = cache_dir.join("plugins");
        debug!("Loading registry from {}", plugins_dir.display());

        let mut registry = Self::new();

        if !plugins_dir.exists() {
            debug!(
                "Registry plugins directory does not exist: {}",
                plugins_dir.display()
            );
            return Ok(registry);
        }

        for entry in WalkDir::new(&plugins_dir)
            .into_iter()
            .filter_entry(|entry| !is_hidden(entry.path()))
        {
            let entry = entry.with_context(|| {
                format!("Cannot read directory entry in {}", plugins_dir.display())
            })?;
            let path = entry.path();

            if !entry.file_type().is_file()
                || path.extension().and_then(|e| e.to_str()) != Some("toml")
            {
                continue;
            }

            match load_plugin_toml(path) {
                Ok(plugin) => {
                    debug!("Loaded plugin: {}", plugin.slug);
                    registry.plugins.insert(plugin.slug.clone(), plugin);
                }
                Err(e) => {
                    tracing::warn!("Skipping {}: {e}", path.display());
                }
            }
        }

        debug!("Loaded {} plugins from cache", registry.plugins.len());

        // Load shared bundle ID mappings from the registry repo.
        // These are crowdsourced from user scans and committed to the registry.
        let bundle_ids_path = cache_dir.join("bundle_ids.toml");
        if bundle_ids_path.exists() {
            if let Ok(content) = std::fs::read_to_string(&bundle_ids_path) {
                registry.apply_shared_bundle_ids(&content);
            }
        }

        let installers_path = cache_dir.join("installers.toml");
        if installers_path.exists() {
            match load_installers_toml(&installers_path) {
                Ok(installers) => registry.installers = installers,
                Err(error) => {
                    tracing::warn!("Skipping installers {}: {error}", installers_path.display())
                }
            }
        }

        Ok(registry)
    }

    /// Load and merge plugins (and bundles) from all configured sources.
    ///
    /// Sources are processed in order; later sources override earlier ones
    /// on slug collision (non-default sources take precedence, allowing
    /// community overrides).
    pub fn load_all_sources(config: &Config) -> Result<Self> {
        let sources = config.sources();
        let mut merged = Self::new();

        for source in &sources {
            let source_cache = config.registries_cache_dir().join(&source.name);

            // For local filesystem sources, load directly from the path if the
            // cache symlink hasn't been created yet (allows `apm install` to
            // work without requiring `apm sync` first).
            let effective_path = if source_cache.exists() {
                source_cache.clone()
            } else if let Some(local) = sync::local_path(&source.url) {
                debug!(
                    "Loading source '{}' directly from local path {}",
                    source.name,
                    local.display()
                );
                local
            } else {
                source_cache.clone()
            };

            debug!(
                "Loading source '{}' from {}",
                source.name,
                effective_path.display()
            );

            match Self::load_from_cache(&effective_path) {
                Ok(mut registry) => {
                    for plugin in registry.plugins.values_mut() {
                        plugin.source_name = Some(source.name.clone());
                    }
                    merged
                        .plugins_by_source
                        .insert(source.name.clone(), registry.plugins.clone());
                    merged.plugins.extend(registry.plugins);
                    merged.installers.extend(registry.installers);
                }
                Err(e) => {
                    tracing::warn!("Could not load source '{}': {e}", source.name);
                }
            }

            // Also load bundles from this source.
            merged.load_bundles_from_cache(&effective_path);
        }

        Ok(merged)
    }

    /// Load bundle TOML files from `<cache_dir>/bundles/*.toml` into this registry.
    ///
    /// Files that fail to parse are warned about and skipped.
    pub fn load_bundles_from_cache(&mut self, cache_dir: &Path) {
        let bundles_dir = cache_dir.join("bundles");
        debug!("Loading bundles from {}", bundles_dir.display());

        if !bundles_dir.exists() {
            debug!(
                "Bundles directory does not exist: {}",
                bundles_dir.display()
            );
            return;
        }

        let entries = match std::fs::read_dir(&bundles_dir) {
            Ok(e) => e,
            Err(e) => {
                tracing::warn!(
                    "Cannot read bundles directory {}: {e}",
                    bundles_dir.display()
                );
                return;
            }
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("toml") {
                continue;
            }
            match load_bundle_toml(&path) {
                Ok(bundle) => {
                    debug!("Loaded bundle: {}", bundle.slug);
                    self.bundles.insert(bundle.slug.clone(), bundle);
                }
                Err(e) => {
                    tracing::warn!("Skipping bundle {}: {e}", path.display());
                }
            }
        }

        debug!("Loaded {} bundles from cache", self.bundles.len());
    }

    /// Find a bundle by slug (exact, case-insensitive).
    pub fn find_bundle(&self, slug: &str) -> Option<&PluginBundle> {
        if let Some(b) = self.bundles.get(slug) {
            return Some(b);
        }
        let lower = slug.to_lowercase();
        self.bundles
            .values()
            .find(|b| b.slug.to_lowercase() == lower)
    }

    /// Find a plugin by slug (exact, case-insensitive).
    pub fn find(&self, slug: &str) -> Option<&PluginDefinition> {
        // Try exact match first.
        if let Some(p) = self.plugins.get(slug) {
            return Some(p);
        }
        // Case-insensitive fallback.
        let lower = slug.to_lowercase();
        self.plugins.values().find(|p| {
            p.slug.to_lowercase() == lower
                || p.aliases.iter().any(|alias| alias.to_lowercase() == lower)
        })
    }

    /// Find a plugin by source name and slug (both case-insensitive).
    pub fn find_in_source(&self, source_name: &str, slug: &str) -> Option<&PluginDefinition> {
        let source = self
            .plugins_by_source
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(source_name))
            .map(|(_, plugins)| plugins)?;

        if let Some(plugin) = source.get(slug) {
            return Some(plugin);
        }

        let lower = slug.to_lowercase();
        source
            .values()
            .find(|plugin| plugin.slug.to_lowercase() == lower)
    }

    /// Apply shared bundle ID mappings from a `bundle_ids.toml` file.
    ///
    /// Format:
    /// ```toml
    /// [mappings]
    /// "com.fabfilter.Pro-Q" = "pro-q3"
    /// "com.soundtoys.audiounit.EchoBoy" = "echoboy"
    /// ```
    fn apply_shared_bundle_ids(&mut self, content: &str) {
        #[derive(serde::Deserialize)]
        struct SharedBundleIds {
            #[serde(default)]
            mappings: std::collections::HashMap<String, String>,
        }

        let shared: SharedBundleIds = match toml::from_str(content) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("Failed to parse bundle_ids.toml: {e}");
                return;
            }
        };

        let mut applied = 0usize;
        for (bundle_id_prefix, slug) in &shared.mappings {
            if let Some(plugin) = self.plugins.get_mut(slug) {
                if !plugin.bundle_ids.contains(bundle_id_prefix) {
                    plugin.bundle_ids.push(bundle_id_prefix.clone());
                    applied += 1;
                }
            }
        }
        debug!("Applied {applied} shared bundle ID mappings");
    }

    /// Returns all plugin definitions as a slice-like iterator.
    pub fn all(&self) -> Vec<&PluginDefinition> {
        self.plugins.values().collect()
    }

    /// Find an installer definition by key (exact, case-insensitive).
    pub fn find_installer(&self, key: &str) -> Option<&InstallerDefinition> {
        if let Some(installer) = self.installers.get(key) {
            return Some(installer);
        }

        let lower = key.to_lowercase();
        self.installers
            .values()
            .find(|installer| installer.key.to_lowercase() == lower)
    }

    /// Total number of plugins in this registry.
    pub fn len(&self) -> usize {
        self.plugins.len()
    }

    /// Returns true if the registry has no plugins.
    pub fn is_empty(&self) -> bool {
        self.plugins.is_empty()
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn is_hidden(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.starts_with('.'))
        .unwrap_or(false)
}

/// Parse a single plugin TOML file into a `PluginDefinition`.
fn load_plugin_toml(path: &Path) -> Result<PluginDefinition> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("Cannot read plugin file: {}", path.display()))?;

    toml::from_str(&raw).map_err(|e| {
        anyhow::anyhow!(
            "TOML parse error in {}:\n  {}\nHint: Fix the syntax error in the registry file.",
            path.display(),
            e
        )
    })
}

/// Parse a single bundle TOML file into a `PluginBundle`.
fn load_bundle_toml(path: &Path) -> Result<PluginBundle> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("Cannot read bundle file: {}", path.display()))?;

    toml::from_str(&raw).map_err(|e| {
        anyhow::anyhow!(
            "TOML parse error in {}:\n  {}\nHint: Fix the syntax error in the bundle file.",
            path.display(),
            e
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, SourceEntry};

    fn write_plugin(cache_root: &Path, source: &str, slug: &str, name: &str, version: &str) {
        let plugins_dir = cache_root
            .join("apm/registries")
            .join(source)
            .join("plugins");
        std::fs::create_dir_all(&plugins_dir).unwrap();
        let body = format!(
            r#"
slug = "{slug}"
name = "{name}"
vendor = "Acme"
version = "{version}"
description = "Fixture plugin"
category = "effect"
license = "freeware"

[formats.au]
url = "https://example.com/{slug}.zip"
sha256 = "manual"
install_type = "zip"
"#
        );
        std::fs::write(plugins_dir.join(format!("{slug}.toml")), body).unwrap();
    }

    #[test]
    fn load_all_sources_tracks_source_specific_provenance() {
        let temp = std::env::temp_dir().join(format!("apm-registry-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp);
        std::fs::create_dir_all(&temp).unwrap();
        let mut config = Config {
            cache_dir: Some(temp.join("apm")),
            ..Config::default()
        };
        config.sources.push(SourceEntry {
            name: "community".to_string(),
            url: "https://example.com/community.git".to_string(),
        });

        write_plugin(
            &temp,
            "official",
            "shared-plugin",
            "Official Shared",
            "1.0.0",
        );
        write_plugin(
            &temp,
            "community",
            "shared-plugin",
            "Community Shared",
            "2.0.0",
        );

        let registry = Registry::load_all_sources(&config).unwrap();

        let merged = registry.find("shared-plugin").unwrap();
        assert_eq!(merged.name, "Community Shared");
        assert_eq!(merged.source_name.as_deref(), Some("community"));

        let official = registry
            .find_in_source("official", "shared-plugin")
            .unwrap();
        assert_eq!(official.name, "Official Shared");
        assert_eq!(official.source_name.as_deref(), Some("official"));

        std::fs::remove_dir_all(&temp).unwrap();
    }

    #[test]
    fn test_find_case_insensitive() {
        let mut registry = Registry::new();
        registry.plugins.insert(
            "tal-noisemaker".to_string(),
            PluginDefinition {
                slug: "tal-noisemaker".to_string(),
                name: "TAL-NoiseMaker".to_string(),
                vendor: "TAL Software".to_string(),
                version: "1.0.0".to_string(),
                description: "Virtual analog synth".to_string(),
                category: "instrument".to_string(),
                product_type: ProductType::Plugin,
                subcategory: None,
                license: "freeware".to_string(),
                tags: vec![],
                aliases: vec![],
                installer: None,
                formats: std::collections::HashMap::new(),
                releases: vec![],
                homepage: None,
                purchase_url: None,
                bundle_ids: vec![],
                is_paid: false,
                price_cents: None,
                currency: None,
                source_name: None,
            },
        );

        // Upper-case lookup should still find the lower-case keyed plugin.
        let found = registry.find("TAL-NOISEMAKER");
        assert!(found.is_some(), "case-insensitive find should match");
        assert_eq!(found.unwrap().slug, "tal-noisemaker");
    }

    #[test]
    fn test_find_matches_alias_slug() {
        let mut registry = Registry::new();
        registry.plugins.insert(
            "echoboy".to_string(),
            PluginDefinition {
                slug: "echoboy".to_string(),
                name: "EchoBoy".to_string(),
                vendor: "Soundtoys".to_string(),
                version: "1.0.0".to_string(),
                description: "Delay plugin".to_string(),
                category: "effects".to_string(),
                product_type: ProductType::Plugin,
                subcategory: None,
                license: "commercial".to_string(),
                tags: vec![],
                aliases: vec!["soundtoys-echoboy".to_string()],
                installer: None,
                formats: std::collections::HashMap::new(),
                releases: vec![],
                homepage: None,
                purchase_url: None,
                bundle_ids: vec![],
                is_paid: false,
                price_cents: None,
                currency: None,
                source_name: None,
            },
        );

        let found = registry.find("soundtoys-echoboy");
        assert!(found.is_some(), "alias lookup should resolve");
        assert_eq!(found.unwrap().slug, "echoboy");
    }

    #[test]
    fn test_load_from_empty_dir() {
        let temp = std::env::temp_dir().join(format!("apm-empty-dir-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp);

        // Create the cache directory with an empty plugins/ subdirectory.
        let plugins_dir = temp.join("plugins");
        std::fs::create_dir_all(&plugins_dir).unwrap();

        let registry = Registry::load_from_cache(&temp).unwrap();
        assert!(
            registry.plugins.is_empty(),
            "registry loaded from empty plugins dir should have no plugins"
        );

        std::fs::remove_dir_all(&temp).unwrap();
    }

    #[test]
    fn test_loads_plugins_from_nested_vendor_directories() {
        let temp = std::env::temp_dir().join(format!("apm-nested-dir-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp);

        let plugins_dir = temp.join("plugins/native-instruments");
        std::fs::create_dir_all(&plugins_dir).unwrap();
        std::fs::write(
            plugins_dir.join("massive-x.toml"),
            r#"
slug = "massive-x"
name = "Massive X"
vendor = "Native Instruments"
version = "1.0.0"
description = "Fixture plugin"
category = "instrument"
license = "commercial"

[formats.vst3]
url = "https://example.com/massive-x.zip"
sha256 = "manual"
install_type = "zip"
"#,
        )
        .unwrap();

        let registry = Registry::load_from_cache(&temp).unwrap();
        assert!(registry.find("massive-x").is_some());

        std::fs::remove_dir_all(&temp).unwrap();
    }
}
