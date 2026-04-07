// State management is the full install-tracking API used by later phases.
// Phase 1 defines the schema; later phases wire up load/save/mutate.
// The dead_code lint is suppressed here intentionally.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tracing::debug;

use crate::config::Config;
use crate::registry::PluginFormat;

// ── InstalledFormat ───────────────────────────────────────────────────────────

/// A single installed bundle (one per format) for a managed plugin.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledFormat {
    /// The plugin format this bundle provides.
    pub format: PluginFormat,

    /// Absolute path to the installed `.component` or `.vst3` bundle.
    pub path: PathBuf,

    /// SHA256 hex digest of the installed bundle (for tamper detection).
    pub sha256: String,
}

// ── InstalledPlugin ───────────────────────────────────────────────────────────

/// A plugin that was installed by apm and tracked in state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledPlugin {
    /// Plugin slug, matching `PluginDefinition::slug`.
    pub name: String,

    /// Installed version string.
    pub version: String,

    /// Vendor / developer name.
    pub vendor: String,

    /// Installed bundles, one per format.
    pub formats: Vec<InstalledFormat>,

    /// UTC timestamp when the plugin was installed.
    pub installed_at: DateTime<Utc>,

    /// Registry source name from which the plugin was installed.
    pub source: String,

    /// If true, `apm upgrade` will skip this plugin.
    #[serde(default)]
    pub pinned: bool,
}

// ── InstallState ──────────────────────────────────────────────────────────────

/// Root of the state file (`~/.local/share/apm/state.toml`).
///
/// Written atomically (write to a temp file, rename into place) to prevent
/// corruption if the process is interrupted mid-write.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstallState {
    /// Schema version — increment when the format changes.
    #[serde(default = "default_schema_version")]
    pub version: u32,

    /// All plugins installed and tracked by apm.
    #[serde(default)]
    pub plugins: Vec<InstalledPlugin>,
}

fn default_schema_version() -> u32 {
    1
}

impl Default for InstallState {
    fn default() -> Self {
        Self {
            version: default_schema_version(),
            plugins: Vec::new(),
        }
    }
}

impl InstallState {
    // ── Loading ───────────────────────────────────────────────────────────────

    /// Load state from the path specified in `config`, or return an empty
    /// state if the file does not exist yet.
    pub fn load(config: &Config) -> Result<Self> {
        let path = config.state_file();
        Self::load_from(&path)
    }

    /// Load state from an explicit path.
    pub fn load_from(path: &Path) -> Result<Self> {
        if !path.exists() {
            debug!(
                "State file not found at {}; starting with empty state.",
                path.display()
            );
            return Ok(Self::default());
        }

        debug!("Loading state from {}", path.display());
        let raw = std::fs::read_to_string(path)
            .with_context(|| format!("Cannot read state file: {}", path.display()))?;

        toml::from_str(&raw).map_err(|e| {
            anyhow::anyhow!(
                "TOML parse error in {}:\n  {}\n\
                 Hint: Back up {} and delete it to reset.\n      \
                 Installed plugins will still be on disk — run `apm scan` to find them.",
                path.display(),
                e,
                path.display()
            )
        })
    }

    // ── Saving ────────────────────────────────────────────────────────────────

    /// Save state to the path specified in `config`, using an atomic write.
    pub fn save(&self, config: &Config) -> Result<()> {
        let path = config.state_file();
        self.save_to(&path)
    }

    /// Save state to an explicit path, writing atomically via a temp file and
    /// rename to prevent corruption on interrupted writes.
    pub fn save_to(&self, path: &Path) -> Result<()> {
        // Ensure the parent directory exists.
        if let Some(parent) = path.parent() {
            crate::config::ensure_dir(parent)
                .with_context(|| format!("Cannot create data directory: {}", parent.display()))?;
        }

        let content =
            toml::to_string_pretty(self).context("Failed to serialise install state to TOML")?;

        // Atomic write: write to a sibling temp file, then rename.
        let tmp_path = path.with_extension("toml.tmp");
        std::fs::write(&tmp_path, content)
            .with_context(|| format!("Cannot write temp state file: {}", tmp_path.display()))?;

        std::fs::rename(&tmp_path, path)
            .with_context(|| format!("Cannot rename state file into place: {}", path.display()))?;

        debug!("State saved to {}", path.display());
        Ok(())
    }

    // ── Queries ───────────────────────────────────────────────────────────────

    /// Find an installed plugin by slug (case-insensitive).
    pub fn find(&self, slug: &str) -> Option<&InstalledPlugin> {
        self.plugins
            .iter()
            .find(|p| p.name.eq_ignore_ascii_case(slug))
    }

    /// Find a mutable reference to an installed plugin by slug (case-insensitive).
    pub fn find_mut(&mut self, slug: &str) -> Option<&mut InstalledPlugin> {
        self.plugins
            .iter_mut()
            .find(|p| p.name.eq_ignore_ascii_case(slug))
    }

    /// Return true if a plugin with the given slug is installed.
    pub fn is_installed(&self, slug: &str) -> bool {
        self.find(slug).is_some()
    }

    // ── Mutations ─────────────────────────────────────────────────────────────

    /// Record a newly installed plugin. Replaces any existing entry with the
    /// same slug.
    pub fn record_install(&mut self, plugin: InstalledPlugin) {
        if let Some(existing) = self.find_mut(&plugin.name.clone()) {
            *existing = plugin;
        } else {
            self.plugins.push(plugin);
        }
        // Keep sorted for deterministic diffs.
        self.plugins.sort_by(|a, b| a.name.cmp(&b.name));
    }

    /// Remove a plugin by slug. Returns the removed entry if it existed.
    pub fn remove(&mut self, slug: &str) -> Option<InstalledPlugin> {
        if let Some(pos) = self
            .plugins
            .iter()
            .position(|p| p.name.eq_ignore_ascii_case(slug))
        {
            Some(self.plugins.remove(pos))
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use tempfile::TempDir;

    /// Build a minimal `InstalledPlugin` for testing.
    fn make_plugin(name: &str, version: &str) -> InstalledPlugin {
        InstalledPlugin {
            name: name.to_string(),
            version: version.to_string(),
            vendor: "TestVendor".to_string(),
            formats: vec![],
            installed_at: Utc::now(),
            source: "official".to_string(),
            pinned: false,
        }
    }

    #[test]
    fn test_record_install_replaces_existing() {
        let mut state = InstallState::default();
        state.record_install(make_plugin("reverb-pro", "1.0.0"));
        state.record_install(make_plugin("reverb-pro", "2.0.0"));

        assert_eq!(state.plugins.len(), 1, "duplicate should be replaced, not appended");
        assert_eq!(state.plugins[0].version, "2.0.0");
    }

    #[test]
    fn test_pin_survives_save_load() {
        let tmp = TempDir::new().unwrap();
        let state_path = tmp.path().join("state.toml");

        let mut state = InstallState::default();
        let mut plugin = make_plugin("compressor-x", "3.1.0");
        plugin.pinned = true;
        state.record_install(plugin);
        state.save_to(&state_path).unwrap();

        let loaded = InstallState::load_from(&state_path).unwrap();
        let found = loaded.find("compressor-x").expect("plugin should exist after reload");
        assert!(found.pinned, "pinned flag should survive save/load round-trip");
    }

    #[test]
    fn test_remove_nonexistent_returns_none() {
        let mut state = InstallState::default();
        state.record_install(make_plugin("delay-unit", "1.0.0"));

        let result = state.remove("i-do-not-exist");
        assert!(result.is_none(), "removing nonexistent plugin should return None");
        assert_eq!(state.plugins.len(), 1, "existing plugins should be untouched");
    }

    #[test]
    fn test_find_case_insensitive() {
        let mut state = InstallState::default();
        state.record_install(make_plugin("tal-noisemaker", "4.0.0"));

        let found = state.find("TAL-NoiseMaker");
        assert!(found.is_some(), "find should be case-insensitive");
        assert_eq!(found.unwrap().name, "tal-noisemaker");
    }

    #[test]
    fn test_empty_state_load_from_missing_file() {
        let tmp = TempDir::new().unwrap();
        let missing = tmp.path().join("does-not-exist.toml");

        let state = InstallState::load_from(&missing).unwrap();
        assert!(state.plugins.is_empty(), "loading missing file should yield empty state");
        assert_eq!(state.version, 1, "default schema version should be 1");
    }

    #[test]
    fn test_save_atomic_creates_parent_dirs() {
        let tmp = TempDir::new().unwrap();
        let nested_path = tmp.path().join("a").join("b").join("c").join("state.toml");

        let state = InstallState::default();
        state.save_to(&nested_path).unwrap();

        assert!(nested_path.exists(), "state file should exist after save");
        let loaded = InstallState::load_from(&nested_path).unwrap();
        assert!(loaded.plugins.is_empty());
    }

    #[test]
    fn test_plugins_sorted_after_record() {
        let mut state = InstallState::default();
        state.record_install(make_plugin("zebra-synth", "1.0.0"));
        state.record_install(make_plugin("analog-lab", "2.0.0"));
        state.record_install(make_plugin("massive-x", "1.5.0"));

        let names: Vec<&str> = state.plugins.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, vec!["analog-lab", "massive-x", "zebra-synth"]);
    }
}
