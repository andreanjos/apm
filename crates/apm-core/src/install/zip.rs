use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use tracing::{debug, info};

use crate::error::ApmError;
use crate::registry::PluginFormat;

/// Extract `zip_path`, find the requested plugin bundle, copy it into
/// `dest_dir`, and return the installed bundle path.
pub fn install_from_zip(
    zip_path: &Path,
    dest_dir: &Path,
    format: PluginFormat,
    expected_bundle_path: Option<&str>,
) -> Result<PathBuf> {
    install_from_zip_with_pkg_handler(
        zip_path,
        dest_dir,
        format,
        expected_bundle_path,
        |pkg_path| {
            Err(ApmError::Install {
                plugin: zip_path.display().to_string(),
                reason: format!(
                    "ZIP archive contains a PKG installer at {}, but shared ZIP execution does not run PKG installers yet",
                    pkg_path.display()
                ),
                hint: "Use the existing CLI/vendor installer path for this package until PKG execution is added to the shared engine.".to_string(),
            }
            .into())
        },
    )
}

/// Variant used by the CLI to preserve its legacy ZIP-with-PKG fallback while
/// keeping ZIP extraction, bundle selection, and bundle copy canonical in core.
pub fn install_from_zip_with_pkg_handler(
    zip_path: &Path,
    dest_dir: &Path,
    format: PluginFormat,
    expected_bundle_path: Option<&str>,
    pkg_handler: impl FnOnce(&Path) -> Result<PathBuf>,
) -> Result<PathBuf> {
    info!("Extracting ZIP: {}", zip_path.display());

    let temp_dir = tempfile::Builder::new()
        .prefix("apm-zip-")
        .tempdir()
        .context("Cannot create temp directory for ZIP extraction")?;

    extract_zip(zip_path, temp_dir.path())?;

    let extension = super::bundle_extension(format);
    let bundles = super::bundle::find_bundles(temp_dir.path(), extension, 6);
    if bundles.is_empty() {
        return no_bundle_error(zip_path, temp_dir.path(), extension, format, pkg_handler);
    }

    let bundle_src = super::bundle::select_bundle(temp_dir.path(), bundles, expected_bundle_path)?;
    super::bundle::copy_bundle(&bundle_src, dest_dir)
}

fn extract_zip(zip_path: &Path, dest: &Path) -> Result<()> {
    use std::io::Read;

    let file = std::fs::File::open(zip_path)
        .with_context(|| format!("Cannot open ZIP file: {}", zip_path.display()))?;
    let mut archive = zip::ZipArchive::new(file)
        .with_context(|| format!("Cannot read ZIP archive: {}", zip_path.display()))?;

    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .with_context(|| format!("Cannot read ZIP entry {index}"))?;

        let Some(entry_path) = entry.enclosed_name().map(|path| path.to_owned()) else {
            debug!("Skipping unsafe ZIP entry name at index {index}");
            continue;
        };
        let out_path = dest.join(&entry_path);

        if entry.is_dir() {
            std::fs::create_dir_all(&out_path)
                .with_context(|| format!("Cannot create directory: {}", out_path.display()))?;
            continue;
        }

        if let Some(parent) = out_path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Cannot create directory: {}", parent.display()))?;
        }

        let mut out_file = std::fs::File::create(&out_path)
            .with_context(|| format!("Cannot create file: {}", out_path.display()))?;
        let mut buffer = Vec::new();
        entry
            .read_to_end(&mut buffer)
            .with_context(|| format!("Cannot read ZIP entry: {}", entry_path.display()))?;
        std::io::Write::write_all(&mut out_file, &buffer)
            .with_context(|| format!("Cannot write file: {}", out_path.display()))?;

        #[cfg(unix)]
        if let Some(mode) = entry.unix_mode() {
            use std::os::unix::fs::PermissionsExt;
            let permissions = std::fs::Permissions::from_mode(mode);
            let _ = std::fs::set_permissions(&out_path, permissions);
        }
    }

    debug!("ZIP extracted to {}", dest.display());
    Ok(())
}

fn no_bundle_error(
    zip_path: &Path,
    extraction_root: &Path,
    extension: &str,
    format: PluginFormat,
    pkg_handler: impl FnOnce(&Path) -> Result<PathBuf>,
) -> Result<PathBuf> {
    if let Some(pkg_path) = super::bundle::find_pkg_in_dir(extraction_root) {
        return pkg_handler(&pkg_path);
    }

    Err(ApmError::Install {
        plugin: zip_path.display().to_string(),
        reason: format!("No .{extension} bundle found inside the ZIP archive"),
        hint: format!(
            "The ZIP may package the {format} plugin under a different path. Check the registry entry's bundle_path field."
        ),
    }
    .into())
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use zip::write::SimpleFileOptions;

    use super::*;

    #[test]
    fn install_from_zip_prefers_expected_bundle_path() {
        let temp = tempfile::tempdir().expect("temp dir");
        let archive = temp.path().join("plugin.zip");
        write_zip(
            &archive,
            &[
                "Other.vst3/Contents/Info.plist",
                "Nested/Preferred.vst3/Contents/Info.plist",
            ],
        );
        let dest = temp.path().join("dest");

        let installed = install_from_zip(
            &archive,
            &dest,
            PluginFormat::Vst3,
            Some("Nested/Preferred.vst3"),
        )
        .expect("zip should install");

        assert_eq!(installed, dest.join("Preferred.vst3"));
        assert!(installed.join("Contents/Info.plist").exists());
        assert!(!dest.join("Other.vst3").exists());
    }

    #[test]
    fn install_from_zip_reports_pkg_archives_as_unsupported() {
        let temp = tempfile::tempdir().expect("temp dir");
        let archive = temp.path().join("plugin.zip");
        write_zip(&archive, &["Installer.pkg/Contents/PkgInfo"]);
        let dest = temp.path().join("dest");

        let error = install_from_zip(&archive, &dest, PluginFormat::Vst3, None)
            .expect_err("pkg-only zip should not install");

        assert!(error
            .to_string()
            .contains("does not run PKG installers yet"));
    }

    fn write_zip(path: &Path, entries: &[&str]) {
        let file = std::fs::File::create(path).expect("create zip");
        let mut writer = zip::ZipWriter::new(file);
        let options = SimpleFileOptions::default().unix_permissions(0o644);

        for entry in entries {
            writer.start_file(entry, options).expect("start zip file");
            writer.write_all(b"test").expect("write zip file");
        }

        writer.finish().expect("finish zip");
    }
}
