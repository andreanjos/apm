use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use apm_core::error::ApmError;
use apm_core::registry::PluginFormat;

use super::pkg;

/// Install a DMG through the shared core DMG bundle path.
///
/// The CLI opts into the embedded-PKG fallback because it can prompt on stdin
/// before invoking `sudo installer`.
pub fn install_from_dmg(
    dmg_path: &Path,
    dest_dir: &Path,
    format: PluginFormat,
    expected_bundle_path: Option<&str>,
) -> Result<PathBuf> {
    apm_core::install::dmg::install_from_dmg_with_pkg_handler(
        dmg_path,
        dest_dir,
        format,
        expected_bundle_path,
        |pkg_path| {
            let installed = pkg::install_from_pkg(pkg_path).with_context(|| {
                format!("DMG contained a PKG installer at {}", pkg_path.display())
            })?;

            pkg::select_installed_bundle(installed, format, expected_bundle_path).map_err(|error| {
                ApmError::Install {
                    plugin: dmg_path.display().to_string(),
                    reason: error.to_string(),
                    hint: "Check ~/Library/Audio/Plug-Ins/ and /Library/Audio/Plug-Ins/ manually."
                        .to_string(),
                }
                .into()
            })
        },
    )
}
