pub(crate) mod bundle;
pub mod dmg;
pub mod quarantine;
pub mod zip;

use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use tracing::debug;

use crate::config::{
    self, system_au_dir, system_vst3_dir, user_au_dir, user_vst3_dir, InstallScope,
};
use crate::error::ApmError;
use crate::registry::PluginFormat;

/// Returns true when the sha256 value is an empty/placeholder marker rather
/// than a usable integrity checksum.
pub fn is_placeholder_sha256(sha256: &str) -> bool {
    let value = sha256.trim();
    value.is_empty() || value.eq_ignore_ascii_case("manual") || value.chars().all(|c| c == '0')
}

/// Verify a local archive against an expected SHA256 digest.
pub fn verify_file_sha256(path: &Path, expected_sha256: &str) -> Result<String> {
    let mut file = std::fs::File::open(path)
        .with_context(|| format!("Cannot open file for checksum: {}", path.display()))?;

    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 65536];

    loop {
        let bytes_read = file
            .read(&mut buffer)
            .with_context(|| format!("Read error while hashing: {}", path.display()))?;
        if bytes_read == 0 {
            break;
        }
        hasher.update(&buffer[..bytes_read]);
    }

    let actual_hex = hex::encode(hasher.finalize());
    if expected_sha256.to_lowercase() != actual_hex.to_lowercase() {
        return Err(ApmError::Checksum {
            expected: expected_sha256.to_owned(),
            actual: actual_hex,
        }
        .into());
    }

    debug!("SHA256 OK for local file: {actual_hex}");
    Ok(actual_hex)
}

pub fn plugin_dest_dir(format: PluginFormat, scope: InstallScope) -> PathBuf {
    match (format, scope) {
        (PluginFormat::Au, InstallScope::User) => user_au_dir(),
        (PluginFormat::Au, InstallScope::System) => system_au_dir(),
        (PluginFormat::Vst3, InstallScope::User) => user_vst3_dir(),
        (PluginFormat::Vst3, InstallScope::System) => system_vst3_dir(),
        (PluginFormat::App, InstallScope::User) => dirs::home_dir()
            .map(|home| home.join("Applications"))
            .unwrap_or_else(|| PathBuf::from("/Applications")),
        (PluginFormat::App, InstallScope::System) => PathBuf::from("/Applications"),
    }
}

pub fn bundle_extension(format: PluginFormat) -> &'static str {
    match format {
        PluginFormat::Au => "component",
        PluginFormat::Vst3 => "vst3",
        PluginFormat::App => "app",
    }
}

fn ensure_dir(path: &Path) -> Result<()> {
    config::ensure_dir(path)
        .with_context(|| format!("Cannot create install directory: {}", path.display()))
}
