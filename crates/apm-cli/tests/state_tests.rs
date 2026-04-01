// Integration tests for state management — creating, mutating, persisting,
// and querying the install state. These tests replicate the InstallState
// logic directly, exercising TOML serialization and file I/O.

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// ── State types (mirrors src/state.rs and src/registry/types.rs) ──────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum PluginFormat {
    Au,
    Vst3,
}

impl std::fmt::Display for PluginFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Au => write!(f, "AU"),
            Self::Vst3 => write!(f, "VST3"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct InstalledFormat {
    format: PluginFormat,
    path: PathBuf,
    sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct InstalledPlugin {
    name: String,
    version: String,
    vendor: String,
    formats: Vec<InstalledFormat>,
    installed_at: DateTime<Utc>,
    source: String,
    #[serde(default)]
    pinned: bool,
}

fn default_schema_version() -> u32 { 1 }

#[derive(Debug, Clone, Serialize, Deserialize)]
struct InstallState {
    #[serde(default = "default_schema_version")]
    version: u32,
    #[serde(default)]
    plugins: Vec<InstalledPlugin>,
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
    fn load_from(path: &Path) -> anyhow::Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let raw = std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("Cannot read state file: {e}"))?;
        toml::from_str(&raw).map_err(|e| anyhow::anyhow!("TOML parse error: {e}"))
    }

    fn save_to(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(parent) = path.parent() {
            if !parent.exists() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| anyhow::anyhow!("Cannot create dir: {e}"))?;
            }
        }
        let content = toml::to_string_pretty(self)
            .map_err(|e| anyhow::anyhow!("Serialization error: {e}"))?;
        let tmp_path = path.with_extension("toml.tmp");
        std::fs::write(&tmp_path, content)
            .map_err(|e| anyhow::anyhow!("Cannot write temp file: {e}"))?;
        std::fs::rename(&tmp_path, path)
            .map_err(|e| anyhow::anyhow!("Cannot rename state file: {e}"))?;
        Ok(())
    }

    fn find(&self, slug: &str) -> Option<&InstalledPlugin> {
        self.plugins.iter().find(|p| p.name.eq_ignore_ascii_case(slug))
    }

    fn find_mut(&mut self, slug: &str) -> Option<&mut InstalledPlugin> {
        self.plugins.iter_mut().find(|p| p.name.eq_ignore_ascii_case(slug))
    }

    fn is_installed(&self, slug: &str) -> bool {
        self.find(slug).is_some()
    }

    fn record_install(&mut self, plugin: InstalledPlugin) {
        if let Some(existing) = self.find_mut(&plugin.name.clone()) {
            *existing = plugin;
        } else {
            self.plugins.push(plugin);
        }
        self.plugins.sort_by(|a, b| a.name.cmp(&b.name));
    }

    fn remove(&mut self, slug: &str) -> Option<InstalledPlugin> {
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

// ── Test helpers ──────────────────────────────────────────────────────────────

fn make_plugin(name: &str, version: &str) -> InstalledPlugin {
    InstalledPlugin {
        name: name.to_string(),
        version: version.to_string(),
        vendor: "Test Vendor".to_string(),
        formats: vec![InstalledFormat {
            format: PluginFormat::Vst3,
            path: PathBuf::from(format!("/tmp/{name}.vst3")),
            sha256: "deadbeef".to_string(),
        }],
        installed_at: Utc::now(),
        source: "official".to_string(),
        pinned: false,
    }
}

// ── Empty state ───────────────────────────────────────────────────────────────

#[test]
fn test_empty_state_has_no_plugins() {
    let state = InstallState::default();
    assert!(state.plugins.is_empty());
}

#[test]
fn test_empty_state_version_is_one() {
    let state = InstallState::default();
    assert_eq!(state.version, 1);
}

#[test]
fn test_load_from_nonexistent_file_returns_empty() {
    let path = PathBuf::from("/tmp/apm-test-nonexistent-state-file-xyz.toml");
    let state = InstallState::load_from(&path).expect("should return empty state");
    assert!(state.plugins.is_empty());
}

// ── Adding plugins ────────────────────────────────────────────────────────────

#[test]
fn test_record_install_adds_plugin() {
    let mut state = InstallState::default();
    state.record_install(make_plugin("my-reverb", "1.0.0"));
    assert_eq!(state.plugins.len(), 1);
    assert_eq!(state.plugins[0].name, "my-reverb");
}

#[test]
fn test_record_install_multiple_plugins() {
    let mut state = InstallState::default();
    state.record_install(make_plugin("plugin-a", "1.0.0"));
    state.record_install(make_plugin("plugin-b", "2.0.0"));
    assert_eq!(state.plugins.len(), 2);
}

#[test]
fn test_record_install_replaces_existing() {
    let mut state = InstallState::default();
    state.record_install(make_plugin("my-synth", "1.0.0"));
    state.record_install(make_plugin("my-synth", "2.0.0"));
    assert_eq!(state.plugins.len(), 1);
    assert_eq!(state.plugins[0].version, "2.0.0");
}

#[test]
fn test_plugins_are_sorted_after_install() {
    let mut state = InstallState::default();
    state.record_install(make_plugin("zzz-last", "1.0.0"));
    state.record_install(make_plugin("aaa-first", "1.0.0"));
    assert_eq!(state.plugins[0].name, "aaa-first");
    assert_eq!(state.plugins[1].name, "zzz-last");
}

// ── Removing plugins ──────────────────────────────────────────────────────────

#[test]
fn test_remove_existing_plugin() {
    let mut state = InstallState::default();
    state.record_install(make_plugin("my-reverb", "1.0.0"));
    let removed = state.remove("my-reverb");
    assert!(removed.is_some());
    assert_eq!(removed.unwrap().name, "my-reverb");
    assert!(state.plugins.is_empty());
}

#[test]
fn test_remove_nonexistent_plugin_returns_none() {
    let mut state = InstallState::default();
    let removed = state.remove("does-not-exist");
    assert!(removed.is_none());
}

#[test]
fn test_remove_leaves_other_plugins_intact() {
    let mut state = InstallState::default();
    state.record_install(make_plugin("plugin-a", "1.0.0"));
    state.record_install(make_plugin("plugin-b", "1.0.0"));
    state.remove("plugin-a");
    assert_eq!(state.plugins.len(), 1);
    assert_eq!(state.plugins[0].name, "plugin-b");
}

// ── Queries ───────────────────────────────────────────────────────────────────

#[test]
fn test_find_plugin_by_name() {
    let mut state = InstallState::default();
    state.record_install(make_plugin("my-compressor", "1.0.0"));
    let found = state.find("my-compressor");
    assert!(found.is_some());
    assert_eq!(found.unwrap().name, "my-compressor");
}

#[test]
fn test_find_plugin_case_insensitive() {
    let mut state = InstallState::default();
    state.record_install(make_plugin("my-compressor", "1.0.0"));
    assert!(state.find("MY-COMPRESSOR").is_some());
    assert!(state.find("My-Compressor").is_some());
    assert!(state.find("my-compressor").is_some());
}

#[test]
fn test_find_nonexistent_returns_none() {
    let state = InstallState::default();
    assert!(state.find("ghost-plugin").is_none());
}

#[test]
fn test_is_installed_returns_true_for_installed_plugin() {
    let mut state = InstallState::default();
    state.record_install(make_plugin("my-reverb", "1.0.0"));
    assert!(state.is_installed("my-reverb"));
}

#[test]
fn test_is_installed_returns_false_for_missing_plugin() {
    let state = InstallState::default();
    assert!(!state.is_installed("ghost-plugin"));
}

// ── Pinning ───────────────────────────────────────────────────────────────────

#[test]
fn test_pin_plugin() {
    let mut state = InstallState::default();
    state.record_install(make_plugin("my-synth", "1.0.0"));
    state.find_mut("my-synth").unwrap().pinned = true;
    assert!(state.find("my-synth").unwrap().pinned);
}

#[test]
fn test_unpin_plugin() {
    let mut state = InstallState::default();
    let mut plugin = make_plugin("my-synth", "1.0.0");
    plugin.pinned = true;
    state.record_install(plugin);
    state.find_mut("my-synth").unwrap().pinned = false;
    assert!(!state.find("my-synth").unwrap().pinned);
}

#[test]
fn test_new_plugins_are_not_pinned_by_default() {
    let mut state = InstallState::default();
    state.record_install(make_plugin("my-reverb", "1.0.0"));
    assert!(!state.find("my-reverb").unwrap().pinned);
}

// ── Persistence ───────────────────────────────────────────────────────────────

#[test]
fn test_save_and_reload_state() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let state_path = tmp.path().join("state.toml");

    let mut state = InstallState::default();
    state.record_install(make_plugin("saved-plugin", "3.0.0"));

    state.save_to(&state_path).expect("save should succeed");
    assert!(state_path.exists(), "state file should be created");

    let loaded = InstallState::load_from(&state_path).expect("reload should succeed");
    assert_eq!(loaded.plugins.len(), 1);
    assert_eq!(loaded.plugins[0].name, "saved-plugin");
    assert_eq!(loaded.plugins[0].version, "3.0.0");
}

#[test]
fn test_save_creates_parent_directories() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let nested_path = tmp.path().join("deep/nested/dir/state.toml");

    let state = InstallState::default();
    state.save_to(&nested_path).expect("save should create parent dirs");
    assert!(nested_path.exists());
}

#[test]
fn test_save_and_reload_preserves_pinned_state() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let state_path = tmp.path().join("state.toml");

    let mut state = InstallState::default();
    let mut plugin = make_plugin("pinned-plugin", "1.0.0");
    plugin.pinned = true;
    state.record_install(plugin);

    state.save_to(&state_path).expect("save");
    let loaded = InstallState::load_from(&state_path).expect("reload");
    assert!(loaded.find("pinned-plugin").unwrap().pinned);
}

#[test]
fn test_save_and_reload_preserves_multiple_formats() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let state_path = tmp.path().join("state.toml");

    let mut state = InstallState::default();
    let plugin = InstalledPlugin {
        name: "multi-format".to_string(),
        version: "1.0.0".to_string(),
        vendor: "Vendor".to_string(),
        formats: vec![
            InstalledFormat {
                format: PluginFormat::Vst3,
                path: PathBuf::from("/tmp/multi-format.vst3"),
                sha256: "aaa".to_string(),
            },
            InstalledFormat {
                format: PluginFormat::Au,
                path: PathBuf::from("/tmp/multi-format.component"),
                sha256: "bbb".to_string(),
            },
        ],
        installed_at: Utc::now(),
        source: "official".to_string(),
        pinned: false,
    };
    state.record_install(plugin);

    state.save_to(&state_path).expect("save");
    let loaded = InstallState::load_from(&state_path).expect("reload");
    assert_eq!(loaded.plugins[0].formats.len(), 2);
}

#[test]
fn test_state_version_field_is_preserved() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let state_path = tmp.path().join("state.toml");

    let state = InstallState::default();
    state.save_to(&state_path).expect("save");

    let loaded = InstallState::load_from(&state_path).expect("reload");
    assert_eq!(loaded.version, 1);
}

#[test]
fn test_state_vendor_field_is_preserved() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let state_path = tmp.path().join("state.toml");

    let mut state = InstallState::default();
    state.record_install(make_plugin("test-plugin", "1.0.0"));
    state.save_to(&state_path).expect("save");

    let loaded = InstallState::load_from(&state_path).expect("reload");
    assert_eq!(loaded.plugins[0].vendor, "Test Vendor");
}

#[test]
fn test_state_source_field_is_preserved() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let state_path = tmp.path().join("state.toml");

    let mut state = InstallState::default();
    state.record_install(make_plugin("test-plugin", "1.0.0"));
    state.save_to(&state_path).expect("save");

    let loaded = InstallState::load_from(&state_path).expect("reload");
    assert_eq!(loaded.plugins[0].source, "official");
}

#[test]
fn test_state_atomic_write_succeeds() {
    // Test that save_to leaves no temp file behind on success.
    let tmp = tempfile::tempdir().expect("create temp dir");
    let state_path = tmp.path().join("state.toml");

    let state = InstallState::default();
    state.save_to(&state_path).expect("save");

    let tmp_path = state_path.with_extension("toml.tmp");
    assert!(!tmp_path.exists(), "temp file should be cleaned up after save");
}
