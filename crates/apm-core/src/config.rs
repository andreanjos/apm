// Platform path constants and helper functions shared by the CLI, scanner,
// installer, and registry cache code.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tracing::{debug, info};

use crate::registry::Source;

/// Built-in official registry source.
///
/// The public repo is a monorepo; registry data lives under `registry/`.
/// The registry loader accepts both dedicated registry repos and this layout.
pub const DEFAULT_REGISTRY_URL: &str = "https://github.com/andreanjos/apm";

// ── macOS Plugin Path Constants ───────────────────────────────────────────────

/// System-wide AU (Audio Units) plugin directory.
pub const SYSTEM_AU_DIR: &str = "/Library/Audio/Plug-Ins/Components";

/// System-wide VST3 plugin directory.
pub const SYSTEM_VST3_DIR: &str = "/Library/Audio/Plug-Ins/VST3";

// ── XDG / macOS Path Helpers ─────────────────────────────────────────────────

/// Returns the user's home directory, panicking if unavailable.
fn home_dir() -> PathBuf {
    dirs::home_dir().expect("Cannot determine home directory")
}

/// `~/.config/apm/` — respects `$XDG_CONFIG_HOME` when set.
pub fn config_dir() -> PathBuf {
    std::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| home_dir().join(".config"))
        .join("apm")
}

/// `~/.local/share/apm/` — respects `$XDG_DATA_HOME` when set.
pub fn data_dir() -> PathBuf {
    std::env::var("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| home_dir().join(".local/share"))
        .join("apm")
}

/// `~/.cache/apm/` — respects `$XDG_CACHE_HOME` when set.
pub fn cache_dir() -> PathBuf {
    std::env::var("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| home_dir().join(".cache"))
        .join("apm")
}

/// User AU plugin directory: `~/Library/Audio/Plug-Ins/Components/`
pub fn user_au_dir() -> PathBuf {
    home_dir().join("Library/Audio/Plug-Ins/Components")
}

/// User VST3 plugin directory: `~/Library/Audio/Plug-Ins/VST3/`
pub fn user_vst3_dir() -> PathBuf {
    home_dir().join("Library/Audio/Plug-Ins/VST3")
}

/// System AU plugin directory (constant).
pub fn system_au_dir() -> PathBuf {
    PathBuf::from(SYSTEM_AU_DIR)
}

/// System VST3 plugin directory (constant).
pub fn system_vst3_dir() -> PathBuf {
    PathBuf::from(SYSTEM_VST3_DIR)
}

// ── Install Scope ─────────────────────────────────────────────────────────────

/// Where to install plugins. Defaults to `User` (no sudo required).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum InstallScope {
    /// `~/Library/Audio/Plug-Ins/` — no elevated privileges required.
    #[default]
    User,
    /// `/Library/Audio/Plug-Ins/` — requires `sudo`.
    System,
}

// ── Config ────────────────────────────────────────────────────────────────────

/// apm user configuration, loaded from `~/.config/apm/config.toml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Default registry URL (cloned by `apm sync`).
    #[serde(default = "default_registry_url")]
    pub default_registry_url: String,

    /// Default install scope. Defaults to `user`.
    #[serde(default)]
    pub install_scope: InstallScope,

    /// Data directory override. Defaults to `~/.local/share/apm/`.
    #[serde(default)]
    pub data_dir: Option<PathBuf>,

    /// Cache directory override. Defaults to `~/.cache/apm/`.
    #[serde(default)]
    pub cache_dir: Option<PathBuf>,

    /// Third-party registry sources (in addition to the default official one).
    #[serde(default)]
    pub sources: Vec<SourceEntry>,
}

/// A registry source stored in config.toml.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SourceEntry {
    /// Short name for display and directory naming (e.g. `"my-registry"`).
    pub name: String,
    /// Git repository URL.
    pub url: String,
}

fn default_registry_url() -> String {
    DEFAULT_REGISTRY_URL.to_string()
}

impl Default for Config {
    fn default() -> Self {
        Self {
            default_registry_url: default_registry_url(),
            install_scope: InstallScope::User,
            data_dir: None,
            cache_dir: None,
            sources: Vec::new(),
        }
    }
}

impl Config {
    /// Returns the resolved data directory (config override or XDG default).
    pub fn resolved_data_dir(&self) -> PathBuf {
        self.data_dir.clone().unwrap_or_else(data_dir)
    }

    /// Returns the resolved cache directory (config override or XDG default).
    pub fn resolved_cache_dir(&self) -> PathBuf {
        self.cache_dir.clone().unwrap_or_else(cache_dir)
    }

    /// Returns the path to the state file (`state.toml`).
    pub fn state_file(&self) -> PathBuf {
        self.resolved_data_dir().join("state.toml")
    }

    /// Returns the directory where registry Git repos are cached.
    pub fn registries_cache_dir(&self) -> PathBuf {
        self.resolved_cache_dir().join("registries")
    }

    /// Returns the staging directory for downloaded archives.
    pub fn downloads_cache_dir(&self) -> PathBuf {
        self.resolved_cache_dir().join("downloads")
    }

    /// Returns the directory where plugin backups are stored.
    ///
    /// Structure: `<backups_dir>/<slug>/<version>/`
    pub fn backups_dir(&self) -> PathBuf {
        self.resolved_data_dir().join("backups")
    }

    /// Returns all configured sources, always including the default official
    /// registry first, followed by any user-added sources.
    pub fn sources(&self) -> Vec<Source> {
        let mut result = vec![Source::official(&self.default_registry_url)];
        for entry in &self.sources {
            result.push(Source {
                name: entry.name.clone(),
                url: entry.url.clone(),
                is_default: false,
            });
        }
        result
    }

    /// Save the current configuration back to `~/.config/apm/config.toml`.
    pub fn save(&self) -> Result<()> {
        let cfg_path = config_dir().join("config.toml");
        write_config(&cfg_path, self)
            .with_context(|| format!("Failed to save config to {}", cfg_path.display()))
    }
}

// ── Load / Init ───────────────────────────────────────────────────────────────

/// Initialise the apm config directory and return a loaded `Config`.
///
/// - Creates `~/.config/apm/` if it does not exist.
/// - If `config.toml` is missing, writes default values and continues.
/// - TOML parse errors include the file path in the error message.
pub fn init() -> Result<Config> {
    let cfg_dir = config_dir();
    ensure_dir(&cfg_dir)
        .with_context(|| format!("Failed to create config directory: {}", cfg_dir.display()))?;

    let cfg_path = cfg_dir.join("config.toml");

    if !cfg_path.exists() {
        info!(
            "No config file found at {}; writing defaults.",
            cfg_path.display()
        );
        let defaults = Config::default();
        write_default_config(&cfg_path, &defaults)
            .with_context(|| format!("Failed to write default config to {}", cfg_path.display()))?;
        return Ok(defaults);
    }

    load_config(&cfg_path)
}

/// Load and parse a `Config` from the given TOML path.
pub fn load_config(path: &Path) -> Result<Config> {
    debug!("Loading config from {}", path.display());
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("Cannot read config file: {}", path.display()))?;

    toml::from_str(&raw).map_err(|e| {
        // toml 0.8 provides span information via `.span()` but the line/column
        // is accessible from the Display output. We surface the path alongside
        // the error so the user knows which file to fix.
        anyhow::anyhow!(
            "TOML parse error in {}:\n  {}\nHint: Fix the syntax error and re-run apm.",
            path.display(),
            e
        )
    })
}

/// Write a default config file with inline comments for discoverability.
fn write_default_config(path: &Path, config: &Config) -> Result<()> {
    write_config(path, config)
}

/// Serialise and write the config to `path`, preserving the hand-written
/// header comments on first write. On subsequent saves we use TOML serialisation
/// directly (comments are not round-tripped, but correctness is what matters).
fn write_config(path: &Path, config: &Config) -> Result<()> {
    let scope_str = match config.install_scope {
        InstallScope::User => "user",
        InstallScope::System => "system",
    };

    let mut content = format!(
        "# apm configuration\n\
         # Edit this file to customise apm behaviour.\n\
         \n\
         # URL of the default plugin registry (a Git repository).\n\
         default_registry_url = \"{registry_url}\"\n\
         \n\
         # Default install scope: \"user\" (~/Library) or \"system\" (/Library, needs sudo).\n\
         install_scope = \"{scope}\"\n",
        registry_url = config.default_registry_url,
        scope = scope_str,
    );

    // Append user-added sources (the default official one is already encoded
    // above via `default_registry_url`).
    if !config.sources.is_empty() {
        content.push('\n');
        for entry in &config.sources {
            content.push_str(&format!(
                "[[sources]]\nname = \"{}\"\nurl = \"{}\"\n\n",
                entry.name, entry.url
            ));
        }
    }

    std::fs::write(path, content)
        .with_context(|| format!("Cannot write config file: {}", path.display()))
}

/// Create a directory and all parents if they do not exist.
pub fn ensure_dir(path: &Path) -> Result<()> {
    if !path.exists() {
        std::fs::create_dir_all(path)
            .with_context(|| format!("Cannot create directory: {}", path.display()))?;
        debug!("Created directory: {}", path.display());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config_has_official_source() {
        let cfg = Config::default();
        let sources = cfg.sources();
        assert!(!sources.is_empty(), "sources should not be empty");
        assert!(
            sources.iter().any(|s| s.name == "official"),
            "sources should contain an entry named 'official'"
        );
    }

    #[test]
    fn test_sources_includes_custom() {
        let mut cfg = Config::default();
        cfg.sources.push(SourceEntry {
            name: "my-registry".to_string(),
            url: "https://example.com/registry".to_string(),
        });

        let sources = cfg.sources();
        assert!(
            sources.iter().any(|s| s.name == "official"),
            "official source should still be present"
        );
        assert!(
            sources.iter().any(|s| s.name == "my-registry"),
            "custom source should be present"
        );
        assert_eq!(sources.len(), 2, "should have exactly 2 sources");
    }

    #[test]
    fn test_resolved_data_dir_with_override() {
        let cfg = Config {
            data_dir: Some(PathBuf::from("/tmp/custom-apm-data")),
            ..Config::default()
        };
        assert_eq!(
            cfg.resolved_data_dir(),
            PathBuf::from("/tmp/custom-apm-data")
        );
    }

    #[test]
    fn test_resolved_data_dir_default() {
        let cfg = Config::default();
        let resolved = cfg.resolved_data_dir();
        // Without override, it should fall back to XDG-based data_dir().
        // We cannot hardcode the exact path because it depends on the user's
        // home directory and XDG_DATA_HOME, but it must end with "apm".
        assert!(
            resolved.ends_with("apm"),
            "default resolved_data_dir should end with 'apm', got: {}",
            resolved.display()
        );
    }

    #[test]
    fn test_load_config_with_missing_fields_uses_defaults() {
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            "default_registry_url = \"https://example.com/reg\"\n",
        )
        .expect("failed to write minimal config");

        let cfg = load_config(&path).expect("load_config should succeed with minimal TOML");
        assert_eq!(cfg.default_registry_url, "https://example.com/reg");
        // Fields not present in the file should get their defaults.
        assert_eq!(cfg.install_scope, InstallScope::User);
        assert!(cfg.data_dir.is_none());
        assert!(cfg.cache_dir.is_none());
        assert!(cfg.sources.is_empty());
    }

    #[test]
    fn test_load_config_with_invalid_toml_gives_actionable_error() {
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let path = dir.path().join("bad.toml");
        std::fs::write(&path, "this is not = = valid toml").expect("failed to write bad config");

        let err = load_config(&path).expect_err("load_config should fail on invalid TOML");
        let msg = format!("{:#}", err);
        assert!(
            msg.contains(&path.display().to_string()),
            "error should contain the file path, got: {msg}"
        );
    }

    #[test]
    fn test_config_save_roundtrip() {
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let path = dir.path().join("roundtrip.toml");

        let original = Config {
            default_registry_url: "https://example.com/rt".to_string(),
            install_scope: InstallScope::System,
            data_dir: None,
            cache_dir: None,
            sources: vec![SourceEntry {
                name: "extra".to_string(),
                url: "https://extra.example.com".to_string(),
            }],
        };

        write_config(&path, &original).expect("write_config should succeed");
        let loaded = load_config(&path).expect("load_config should succeed after save");

        assert_eq!(loaded.default_registry_url, original.default_registry_url);
        assert_eq!(loaded.install_scope, original.install_scope);
        assert_eq!(loaded.sources.len(), original.sources.len());
        assert_eq!(loaded.sources, original.sources);
    }
}
