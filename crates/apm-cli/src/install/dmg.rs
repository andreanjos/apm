// DMG installer — mounts a disk image with hdiutil, locates plugin bundles,
// copies them to the destination directory, then unmounts. A DmgGuard RAII
// type ensures the image is always detached even on panic or early return.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use tracing::{debug, info, warn};
use walkdir::WalkDir;

use apm_core::error::ApmError;
use apm_core::registry::PluginFormat;

use super::pkg;

// ── DmgGuard ─────────────────────────────────────────────────────────────────

/// RAII guard that unmounts a DMG volume on drop, ensuring cleanup even on
/// panic or early return.
struct DmgGuard {
    mountpoint: PathBuf,
    detached: bool,
}

impl DmgGuard {
    fn new(mountpoint: PathBuf) -> Self {
        Self {
            mountpoint,
            detached: false,
        }
    }

    /// Explicitly detach — returns an error if it fails. Called by the caller
    /// after work is done; Drop calls it silently as a last resort.
    fn detach(&mut self) -> Result<()> {
        if self.detached {
            return Ok(());
        }

        debug!("Detaching DMG at {}", self.mountpoint.display());
        let status = std::process::Command::new("hdiutil")
            .args(["detach", "-quiet"])
            .arg(&self.mountpoint)
            .status();

        match status {
            Ok(s) if s.success() => {
                self.detached = true;
                debug!("DMG detached: {}", self.mountpoint.display());
                Ok(())
            }
            Ok(s) => {
                // Try force-detach as a fallback.
                let force = std::process::Command::new("hdiutil")
                    .args(["detach", "-force"])
                    .arg(&self.mountpoint)
                    .status();
                if force.map(|f| f.success()).unwrap_or(false) {
                    self.detached = true;
                    warn!(
                        "DMG force-detached (normal detach exited {}): {}",
                        s,
                        self.mountpoint.display()
                    );
                    Ok(())
                } else {
                    Err(ApmError::Install {
                        plugin: self.mountpoint.display().to_string(),
                        reason: format!("hdiutil detach exited {s}"),
                        hint: format!(
                            "Run `hdiutil detach {}` manually to unmount the volume.",
                            self.mountpoint.display()
                        ),
                    }
                    .into())
                }
            }
            Err(e) => Err(ApmError::Install {
                plugin: self.mountpoint.display().to_string(),
                reason: format!("Cannot run hdiutil detach: {e}"),
                hint: format!(
                    "Run `hdiutil detach {}` manually to unmount the volume.",
                    self.mountpoint.display()
                ),
            }
            .into()),
        }
    }
}

impl Drop for DmgGuard {
    fn drop(&mut self) {
        if !self.detached {
            // Best-effort silent cleanup.
            let _ = std::process::Command::new("hdiutil")
                .args(["detach", "-quiet", "-force"])
                .arg(&self.mountpoint)
                .status();
        }
    }
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Mount `dmg_path`, find the bundle for `format`, copy it to `dest_dir`,
/// unmount, and return the path to the installed bundle.
///
/// If the DMG contains a `.pkg` file instead of a bare bundle, this function
/// will run the embedded PKG installer and select the matching installed bundle.
pub fn install_from_dmg(
    dmg_path: &Path,
    dest_dir: &Path,
    format: PluginFormat,
    expected_bundle_path: Option<&str>,
) -> Result<PathBuf> {
    info!("Mounting DMG: {}", dmg_path.display());

    let mountpoint = mount_dmg(dmg_path)?;
    let mut guard = DmgGuard::new(mountpoint.clone());

    let result = find_and_copy_bundle(&mountpoint, dest_dir, format, expected_bundle_path);

    // Always detach — surface detach errors only if install succeeded.
    match guard.detach() {
        Ok(()) => {}
        Err(e) => {
            warn!("DMG detach warning: {e}");
        }
    }

    result
}

// ── Mount ─────────────────────────────────────────────────────────────────────

/// Attach the DMG and return the mountpoint path.
///
/// Uses a dedicated temp mountpoint so we know exactly where it is mounted,
/// regardless of the volume name embedded in the image.
fn mount_dmg(dmg_path: &Path) -> Result<PathBuf> {
    // Create a temp directory to use as the mountpoint.
    let tmp_dir = tempfile::Builder::new()
        .prefix("apm-dmg-")
        .tempdir()
        .context("Cannot create temp directory for DMG mountpoint")?;

    // We need to keep the path but let the tempdir be dropped (the mounted
    // volume will hold the path open; we just need the string).
    let mountpoint = tmp_dir.into_path();

    debug!(
        "Attaching DMG {} at {}",
        dmg_path.display(),
        mountpoint.display()
    );

    let output = std::process::Command::new("hdiutil")
        .args([
            "attach",
            "-nobrowse",   // Don't open in Finder
            "-noverify",   // Skip image verification (we already verified SHA256)
            "-noautoopen", // Don't auto-open any packages
            "-quiet",      // Suppress progress output
            "-mountpoint",
        ])
        .arg(&mountpoint)
        .arg(dmg_path)
        .output()
        .with_context(|| "Cannot run hdiutil — is macOS installed?")?;

    if !output.status.success() {
        // Clean up the temp mountpoint directory we created.
        let _ = std::fs::remove_dir_all(&mountpoint);

        let stderr = String::from_utf8_lossy(&output.stderr);
        let hint = if stderr.contains("license") || stderr.contains("agree") {
            "This DMG contains a license agreement that blocks headless mounting. \
             Mount it manually with Finder, accept the license, then re-run apm install."
                .to_owned()
        } else if stderr.contains("Permission denied") || stderr.contains("not permitted") {
            "Permission denied mounting the DMG. Try running as your normal user (not root)."
                .to_owned()
        } else {
            format!("hdiutil stderr: {}", stderr.trim())
        };

        return Err(ApmError::Install {
            plugin: dmg_path.display().to_string(),
            reason: format!("hdiutil attach exited {}", output.status),
            hint,
        }
        .into());
    }

    debug!("DMG mounted at {}", mountpoint.display());
    Ok(mountpoint)
}

// ── Bundle discovery and copy ─────────────────────────────────────────────────

fn find_and_copy_bundle(
    mountpoint: &Path,
    dest_dir: &Path,
    format: PluginFormat,
    expected_bundle_path: Option<&str>,
) -> Result<PathBuf> {
    let extension = bundle_extension(format);

    debug!(
        "Searching {} for .{} bundle",
        mountpoint.display(),
        extension
    );

    // Walk up to 4 levels deep — some DMGs nest bundles in subdirectories.
    let bundles: Vec<PathBuf> = WalkDir::new(mountpoint)
        .min_depth(1)
        .max_depth(4)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_type().is_dir()
                && e.path().extension().and_then(|x| x.to_str()) == Some(extension)
        })
        .map(|e| e.into_path())
        .collect();

    if bundles.is_empty() {
        // Check whether the DMG contains a PKG instead.
        if let Some(pkg_path) = pkg::find_pkg_in_dir(mountpoint) {
            let installed = pkg::install_from_pkg(&pkg_path).with_context(|| {
                format!("DMG contained a PKG installer at {}", pkg_path.display())
            })?;

            return pkg::select_installed_bundle(installed, format, expected_bundle_path).map_err(
                |error| {
                    ApmError::Install {
                        plugin: mountpoint.display().to_string(),
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
            plugin: mountpoint.display().to_string(),
            reason: format!("No .{extension} bundle found inside the DMG"),
            hint: format!(
                "The DMG may package the {format} plugin under a different path. \
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
}

/// Copy a plugin bundle directory to `dest_dir` using `cp -R`, which preserves
/// the bundle's internal structure and resource forks.
fn copy_bundle(bundle_src: &Path, dest_dir: &Path) -> Result<PathBuf> {
    apm_core::config::ensure_dir(dest_dir)
        .with_context(|| format!("Cannot create plugin directory: {}", dest_dir.display()))?;

    let bundle_name = bundle_src
        .file_name()
        .context("Bundle path has no file name")?;

    let dest_bundle = dest_dir.join(bundle_name);

    // Remove existing bundle if present (upgrade scenario).
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
