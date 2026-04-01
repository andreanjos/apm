pub mod search;
pub mod sync;
pub mod types;

use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result};
use tracing::debug;

use crate::config::Config;

// Re-export all registry types at the crate-module boundary so that future
// phases can import them as `use apm::registry::PluginDefinition` etc. without
// digging into the internal `types` submodule.
#[allow(unused_imports)]
pub use types::{
    FormatSource, InstallType, PluginDefinition, PluginFormat, RegistryIndex, RegistryIndexEntry,
    Source,
};

// ── Registry ──────────────────────────────────────────────────────────────────

/// An in-memory collection of plugin definitions loaded from one or more
/// registry sources. Keyed by plugin slug.
#[derive(Debug, Default)]
pub struct Registry {
    /// All known plugins, keyed by slug (e.g. `"valhalla-supermassive"`).
    pub plugins: HashMap<String, PluginDefinition>,
}

impl Registry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            plugins: HashMap::new(),
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

        let entries = std::fs::read_dir(&plugins_dir).with_context(|| {
            format!("Cannot read registry plugins directory: {}", plugins_dir.display())
        })?;

        for entry in entries {
            let entry = entry.with_context(|| {
                format!("Cannot read directory entry in {}", plugins_dir.display())
            })?;
            let path = entry.path();

            if path.extension().and_then(|e| e.to_str()) != Some("toml") {
                continue;
            }

            match load_plugin_toml(&path) {
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
        Ok(registry)
    }

    /// Load and merge plugins from all configured sources.
    ///
    /// Sources are processed in order; later sources override earlier ones
    /// on slug collision (non-default sources take precedence, allowing
    /// community overrides).
    pub fn load_all_sources(config: &Config) -> Result<Self> {
        let sources = config.sources();
        let mut merged = Self::new();

        for source in &sources {
            let source_cache = config.registries_cache_dir().join(&source.name);
            debug!(
                "Loading source '{}' from {}",
                source.name,
                source_cache.display()
            );

            match Self::load_from_cache(&source_cache) {
                Ok(registry) => {
                    merged.plugins.extend(registry.plugins);
                }
                Err(e) => {
                    tracing::warn!("Could not load source '{}': {e}", source.name);
                }
            }
        }

        Ok(merged)
    }

    /// Find a plugin by slug (exact, case-insensitive).
    pub fn find(&self, slug: &str) -> Option<&PluginDefinition> {
        // Try exact match first.
        if let Some(p) = self.plugins.get(slug) {
            return Some(p);
        }
        // Case-insensitive fallback.
        let lower = slug.to_lowercase();
        self.plugins
            .values()
            .find(|p| p.slug.to_lowercase() == lower)
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
