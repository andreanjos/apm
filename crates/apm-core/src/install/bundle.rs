use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use tracing::{debug, info};
use walkdir::WalkDir;

use crate::error::ApmError;

pub(crate) fn find_bundles(root: &Path, extension: &str, max_depth: usize) -> Vec<PathBuf> {
    let mut bundles: Vec<PathBuf> = WalkDir::new(root)
        .min_depth(1)
        .max_depth(max_depth)
        .into_iter()
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            entry.file_type().is_dir()
                && entry.path().extension().and_then(|value| value.to_str()) == Some(extension)
        })
        .map(|entry| entry.into_path())
        .collect();
    bundles.sort();
    bundles
}

pub(crate) fn select_bundle(
    root: &Path,
    bundles: Vec<PathBuf>,
    expected_bundle_path: Option<&str>,
) -> Result<PathBuf> {
    let fallback = bundles
        .first()
        .cloned()
        .context("No bundle candidates were available")?;

    let Some(expected) = expected_bundle_path.and_then(normalize_expected_bundle_path) else {
        return Ok(fallback);
    };

    Ok(bundles
        .iter()
        .find(|bundle| relative_bundle_path(root, bundle) == expected)
        .or_else(|| {
            bundles.iter().find(|bundle| {
                bundle
                    .file_name()
                    .zip(expected.file_name())
                    .is_some_and(|(bundle_name, expected_name)| bundle_name == expected_name)
            })
        })
        .cloned()
        .unwrap_or(fallback))
}

pub(crate) fn find_pkg_in_dir(root: &Path) -> Option<PathBuf> {
    let mut packages: Vec<PathBuf> = WalkDir::new(root)
        .min_depth(1)
        .max_depth(6)
        .into_iter()
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let path = entry.into_path();
            let extension = path.extension().and_then(|value| value.to_str())?;
            matches!(extension, "pkg" | "mpkg").then_some(path)
        })
        .collect();
    packages.sort();
    packages.into_iter().next()
}

pub(crate) fn copy_bundle(bundle_src: &Path, dest_dir: &Path) -> Result<PathBuf> {
    super::ensure_dir(dest_dir)?;

    let bundle_name = bundle_src
        .file_name()
        .context("Bundle path has no file name")?;
    let dest_bundle = dest_dir.join(bundle_name);

    if dest_bundle.exists() {
        std::fs::remove_dir_all(&dest_bundle)
            .with_context(|| format!("Cannot remove existing bundle: {}", dest_bundle.display()))?;
    }

    debug!(
        "Copying bundle {} -> {}",
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

fn normalize_expected_bundle_path(value: &str) -> Option<PathBuf> {
    let trimmed = value.trim().trim_start_matches('/').trim_end_matches('/');
    (!trimmed.is_empty()).then(|| PathBuf::from(trimmed))
}

fn relative_bundle_path(root: &Path, bundle: &Path) -> PathBuf {
    bundle
        .strip_prefix(root)
        .map(Path::to_path_buf)
        .unwrap_or_else(|_| bundle.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn select_bundle_prefers_expected_relative_path() {
        let root = PathBuf::from("/tmp/mounted");
        let selected = select_bundle(
            &root,
            vec![root.join("Other.vst3"), root.join("Nested/Preferred.vst3")],
            Some("Nested/Preferred.vst3"),
        )
        .expect("selected bundle");

        assert_eq!(selected, root.join("Nested/Preferred.vst3"));
    }

    #[test]
    fn select_bundle_falls_back_to_expected_file_name() {
        let root = PathBuf::from("/tmp/mounted");
        let selected = select_bundle(
            &root,
            vec![root.join("Nested/Preferred.component")],
            Some("Preferred.component"),
        )
        .expect("selected bundle");

        assert_eq!(selected, root.join("Nested/Preferred.component"));
    }

    #[test]
    fn find_pkg_in_dir_finds_nested_pkg() {
        let temp = tempfile::tempdir().expect("temp dir");
        let nested = temp.path().join("Vendor/Installer.pkg/Contents");
        std::fs::create_dir_all(&nested).expect("create pkg");

        let found = find_pkg_in_dir(temp.path()).expect("pkg should be found");

        assert_eq!(found, temp.path().join("Vendor/Installer.pkg"));
    }

    #[test]
    fn find_pkg_in_dir_finds_flat_mpkg() {
        let temp = tempfile::tempdir().expect("temp dir");
        let package = temp.path().join("Vendor/Installer.mpkg");
        std::fs::create_dir_all(package.parent().expect("package parent")).expect("create vendor");
        std::fs::write(&package, "meta package").expect("write package");

        let found = find_pkg_in_dir(temp.path()).expect("mpkg should be found");

        assert_eq!(found, package);
    }

    #[test]
    fn copy_bundle_replaces_existing_bundle() {
        let temp = tempfile::tempdir().expect("temp dir");
        let src = temp.path().join("src/Test.vst3");
        let dest = temp.path().join("dest");
        std::fs::create_dir_all(src.join("Contents")).expect("create source bundle");
        std::fs::write(src.join("Contents/Info.plist"), "new").expect("write source");
        std::fs::create_dir_all(dest.join("Test.vst3/Contents")).expect("create old bundle");
        std::fs::write(dest.join("Test.vst3/Contents/Info.plist"), "old").expect("write old");

        let installed = copy_bundle(&src, &dest).expect("copy bundle");

        assert_eq!(installed, dest.join("Test.vst3"));
        assert_eq!(
            std::fs::read_to_string(installed.join("Contents/Info.plist")).expect("read copied"),
            "new"
        );
    }

    #[test]
    fn find_bundles_filters_by_extension() {
        let temp = tempfile::tempdir().expect("temp dir");
        std::fs::create_dir_all(temp.path().join("A.vst3")).expect("create vst3");
        std::fs::create_dir_all(temp.path().join("B.component")).expect("create au");

        let bundles = find_bundles(temp.path(), "vst3", 2);

        assert_eq!(bundles, vec![temp.path().join("A.vst3")]);
    }
}
