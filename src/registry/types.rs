// Registry types are the full TOML schema for the plugin registry.
// Phase 1 defines these types; later phases parse actual TOML files.
// The dead_code lint is suppressed here intentionally.
#![allow(dead_code)]

use serde::{Deserialize, Serialize};

// ── Plugin Format ─────────────────────────────────────────────────────────────

/// The binary format of an audio plugin bundle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PluginFormat {
    /// Audio Units — macOS-native format, `.component` bundles.
    Au,
    /// VST3 — cross-platform, `.vst3` bundles.
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

// ── Install Type ──────────────────────────────────────────────────────────────

/// How a plugin archive should be installed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InstallType {
    /// macOS Disk Image — mount with `hdiutil`, copy bundle, detach.
    Dmg,
    /// macOS Installer Package — run with `installer -pkg`.
    Pkg,
    /// ZIP archive — extract and locate bundle.
    Zip,
}

impl std::fmt::Display for InstallType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Dmg => write!(f, "DMG"),
            Self::Pkg => write!(f, "PKG"),
            Self::Zip => write!(f, "ZIP"),
        }
    }
}

// ── DownloadType ──────────────────────────────────────────────────────────────

/// Whether apm can download this plugin automatically or the user must do it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum DownloadType {
    /// apm can download this plugin automatically (default).
    Direct,
    /// The user must download this plugin manually (e.g., requires signup/login).
    Manual,
}

fn default_download_type() -> DownloadType {
    DownloadType::Direct
}

// ── FormatSource ──────────────────────────────────────────────────────────────

/// Download and verification metadata for a single format of a plugin.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FormatSource {
    /// Direct download URL for the archive containing this format's bundle.
    pub url: String,

    /// Expected SHA256 hex digest of the downloaded archive.
    pub sha256: String,

    /// How the archive should be handled after download.
    pub install_type: InstallType,

    /// Path of the bundle inside the archive (relative, e.g. `"Plugin.vst3"`).
    /// Optional — some archives contain exactly one bundle at the root.
    pub bundle_path: Option<String>,

    /// Whether apm can download this automatically or the user must download
    /// it manually (e.g., requires account signup). Defaults to `direct`.
    #[serde(default = "default_download_type")]
    pub download_type: DownloadType,
}

// ── PluginDefinition ──────────────────────────────────────────────────────────

/// A plugin definition as stored in the registry.
///
/// One TOML file per plugin in the registry Git repository, at
/// `plugins/<slug>.toml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginDefinition {
    /// Unique, URL-safe slug used as the registry identifier (e.g. `"tal-noisemaker"`).
    pub slug: String,

    /// Human-readable display name (e.g. `"TAL-NoiseMaker"`).
    pub name: String,

    /// Plugin vendor / developer name (e.g. `"TAL Software"`).
    pub vendor: String,

    /// Current version string (semver or vendor-defined).
    pub version: String,

    /// Short description shown in search results.
    pub description: String,

    /// Primary category (e.g. `"instrument"`, `"effect"`).
    pub category: String,

    /// Optional finer-grained sub-category (e.g. `"reverb"`, `"synth"`).
    pub subcategory: Option<String>,

    /// License identifier (SPDX, e.g. `"MIT"`, `"GPL-2.0"`, `"Freeware"`).
    pub license: String,

    /// Free-form tags for search (e.g. `["synth", "virtual-analog", "free"]`).
    #[serde(default)]
    pub tags: Vec<String>,

    /// Available plugin formats and their download sources.
    pub formats: std::collections::HashMap<PluginFormat, FormatSource>,

    /// Optional homepage or product page URL.
    pub homepage: Option<String>,
}

// ── Source ────────────────────────────────────────────────────────────────────

/// A configured registry source (mirrors `apt`'s `sources.list` entry).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Source {
    /// Short identifier used in CLI output and state tracking.
    pub name: String,

    /// Git repository URL (for remote sources) or filesystem path (for local
    /// testing sources).
    pub url: String,

    /// Whether this is the built-in default registry.
    #[serde(default)]
    pub is_default: bool,
}

impl Source {
    /// Create the default official registry source.
    pub fn official(url: impl Into<String>) -> Self {
        Self {
            name: "official".to_string(),
            url: url.into(),
            is_default: true,
        }
    }
}

// ── RegistryIndex ─────────────────────────────────────────────────────────────

/// Root manifest of a registry (`index.toml`), listing available plugins.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryIndex {
    /// Schema version for forward-compatibility.
    pub version: u32,

    /// ISO 8601 timestamp when the index was generated.
    pub generated: String,

    /// Lightweight entries referencing individual plugin TOML files.
    #[serde(default)]
    pub plugins: Vec<RegistryIndexEntry>,
}

/// A single row in `index.toml`'s `[[plugins]]` array.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryIndexEntry {
    /// Plugin slug (matches `PluginDefinition::slug`).
    pub name: String,

    /// Relative path to the plugin's TOML file inside the registry repo.
    pub path: String,

    /// Current version (duplicated here for fast version checks without
    /// reading every plugin TOML).
    pub version: String,
}
