// PKG installer — runs macOS installer(8) with sudo, then uses pkgutil to
// discover what was installed. Always warns the user about elevated privileges
// before proceeding.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use tracing::{debug, info, warn};
use walkdir::WalkDir;

use apm_core::error::ApmError;
use apm_core::registry::PluginFormat;

/// Install a PKG file using `sudo installer -pkg <path> -target /`.
///
/// Warns the user that administrator access is required, prompts for
/// confirmation, then runs the installer. After success, uses `pkgutil` to
/// enumerate installed files and returns the paths to any `.component` or
/// `.vst3` bundles that were installed.
///
/// Returns an error if the user declines, if sudo fails, or if no bundles
/// can be found after installation.
pub fn install_from_pkg(pkg_path: &Path) -> Result<Vec<PathBuf>> {
    // ── Warn and confirm ──────────────────────────────────────────────────────

    eprintln!(
        "\n  This plugin uses a PKG installer which requires administrator access.\n\
         \n\
           The installer will run:\n\
         \n\
             sudo installer -pkg {path}\n\
         \n\
           PKG installers can run pre/post-install scripts as root.\n\
         \n\
           Continue? [y/N] ",
        path = pkg_path.display()
    );

    let mut input = String::new();
    std::io::stdin()
        .read_line(&mut input)
        .context("Cannot read user input")?;

    if !input.trim().eq_ignore_ascii_case("y") {
        eprintln!(
            "  Installation cancelled.\n\
             \n\
               Hint: You can retry with `sudo apm install <plugin>` if you want to proceed."
        );
        return Err(ApmError::Install {
            plugin: pkg_path.display().to_string(),
            reason: "Installation cancelled by user".to_owned(),
            hint: "Re-run `apm install <plugin>` and confirm the PKG installer prompt.".to_owned(),
        }
        .into());
    }

    // ── Run the PKG installer ─────────────────────────────────────────────────

    info!("Running PKG installer: {}", pkg_path.display());

    let status = std::process::Command::new("sudo")
        .args(["installer", "-pkg"])
        .arg(pkg_path)
        .args(["-target", "/"])
        .status()
        .with_context(|| format!("Cannot run `sudo installer` for {}", pkg_path.display()))?;

    if !status.success() {
        return Err(ApmError::Install {
            plugin: pkg_path.display().to_string(),
            reason: format!("installer exited {status}"),
            hint: "The PKG installer failed. Check that you have administrator access \
                   and try running `sudo apm install <plugin>` directly."
                .to_owned(),
        }
        .into());
    }

    // ── Discover what was installed ───────────────────────────────────────────

    let bundle_id = pkg_bundle_id(pkg_path);
    let installed = find_installed_bundles(pkg_path, bundle_id.as_deref());

    if installed.is_empty() {
        warn!(
            "PKG installer succeeded but no .component or .vst3 bundles could be found via \
             pkgutil. The plugin may have installed to a non-standard location."
        );
    }

    Ok(installed)
}

/// Select the installed bundle that best matches the requested format and
/// optional expected bundle path from the registry.
pub fn select_installed_bundle(
    bundles: Vec<PathBuf>,
    format: PluginFormat,
    expected_bundle_path: Option<&str>,
) -> Result<PathBuf> {
    if bundles.is_empty() {
        anyhow::bail!("PKG installer ran but no plugin bundles were discovered afterward");
    }

    if let Some(expected) = expected_bundle_path.and_then(|path| Path::new(path).file_name()) {
        if let Some(found) = bundles
            .iter()
            .find(|bundle| bundle.file_name() == Some(expected))
        {
            return Ok(found.clone());
        }
    }

    let expected_ext = match format {
        PluginFormat::Au => "component",
        PluginFormat::Vst3 => "vst3",
        PluginFormat::App => "app",
    };

    if let Some(found) = bundles.iter().find(|bundle| {
        bundle
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext == expected_ext)
            .unwrap_or(false)
    }) {
        return Ok(found.clone());
    }

    let mut bundles = bundles;
    bundles.sort();
    Ok(bundles.into_iter().next().expect("non-empty bundles"))
}

/// Find the first `.pkg` or `.mpkg` file under `root`, using deterministic sort
/// order so archive probing behaves predictably.
pub fn find_pkg_in_dir(root: &Path) -> Option<PathBuf> {
    let mut packages: Vec<PathBuf> = WalkDir::new(root)
        .min_depth(1)
        .max_depth(6)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter_map(|entry| {
            let path = entry.into_path();
            let ext = path.extension().and_then(|e| e.to_str())?;
            if ext == "pkg" || ext == "mpkg" {
                Some(path)
            } else {
                None
            }
        })
        .collect();

    packages.sort();
    packages.into_iter().next()
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Try to determine the PKG's bundle ID using `pkgutil --pkg-info`.
fn pkg_bundle_id(pkg_path: &Path) -> Option<String> {
    let output = std::process::Command::new("pkgutil")
        .args(["--pkg-info"])
        .arg(pkg_path)
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        if let Some(rest) = line.strip_prefix("package-id:") {
            let id = rest.trim().to_owned();
            debug!("PKG bundle ID: {id}");
            return Some(id);
        }
    }
    None
}

/// Query `pkgutil --files` to enumerate files installed by this package,
/// then return paths to any `.component` or `.vst3` bundles.
fn find_installed_bundles(pkg_path: &Path, bundle_id: Option<&str>) -> Vec<PathBuf> {
    // Try with the bundle ID first, fall back to scanning common directories.
    if let Some(id) = bundle_id {
        let output = std::process::Command::new("pkgutil")
            .args(["--files", id])
            .output();

        if let Ok(out) = output {
            if out.status.success() {
                let text = String::from_utf8_lossy(&out.stdout);
                let bundles = extract_bundle_paths_from_pkgutil(&text);
                if !bundles.is_empty() {
                    return bundles;
                }
            }
        }
    }

    // Fallback: scan the standard install locations.
    debug!(
        "pkgutil lookup failed for {}; scanning plugin directories",
        pkg_path.display()
    );
    scan_standard_dirs_for_bundles()
}

/// Parse the output of `pkgutil --files <bundle-id>` and return absolute paths
/// to `.component` and `.vst3` bundle directories.
fn extract_bundle_paths_from_pkgutil(files_output: &str) -> Vec<PathBuf> {
    // pkgutil --files outputs relative paths like:
    //   Library/Audio/Plug-Ins/VST3/Plugin.vst3
    // or it may output a root prefix. We look for lines ending in .vst3 or
    // .component and reconstruct absolute paths.

    let mut seen_bundles: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
    let mut result = Vec::new();

    for line in files_output.lines() {
        let line = line.trim();
        if line.ends_with(".vst3") || line.ends_with(".component") {
            // Strip trailing slash if present.
            let clean = line.trim_end_matches('/');
            // Make absolute (pkgutil paths are relative to /).
            let abs = if clean.starts_with('/') {
                PathBuf::from(clean)
            } else {
                PathBuf::from("/").join(clean)
            };

            if abs.exists() && abs.is_dir() && seen_bundles.insert(abs.clone()) {
                result.push(abs);
            }
        }
    }

    result
}

/// Last-resort scan of the four standard plugin directories.
fn scan_standard_dirs_for_bundles() -> Vec<PathBuf> {
    let dirs = [
        dirs::home_dir()
            .unwrap_or_default()
            .join("Library/Audio/Plug-Ins/VST3"),
        dirs::home_dir()
            .unwrap_or_default()
            .join("Library/Audio/Plug-Ins/Components"),
        PathBuf::from("/Library/Audio/Plug-Ins/VST3"),
        PathBuf::from("/Library/Audio/Plug-Ins/Components"),
    ];

    let mut result = Vec::new();

    for dir in &dirs {
        if !dir.exists() {
            continue;
        }
        for entry in WalkDir::new(dir)
            .min_depth(1)
            .max_depth(1)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.into_path();
            if path.is_dir() {
                if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                    if ext == "vst3" || ext == "component" {
                        result.push(path);
                    }
                }
            }
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::{find_pkg_in_dir, select_installed_bundle};
    use apm_core::registry::PluginFormat;
    use std::path::PathBuf;

    #[test]
    fn test_find_pkg_in_dir_finds_nested_pkg() {
        let temp = tempfile::tempdir().expect("tempdir");
        let nested = temp.path().join("Vendor/Installer");
        std::fs::create_dir_all(&nested).expect("create nested dir");
        let pkg = nested.join("Plugin.pkg");
        std::fs::write(&pkg, b"pkg").expect("write pkg");

        let found = find_pkg_in_dir(temp.path()).expect("expected pkg");
        assert_eq!(found, pkg);
    }

    #[test]
    fn test_find_pkg_in_dir_returns_none_without_pkg() {
        let temp = tempfile::tempdir().expect("tempdir");
        std::fs::write(temp.path().join("README.txt"), b"no pkg").expect("write readme");

        assert!(find_pkg_in_dir(temp.path()).is_none());
    }

    #[test]
    fn test_select_installed_bundle_prefers_expected_bundle_name() {
        let bundles = vec![
            PathBuf::from("/Library/Audio/Plug-Ins/VST3/Other.vst3"),
            PathBuf::from("/Library/Audio/Plug-Ins/VST3/TestSynth.vst3"),
        ];

        let selected = select_installed_bundle(bundles, PluginFormat::Vst3, Some("TestSynth.vst3"))
            .expect("selected bundle");

        assert_eq!(
            selected,
            PathBuf::from("/Library/Audio/Plug-Ins/VST3/TestSynth.vst3")
        );
    }

    #[test]
    fn test_select_installed_bundle_prefers_requested_format_when_name_missing() {
        let bundles = vec![
            PathBuf::from("/Library/Audio/Plug-Ins/Components/TestSynth.component"),
            PathBuf::from("/Library/Audio/Plug-Ins/VST3/TestSynth.vst3"),
        ];

        let selected =
            select_installed_bundle(bundles, PluginFormat::Au, None).expect("selected bundle");

        assert_eq!(
            selected,
            PathBuf::from("/Library/Audio/Plug-Ins/Components/TestSynth.component")
        );
    }
}
