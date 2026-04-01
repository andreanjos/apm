// Platform path constants and helper functions are public API used by later
// phases of apm. The dead_code lint is suppressed here because this is
// intentional infrastructure — not yet wired up in Phase 1.
#![allow(dead_code)]

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tracing::{debug, info};

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
    /// Default registry URL (cloned via git2 for `apm sync`).
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
}

fn default_registry_url() -> String {
    "https://github.com/apm-pm/registry".to_string()
}

impl Default for Config {
    fn default() -> Self {
        Self {
            default_registry_url: default_registry_url(),
            install_scope: InstallScope::User,
            data_dir: None,
            cache_dir: None,
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
    let scope_str = match config.install_scope {
        InstallScope::User => "user",
        InstallScope::System => "system",
    };

    let content = format!(
        "# apm configuration — auto-generated on first run\n\
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
