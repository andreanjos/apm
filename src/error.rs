// ApmError variants are the full error surface for all phases of apm.

use std::path::PathBuf;
use thiserror::Error;

/// Top-level error type for apm.
///
/// Each variant carries enough context for a human-readable message with a
/// remediation hint. Use [`anyhow::Context`] in command handlers to add
/// higher-level context (e.g. "while installing tal-noisemaker").
#[derive(Error, Debug)]
#[allow(dead_code)]
pub enum ApmError {
    // ── Configuration ────────────────────────────────────────────────────────
    #[error(
        "Failed to load configuration from {path}: {source}\n\
         Hint: Delete {path} and let apm recreate it with defaults, or fix the TOML syntax."
    )]
    Config {
        path: PathBuf,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    // ── Registry ─────────────────────────────────────────────────────────────
    #[error(
        "Failed to parse registry file {path}: {source}\n\
         Hint: The registry file has invalid TOML syntax. Check line {line} for errors."
    )]
    RegistryParse {
        path: PathBuf,
        line: u32,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    #[error(
        "Plugin '{name}' not found in any configured registry.\n\
         Hint: Run `apm sync` to update your local registry cache, then try `apm search {name}` \
         to find the correct plugin name."
    )]
    PluginNotFound { name: String },

    #[error(
        "Registry sync failed for source '{source_name}': {reason}\n\
         Hint: Check your network connection and verify the registry URL with `apm sources list`."
    )]
    RegistrySync {
        source_name: String,
        reason: String,
    },

    // ── Download ─────────────────────────────────────────────────────────────
    #[error(
        "Download failed for {url}: {reason}\n\
         Hint: Check your network connection and try again. If the error persists, the registry \
         entry for this plugin may have a stale URL."
    )]
    Download { url: String, reason: String },

    // ── Checksum ─────────────────────────────────────────────────────────────
    #[error(
        "SHA256 checksum mismatch for downloaded file.\n  Expected: {expected}\n  Got:      {actual}\n\
         Hint: The downloaded file may be corrupt or tampered with. The download has been \
         deleted. Run `apm install` again to retry."
    )]
    Checksum { expected: String, actual: String },

    // ── Installation ─────────────────────────────────────────────────────────
    #[error(
        "Installation failed for '{plugin}': {reason}\n\
         Hint: {hint}"
    )]
    Install {
        plugin: String,
        reason: String,
        hint: String,
    },

    #[error(
        "Permission denied writing to {path}.\n\
         Hint: apm installs plugins to your user library by default (~~/Library/Audio/Plug-Ins/). \
         If you need a system-wide install, re-run with `sudo apm install --system`."
    )]
    Permission { path: PathBuf },

    // ── Scanner ───────────────────────────────────────────────────────────────
    #[error(
        "Failed to read plugin bundle at {path}: {reason}\n\
         Hint: The bundle may be corrupt or incomplete. Try reinstalling the plugin."
    )]
    Scanner { path: PathBuf, reason: String },

    #[error(
        "Failed to parse Info.plist at {path}: {reason}\n\
         Hint: The plist may be in an unsupported format. This plugin will be skipped."
    )]
    PlistParse { path: PathBuf, reason: String },

    // ── Parse ─────────────────────────────────────────────────────────────────
    #[error(
        "TOML parse error in {path} at line {line}: {reason}\n\
         Hint: Fix the TOML syntax error. Use a TOML validator if needed."
    )]
    TomlParse {
        path: PathBuf,
        line: u32,
        reason: String,
    },

    // ── Network ───────────────────────────────────────────────────────────────
    #[error(
        "Network error: {reason}\n\
         Hint: Check your internet connection and proxy settings."
    )]
    Network { reason: String },

    // ── I/O ───────────────────────────────────────────────────────────────────
    #[error("I/O error{}: {source}", .context.as_deref().map(|c| format!(" ({})", c)).unwrap_or_default())]
    Io {
        #[source]
        source: std::io::Error,
        context: Option<String>,
    },

}


impl From<std::io::Error> for ApmError {
    fn from(e: std::io::Error) -> Self {
        Self::Io { source: e, context: None }
    }
}

impl From<reqwest::Error> for ApmError {
    fn from(e: reqwest::Error) -> Self {
        let reason = e.to_string();
        let url = e.url().map(|u| u.to_string()).unwrap_or_else(|| "<unknown>".to_string());
        if e.is_connect() || e.is_timeout() {
            Self::Network { reason }
        } else {
            Self::Download { url, reason }
        }
    }
}
