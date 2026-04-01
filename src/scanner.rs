// Plugin scanner — walks macOS AU and VST3 plugin directories and parses
// metadata from each bundle's Info.plist. No network access required.
//
// Fields on ScannedPlugin that are not yet consumed by command handlers
// (bundle_id, scope) will be used in later phases; dead_code is suppressed
// to avoid noise, consistent with the rest of the Phase 1 infrastructure.
#![allow(dead_code)]

use std::fmt;
use std::path::{Path, PathBuf};

use plist::Value;
use tracing::warn;
use walkdir::WalkDir;

use crate::config::{self, Config};

// ── Plugin Format ─────────────────────────────────────────────────────────────

/// The binary format of a discovered plugin bundle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginFormat {
    Au,
    Vst3,
}

impl fmt::Display for PluginFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Au => write!(f, "AU"),
            Self::Vst3 => write!(f, "VST3"),
        }
    }
}

// ── Install Scope ─────────────────────────────────────────────────────────────

/// Where the plugin bundle lives — system-wide or per-user.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallScope {
    System,
    User,
}

impl fmt::Display for InstallScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::System => write!(f, "System"),
            Self::User => write!(f, "User"),
        }
    }
}

// ── ScannedPlugin ─────────────────────────────────────────────────────────────

/// A plugin bundle discovered on the file system with metadata read from
/// its `Contents/Info.plist`.
#[derive(Debug, Clone)]
pub struct ScannedPlugin {
    /// Display name from `CFBundleName` (falls back to bundle file stem).
    pub name: String,

    /// Version string from `CFBundleShortVersionString` then `CFBundleVersion`.
    pub version: String,

    /// Vendor name. For AU: parsed from `AudioComponents[0].name` ("Vendor: Plugin").
    /// For VST3 or when the AU field is absent: empty string.
    pub vendor: String,

    /// Reverse-domain bundle identifier from `CFBundleIdentifier`.
    pub bundle_id: String,

    /// Plugin format derived from the bundle extension.
    pub format: PluginFormat,

    /// Whether the bundle is in a system or user plugin directory.
    pub scope: InstallScope,

    /// Absolute path to the bundle directory (e.g. `.component` or `.vst3`).
    pub path: PathBuf,
}

// ── Directory descriptors ─────────────────────────────────────────────────────

struct ScanDir {
    path: PathBuf,
    format: PluginFormat,
    scope: InstallScope,
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Scan all four macOS plugin directories and return every successfully parsed
/// plugin, sorted by name (case-insensitive). Bundles whose `Info.plist` is
/// missing or unparseable produce a `tracing::warn!` and are skipped.
pub fn scan_plugins(_config: &Config) -> Vec<ScannedPlugin> {
    let dirs = [
        ScanDir {
            path: config::system_au_dir(),
            format: PluginFormat::Au,
            scope: InstallScope::System,
        },
        ScanDir {
            path: config::system_vst3_dir(),
            format: PluginFormat::Vst3,
            scope: InstallScope::System,
        },
        ScanDir {
            path: config::user_au_dir(),
            format: PluginFormat::Au,
            scope: InstallScope::User,
        },
        ScanDir {
            path: config::user_vst3_dir(),
            format: PluginFormat::Vst3,
            scope: InstallScope::User,
        },
    ];

    let mut plugins: Vec<ScannedPlugin> = Vec::new();

    for dir in &dirs {
        scan_dir(&dir.path, dir.format, dir.scope, &mut plugins);
    }

    // Sort by name, case-insensitive, then by path for a stable order.
    plugins.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then_with(|| a.path.cmp(&b.path))
    });

    plugins
}

// ── Internal helpers ──────────────────────────────────────────────────────────

/// Walk `dir` one level deep looking for bundles with the expected extension
/// (`.component` for AU, `.vst3` for VST3). For each bundle found, attempt to
/// read and parse `Contents/Info.plist`; on failure emit a warning and skip.
fn scan_dir(dir: &Path, format: PluginFormat, scope: InstallScope, out: &mut Vec<ScannedPlugin>) {
    let extension = match format {
        PluginFormat::Au => "component",
        PluginFormat::Vst3 => "vst3",
    };

    if !dir.exists() {
        // Silently skip directories that do not exist on this machine —
        // system VST3 directory is absent on some installs, for example.
        return;
    }

    // max_depth(1): we only want immediate children of the plugin directory.
    // min_depth(1): skip the directory itself.
    for entry in WalkDir::new(dir)
        .min_depth(1)
        .max_depth(1)
        .into_iter()
        .filter_map(|e| match e {
            Ok(e) => Some(e),
            Err(err) => {
                warn!("Cannot read directory entry: {err}");
                None
            }
        })
    {
        let path = entry.path();

        // Bundles are directories with the right extension.
        if !path.is_dir() {
            continue;
        }

        match path.extension().and_then(|e| e.to_str()) {
            Some(ext) if ext == extension => {}
            _ => continue,
        }

        match parse_bundle(path, format, scope) {
            Some(plugin) => out.push(plugin),
            None => {
                // Warning already emitted inside parse_bundle.
            }
        }
    }
}

/// Attempt to read and parse `<bundle>/Contents/Info.plist`, returning a
/// `ScannedPlugin` on success or `None` (with a warning) on failure.
fn parse_bundle(bundle: &Path, format: PluginFormat, scope: InstallScope) -> Option<ScannedPlugin> {
    let plist_path = bundle.join("Contents/Info.plist");

    if !plist_path.exists() {
        warn!(
            "Missing Info.plist for bundle '{}' — skipping.",
            bundle.display()
        );
        return None;
    }

    let value = match Value::from_file(&plist_path) {
        Ok(v) => v,
        Err(err) => {
            warn!(
                "Cannot parse Info.plist at '{}': {err} — skipping.",
                plist_path.display()
            );
            return None;
        }
    };

    let dict = match value.as_dictionary() {
        Some(d) => d,
        None => {
            warn!(
                "Info.plist at '{}' is not a dictionary — skipping.",
                plist_path.display()
            );
            return None;
        }
    };

    // Helper: extract a string value by key.
    let get_str = |key: &str| -> Option<String> {
        dict.get(key)
            .and_then(|v| v.as_string())
            .map(|s| s.to_owned())
    };

    // Bundle name: CFBundleName is preferred; fall back to the file stem.
    let name = get_str("CFBundleName").unwrap_or_else(|| {
        bundle
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Unknown")
            .to_owned()
    });

    // Version: prefer the human-readable short version string.
    let version = get_str("CFBundleShortVersionString")
        .or_else(|| get_str("CFBundleVersion"))
        .unwrap_or_else(|| "Unknown".to_owned());

    let bundle_id = get_str("CFBundleIdentifier").unwrap_or_default();

    // Vendor: for AU plugins, try AudioComponents[0].name which is typically
    // formatted as "Vendor: PluginName". Extract the part before the colon.
    let vendor = if format == PluginFormat::Au {
        extract_au_vendor(dict)
    } else {
        String::new()
    };

    Some(ScannedPlugin {
        name,
        version,
        vendor,
        bundle_id,
        format,
        scope,
        path: bundle.to_path_buf(),
    })
}

/// Try to extract a vendor name from the `AudioComponents` array in an AU
/// Info.plist. The `name` field is conventionally "Vendor: PluginName".
fn extract_au_vendor(dict: &plist::Dictionary) -> String {
    let components = match dict.get("AudioComponents").and_then(|v| v.as_array()) {
        Some(arr) if !arr.is_empty() => arr,
        _ => return String::new(),
    };

    let first = match components[0].as_dictionary() {
        Some(d) => d,
        None => return String::new(),
    };

    let name = match first.get("name").and_then(|v| v.as_string()) {
        Some(s) => s,
        None => return String::new(),
    };

    // "Acme DSP: SuperReverb" → "Acme DSP"
    if let Some(colon_pos) = name.find(':') {
        name[..colon_pos].trim().to_owned()
    } else {
        // No colon — use the whole name as vendor.
        name.trim().to_owned()
    }
}
