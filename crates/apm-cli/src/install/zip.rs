use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use apm_core::error::ApmError;
use apm_core::registry::PluginFormat;

use super::pkg;

/// Extract `zip_path`, find the requested plugin bundle, copy it into
/// `dest_dir`, and return the installed bundle path.
///
/// Core owns the normal ZIP archive path. The CLI only supplies the legacy
/// fallback for archives that contain a `.pkg` instead of a bundle.
pub fn install_from_zip(
    zip_path: &Path,
    dest_dir: &Path,
    format: PluginFormat,
    expected_bundle_path: Option<&str>,
) -> Result<PathBuf> {
    apm_core::install::zip::install_from_zip_with_pkg_handler(
        zip_path,
        dest_dir,
        format,
        expected_bundle_path,
        |pkg_path| {
            let installed = pkg::install_from_pkg(pkg_path).with_context(|| {
                format!(
                    "ZIP archive contained a PKG installer at {}",
                    pkg_path.display()
                )
            })?;

            pkg::select_installed_bundle(installed, format, expected_bundle_path).map_err(|error| {
                ApmError::Install {
                    plugin: zip_path.display().to_string(),
                    reason: error.to_string(),
                    hint: "Check ~/Library/Audio/Plug-Ins/ and /Library/Audio/Plug-Ins/ manually."
                        .to_owned(),
                }
                .into()
            })
        },
    )
}
