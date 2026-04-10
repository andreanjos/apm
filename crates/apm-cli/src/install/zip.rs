// ZIP installer — extracts a ZIP archive to a temp directory, locates the
// plugin bundle, copies it to the destination directory, and cleans up.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use tracing::{debug, info};
use walkdir::WalkDir;

use apm_core::error::ApmError;
use apm_core::registry::PluginFormat;

use super::pkg;

/// Extract `zip_path`, find the `.component` or `.vst3` bundle for `format`,
/// copy it to `dest_dir`, and return the path to the installed bundle.
pub fn install_from_zip(
    zip_path: &Path,
    dest_dir: &Path,
    format: PluginFormat,
    expected_bundle_path: Option<&str>,
) -> Result<PathBuf> {
    info!("Extracting ZIP: {}", zip_path.display());

    let tmp_dir = tempfile::Builder::new()
        .prefix("apm-zip-")
        .tempdir()
        .context("Cannot create temp directory for ZIP extraction")?;

    extract_zip(zip_path, tmp_dir.path())?;

    let extension = bundle_extension(format);
    debug!(
        "Searching {} for .{} bundle",
        tmp_dir.path().display(),
        extension
    );

    let bundles: Vec<PathBuf> = WalkDir::new(tmp_dir.path())
        .min_depth(1)
        .max_depth(6)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_type().is_dir()
                && e.path().extension().and_then(|x| x.to_str()) == Some(extension)
        })
        .map(|e| e.into_path())
        .collect();

    if bundles.is_empty() {
        if let Some(pkg_path) = pkg::find_pkg_in_dir(tmp_dir.path()) {
            let installed = pkg::install_from_pkg(&pkg_path).with_context(|| {
                format!(
                    "ZIP archive contained a PKG installer at {}",
                    pkg_path.display()
                )
            })?;

            return pkg::select_installed_bundle(installed, format, expected_bundle_path).map_err(
                |error| {
                    ApmError::Install {
                        plugin: zip_path.display().to_string(),
                        reason: error.to_string(),
                        hint:
                            "Check ~/Library/Audio/Plug-Ins/ and /Library/Audio/Plug-Ins/ manually."
                                .to_owned(),
                    }
                    .into()
                },
            );
        }

        return Err(ApmError::Install {
            plugin: zip_path.display().to_string(),
            reason: format!("No .{extension} bundle found inside the ZIP archive"),
            hint: format!(
                "The ZIP may package the {format} plugin under a different path. \
                 Check the registry entry's bundle_path field."
            ),
        }
        .into());
    }

    // Use the first match (sorted for determinism).
    let mut sorted = bundles;
    sorted.sort();
    let bundle_src = &sorted[0];

    if sorted.len() > 1 {
        debug!(
            "Multiple .{} bundles found; using {}",
            extension,
            bundle_src.display()
        );
    }

    copy_bundle(bundle_src, dest_dir)
    // tmp_dir is dropped here — automatically cleaned up.
}

// ── Extraction ────────────────────────────────────────────────────────────────

fn extract_zip(zip_path: &Path, dest: &Path) -> Result<()> {
    use std::io::Read;

    let file = std::fs::File::open(zip_path)
        .with_context(|| format!("Cannot open ZIP file: {}", zip_path.display()))?;

    let mut archive = zip::ZipArchive::new(file)
        .with_context(|| format!("Cannot read ZIP archive: {}", zip_path.display()))?;

    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .with_context(|| format!("Cannot read ZIP entry {i}"))?;

        let entry_path = match entry.enclosed_name() {
            Some(p) => p.to_owned(),
            None => {
                debug!("Skipping unsafe ZIP entry name at index {i}");
                continue;
            }
        };

        let out_path = dest.join(&entry_path);

        if entry.is_dir() {
            std::fs::create_dir_all(&out_path)
                .with_context(|| format!("Cannot create directory: {}", out_path.display()))?;
        } else {
            if let Some(parent) = out_path.parent() {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("Cannot create directory: {}", parent.display()))?;
            }

            let mut out_file = std::fs::File::create(&out_path)
                .with_context(|| format!("Cannot create file: {}", out_path.display()))?;

            let mut buf = Vec::new();
            entry
                .read_to_end(&mut buf)
                .with_context(|| format!("Cannot read ZIP entry: {}", entry_path.display()))?;

            std::io::Write::write_all(&mut out_file, &buf)
                .with_context(|| format!("Cannot write file: {}", out_path.display()))?;

            // Restore Unix permissions if available.
            #[cfg(unix)]
            if let Some(mode) = entry.unix_mode() {
                use std::os::unix::fs::PermissionsExt;
                let perms = std::fs::Permissions::from_mode(mode);
                let _ = std::fs::set_permissions(&out_path, perms);
            }
        }
    }

    debug!("ZIP extracted to {}", dest.display());
    Ok(())
}

// ── Bundle copy ───────────────────────────────────────────────────────────────

fn copy_bundle(bundle_src: &Path, dest_dir: &Path) -> Result<PathBuf> {
    apm_core::config::ensure_dir(dest_dir)
        .with_context(|| format!("Cannot create plugin directory: {}", dest_dir.display()))?;

    let bundle_name = bundle_src
        .file_name()
        .context("Bundle path has no file name")?;

    let dest_bundle = dest_dir.join(bundle_name);

    // Remove any existing bundle (upgrade path).
    if dest_bundle.exists() {
        std::fs::remove_dir_all(&dest_bundle)
            .with_context(|| format!("Cannot remove existing bundle: {}", dest_bundle.display()))?;
    }

    debug!(
        "Copying bundle {} → {}",
        bundle_src.display(),
        dest_bundle.display()
    );

    let status = std::process::Command::new("cp")
        .args(["-R"])
        .arg(bundle_src)
        .arg(dest_dir)
        .status()
        .context("Cannot run cp")?;

    if !status.success() {
        return Err(ApmError::Install {
            plugin: bundle_src.display().to_string(),
            reason: format!("cp -R exited {status}"),
            hint: format!(
                "Check that {} is writable. If installing to /Library, re-run with sudo.",
                dest_dir.display()
            ),
        }
        .into());
    }

    info!("Installed bundle: {}", dest_bundle.display());
    Ok(dest_bundle)
}

fn bundle_extension(format: PluginFormat) -> &'static str {
    match format {
        PluginFormat::Au => "component",
        PluginFormat::Vst3 => "vst3",
        PluginFormat::App => "app",
    }
}
