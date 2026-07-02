use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use tracing::{debug, info, warn};

use crate::error::ApmError;
use crate::registry::PluginFormat;

/// Mount `dmg_path`, find a bundle for `format`, copy it into `dest_dir`, and
/// return the installed bundle path.
///
/// Shared engine execution deliberately does not run embedded PKG installers.
/// PKG installers can execute privileged scripts, so callers that want that
/// legacy behavior must opt in through `install_from_dmg_with_pkg_handler`.
pub fn install_from_dmg(
    dmg_path: &Path,
    dest_dir: &Path,
    format: PluginFormat,
    expected_bundle_path: Option<&str>,
) -> Result<PathBuf> {
    install_from_dmg_with_pkg_handler(
        dmg_path,
        dest_dir,
        format,
        expected_bundle_path,
        |pkg_path| {
            Err(ApmError::Install {
                plugin: dmg_path.display().to_string(),
                reason: format!(
                    "DMG contains a PKG installer at {}, but shared DMG execution does not run PKG installers yet",
                    pkg_path.display()
                ),
                hint: "Use the existing CLI/vendor installer path for this package until privileged PKG execution is added to the shared engine.".to_string(),
            }
            .into())
        },
    )
}

/// Variant used by the CLI to preserve its legacy DMG-with-PKG fallback while
/// keeping DMG mounting, bundle selection, and bundle copy canonical in core.
pub fn install_from_dmg_with_pkg_handler(
    dmg_path: &Path,
    dest_dir: &Path,
    format: PluginFormat,
    expected_bundle_path: Option<&str>,
    pkg_handler: impl FnOnce(&Path) -> Result<PathBuf>,
) -> Result<PathBuf> {
    info!("Mounting DMG: {}", dmg_path.display());

    let mountpoint = mount_dmg(dmg_path)?;
    let mut guard = DmgGuard::new(mountpoint);
    let result = install_from_mounted_dir(
        &guard.mountpoint,
        dest_dir,
        format,
        expected_bundle_path,
        pkg_handler,
    );

    if let Err(error) = guard.detach() {
        warn!("DMG detach warning: {error}");
    }

    result
}

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
            Ok(status) if status.success() => {
                self.detached = true;
                let _ = std::fs::remove_dir_all(&self.mountpoint);
                debug!("DMG detached: {}", self.mountpoint.display());
                Ok(())
            }
            Ok(status) => {
                let force = std::process::Command::new("hdiutil")
                    .args(["detach", "-force"])
                    .arg(&self.mountpoint)
                    .status();
                if force.map(|status| status.success()).unwrap_or(false) {
                    self.detached = true;
                    let _ = std::fs::remove_dir_all(&self.mountpoint);
                    warn!(
                        "DMG force-detached (normal detach exited {}): {}",
                        status,
                        self.mountpoint.display()
                    );
                    Ok(())
                } else {
                    Err(ApmError::Install {
                        plugin: self.mountpoint.display().to_string(),
                        reason: format!("hdiutil detach exited {status}"),
                        hint: format!(
                            "Run `hdiutil detach {}` manually to unmount the volume.",
                            self.mountpoint.display()
                        ),
                    }
                    .into())
                }
            }
            Err(error) => Err(ApmError::Install {
                plugin: self.mountpoint.display().to_string(),
                reason: format!("Cannot run hdiutil detach: {error}"),
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
            let _ = std::process::Command::new("hdiutil")
                .args(["detach", "-quiet", "-force"])
                .arg(&self.mountpoint)
                .status();
            let _ = std::fs::remove_dir_all(&self.mountpoint);
        }
    }
}

fn mount_dmg(dmg_path: &Path) -> Result<PathBuf> {
    let temp_dir = tempfile::Builder::new()
        .prefix("apm-dmg-")
        .tempdir()
        .context("Cannot create temp directory for DMG mountpoint")?;
    let mountpoint = temp_dir.into_path();

    debug!(
        "Attaching DMG {} at {}",
        dmg_path.display(),
        mountpoint.display()
    );

    let output = std::process::Command::new("hdiutil")
        .args([
            "attach",
            "-nobrowse",
            "-noverify",
            "-noautoopen",
            "-quiet",
            "-mountpoint",
        ])
        .arg(&mountpoint)
        .arg(dmg_path)
        .output()
        .context("Cannot run hdiutil - is macOS installed?")?;

    if output.status.success() {
        debug!("DMG mounted at {}", mountpoint.display());
        return Ok(mountpoint);
    }

    let _ = std::fs::remove_dir_all(&mountpoint);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let hint = if stderr.contains("license") || stderr.contains("agree") {
        "This DMG contains a license agreement that blocks headless mounting. Mount it manually with Finder, accept the license, then retry the install.".to_string()
    } else if stderr.contains("Permission denied") || stderr.contains("not permitted") {
        "Permission denied mounting the DMG. Try running as your normal user, not root.".to_string()
    } else {
        format!("hdiutil stderr: {}", stderr.trim())
    };

    Err(ApmError::Install {
        plugin: dmg_path.display().to_string(),
        reason: format!("hdiutil attach exited {}", output.status),
        hint,
    }
    .into())
}

fn install_from_mounted_dir(
    mountpoint: &Path,
    dest_dir: &Path,
    format: PluginFormat,
    expected_bundle_path: Option<&str>,
    pkg_handler: impl FnOnce(&Path) -> Result<PathBuf>,
) -> Result<PathBuf> {
    let extension = super::bundle_extension(format);
    debug!(
        "Searching {} for .{} bundle",
        mountpoint.display(),
        extension
    );

    let bundles = super::bundle::find_bundles(mountpoint, extension, 4);
    if bundles.is_empty() {
        return no_bundle_error(mountpoint, extension, format, pkg_handler);
    }

    let bundle_src = super::bundle::select_bundle(mountpoint, bundles, expected_bundle_path)?;
    super::bundle::copy_bundle(&bundle_src, dest_dir)
}

fn no_bundle_error(
    mountpoint: &Path,
    extension: &str,
    format: PluginFormat,
    pkg_handler: impl FnOnce(&Path) -> Result<PathBuf>,
) -> Result<PathBuf> {
    if let Some(pkg_path) = super::bundle::find_pkg_in_dir(mountpoint) {
        return pkg_handler(&pkg_path);
    }

    Err(ApmError::Install {
        plugin: mountpoint.display().to_string(),
        reason: format!("No .{extension} bundle found inside the DMG"),
        hint: format!(
            "The DMG may package the {format} plugin under a different path. Check the registry entry's bundle_path field."
        ),
    }
    .into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_from_mounted_dir_prefers_expected_bundle_path() {
        let temp = tempfile::tempdir().expect("temp dir");
        let mount = temp.path().join("mount");
        let dest = temp.path().join("dest");
        std::fs::create_dir_all(mount.join("Other.vst3/Contents")).expect("other bundle");
        std::fs::write(mount.join("Other.vst3/Contents/Info.plist"), "other").expect("write other");
        std::fs::create_dir_all(mount.join("Nested/Preferred.vst3/Contents"))
            .expect("preferred bundle");
        std::fs::write(
            mount.join("Nested/Preferred.vst3/Contents/Info.plist"),
            "preferred",
        )
        .expect("write preferred");

        let installed = install_from_mounted_dir(
            &mount,
            &dest,
            PluginFormat::Vst3,
            Some("Nested/Preferred.vst3"),
            |_| unreachable!("pkg handler should not run"),
        )
        .expect("install from mounted dir");

        assert_eq!(installed, dest.join("Preferred.vst3"));
        assert_eq!(
            std::fs::read_to_string(installed.join("Contents/Info.plist")).expect("read copied"),
            "preferred"
        );
        assert!(!dest.join("Other.vst3").exists());
    }

    #[test]
    fn install_from_mounted_dir_reports_embedded_pkg_as_unsupported_by_default() {
        let temp = tempfile::tempdir().expect("temp dir");
        let mount = temp.path().join("mount");
        std::fs::create_dir_all(mount.join("Installer.pkg/Contents")).expect("pkg");

        let error = install_from_mounted_dir(
            &mount,
            temp.path(),
            PluginFormat::Vst3,
            None,
            |pkg_path| {
                Err(ApmError::Install {
                    plugin: mount.display().to_string(),
                    reason: format!(
                        "DMG contains a PKG installer at {}, but shared DMG execution does not run PKG installers yet",
                        pkg_path.display()
                    ),
                    hint: "Use the existing CLI/vendor installer path for this package until privileged PKG execution is added to the shared engine.".to_string(),
                }
                .into())
            },
        )
        .expect_err("pkg-only dmg should not install");

        assert!(error
            .to_string()
            .contains("does not run PKG installers yet"));
    }
}
