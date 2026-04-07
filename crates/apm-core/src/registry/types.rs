// Registry types are the full TOML schema for the plugin registry.

use serde::{Deserialize, Serialize};
use std::cmp::Ordering;

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

impl std::fmt::Display for DownloadType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Direct => write!(f, "direct"),
            Self::Manual => write!(f, "manual"),
        }
    }
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

/// A specific published release of a plugin.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginRelease {
    /// Version string for this historical release.
    pub version: String,

    /// Available formats and download metadata for this release.
    pub formats: std::collections::HashMap<PluginFormat, FormatSource>,
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

    /// Historical releases available for explicit install requests.
    ///
    /// The top-level `version` and `formats` remain the canonical latest
    /// release for backwards compatibility with older registry entries and
    /// existing callers.
    #[serde(default)]
    pub releases: Vec<PluginRelease>,

    /// Optional homepage or product page URL.
    pub homepage: Option<String>,

    /// Whether this plugin requires purchase through apm-server.
    #[serde(default)]
    pub is_paid: bool,

    /// Price in minor units (for example, cents) when the plugin is paid.
    #[serde(default)]
    pub price_cents: Option<i64>,

    /// ISO currency code for `price_cents`.
    #[serde(default)]
    pub currency: Option<String>,

    /// Registry source that supplied this definition after cache loading.
    ///
    /// This is runtime metadata, not part of registry TOML authoring.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_name: Option<String>,
}

impl PluginDefinition {
    /// Return the latest release represented by the top-level plugin fields.
    pub fn latest_release(&self) -> PluginRelease {
        PluginRelease {
            version: self.version.clone(),
            formats: self.formats.clone(),
        }
    }

    /// Resolve either the latest release (`None`) or a specific version.
    pub fn resolve_release(&self, requested_version: Option<&str>) -> Option<PluginRelease> {
        match requested_version {
            None => Some(self.latest_release()),
            Some(version) if version == self.version => Some(self.latest_release()),
            Some(version) => self
                .releases
                .iter()
                .find(|release| release.version == version)
                .cloned(),
        }
    }

    /// Return all known versions for this plugin, newest first.
    pub fn available_versions(&self) -> Vec<String> {
        let mut versions = vec![self.version.clone()];

        for release in &self.releases {
            if !versions.iter().any(|version| version == &release.version) {
                versions.push(release.version.clone());
            }
        }

        versions.sort_by(|left, right| compare_versions_desc(left, right));
        versions
    }
}

fn compare_versions_desc(left: &str, right: &str) -> Ordering {
    match (semver::Version::parse(left), semver::Version::parse(right)) {
        (Ok(l), Ok(r)) => r.cmp(&l),
        _ => right.cmp(left),
    }
}

// ── PluginBundle ──────────────────────────────────────────────────────────────

/// A named collection of plugins that can be installed together.
///
/// Bundle definitions live in `<cache_dir>/bundles/*.toml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginBundle {
    /// Unique slug for the bundle (e.g. `"producer-essentials"`).
    pub slug: String,

    /// Human-readable display name.
    pub name: String,

    /// Short description shown in `apm bundles`.
    pub description: String,

    /// List of plugin slugs included in this bundle.
    pub plugins: Vec<String>,
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// Build a minimal `PluginDefinition` for testing, with the latest version
    /// set to `latest_ver` and `releases` populated from the given list.
    fn make_plugin(latest_ver: &str, release_versions: &[&str]) -> PluginDefinition {
        let dummy_format_source = FormatSource {
            url: "https://example.com/plugin.zip".to_string(),
            sha256: "abc123".to_string(),
            install_type: InstallType::Zip,
            bundle_path: None,
            download_type: DownloadType::Direct,
        };
        let mut formats = HashMap::new();
        formats.insert(PluginFormat::Vst3, dummy_format_source.clone());

        let releases: Vec<PluginRelease> = release_versions
            .iter()
            .map(|v| PluginRelease {
                version: v.to_string(),
                formats: formats.clone(),
            })
            .collect();

        PluginDefinition {
            slug: "test-plugin".to_string(),
            name: "Test Plugin".to_string(),
            vendor: "Test Vendor".to_string(),
            version: latest_ver.to_string(),
            description: "A test plugin".to_string(),
            category: "effect".to_string(),
            subcategory: None,
            license: "freeware".to_string(),
            tags: vec![],
            formats,
            releases,
            homepage: None,
            is_paid: false,
            price_cents: None,
            currency: None,
            source_name: None,
        }
    }

    #[test]
    fn test_plugin_available_versions_deduplicates() {
        // latest = "2.0.0", and releases includes "2.0.0" plus "1.0.0"
        let plugin = make_plugin("2.0.0", &["2.0.0", "1.0.0"]);
        let versions = plugin.available_versions();

        // "2.0.0" must appear exactly once even though it is both
        // the top-level version AND present in releases.
        assert_eq!(
            versions.iter().filter(|v| v.as_str() == "2.0.0").count(),
            1,
            "2.0.0 should appear exactly once, got: {versions:?}"
        );
        assert_eq!(versions, vec!["2.0.0", "1.0.0"]);
    }

    #[test]
    fn test_plugin_resolve_release_latest() {
        let plugin = make_plugin("3.0.0", &["2.0.0", "1.0.0"]);
        let release = plugin.resolve_release(None).expect("should resolve latest");
        assert_eq!(release.version, "3.0.0");
    }

    #[test]
    fn test_plugin_resolve_release_specific() {
        let plugin = make_plugin("3.0.0", &["2.0.0", "1.0.0"]);
        let release = plugin
            .resolve_release(Some("1.0.0"))
            .expect("should find 1.0.0 in releases");
        assert_eq!(release.version, "1.0.0");
    }

    #[test]
    fn test_plugin_resolve_release_missing() {
        let plugin = make_plugin("3.0.0", &["2.0.0", "1.0.0"]);
        assert!(
            plugin.resolve_release(Some("9.9.9")).is_none(),
            "non-existent version should return None"
        );
    }
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
