// Registry sync — clones or fast-forward fetches a Git-backed registry source
// into the local cache directory.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::Result;
use tracing::debug;

use crate::config::ensure_dir;
use crate::error::ApmError;
use crate::registry::Source;

/// Check whether a source URL refers to a local filesystem path rather than
/// a remote Git URL. Returns the resolved path if local, `None` otherwise.
pub fn local_path(url: &str) -> Option<PathBuf> {
    let expanded = if let Some(stripped) = url.strip_prefix('~') {
        if let Some(home) = dirs::home_dir() {
            home.join(stripped.trim_start_matches('/'))
        } else {
            return None;
        }
    } else {
        PathBuf::from(url)
    };

    if expanded.is_absolute() && expanded.exists() {
        Some(expanded)
    } else if url.starts_with("./") || url.starts_with("../") {
        let abs = std::env::current_dir().ok()?.join(url);
        if abs.exists() {
            Some(abs)
        } else {
            None
        }
    } else {
        None
    }
}

/// Sync a single registry source to `<registries_cache_dir>/<source.name>/`.
///
/// - If the source URL is a local filesystem path, creates a symlink into the
///   cache (or updates it if the target changed).
/// - If the target directory does not exist: clone the repository.
/// - If it already exists: fetch `origin` and reset to `origin/main` (or
///   `origin/master` as a fallback).
///
/// Progress is printed to stdout so the user knows the network call is active.
pub fn sync_source(source: &Source, registries_cache_dir: &Path) -> Result<()> {
    ensure_dir(registries_cache_dir)?;

    let dest = registries_cache_dir.join(&source.name);

    // Local filesystem path — symlink into cache instead of git clone.
    if let Some(local) = local_path(&source.url) {
        return sync_local(&local, &dest, source);
    }

    if dest.exists() {
        fetch_and_reset(&dest, source)
    } else {
        clone_repo(&dest, source)
    }
}

/// Sync a local filesystem registry into the cache via symlink.
fn sync_local(local_path: &Path, dest: &Path, source: &Source) -> Result<()> {
    debug!(
        "Syncing local source '{}': {} → {}",
        source.name,
        local_path.display(),
        dest.display()
    );

    // Remove existing symlink or directory if it points elsewhere.
    if dest.exists() || dest.symlink_metadata().is_ok() {
        if dest.is_symlink() {
            let current_target = std::fs::read_link(dest).unwrap_or_default();
            if current_target == local_path {
                debug!("Symlink already correct, nothing to do");
                return Ok(());
            }
            std::fs::remove_file(dest).map_err(|e| ApmError::RegistrySync {
                source_name: source.name.clone(),
                reason: format!("Cannot remove existing symlink: {e}"),
            })?;
        } else {
            // Existing directory (e.g. from a previous git clone) — remove it
            // so we can replace with a symlink.
            std::fs::remove_dir_all(dest).map_err(|e| ApmError::RegistrySync {
                source_name: source.name.clone(),
                reason: format!("Cannot remove existing cache directory: {e}"),
            })?;
        }
    }

    #[cfg(unix)]
    std::os::unix::fs::symlink(local_path, dest).map_err(|e| ApmError::RegistrySync {
        source_name: source.name.clone(),
        reason: format!("Cannot create symlink: {e}"),
    })?;

    #[cfg(not(unix))]
    {
        // Fallback: copy the directory tree on non-Unix platforms.
        copy_dir_recursive(local_path, dest).map_err(|e| ApmError::RegistrySync {
            source_name: source.name.clone(),
            reason: format!("Cannot copy local registry: {e}"),
        })?;
    }

    debug!("Local source synced via symlink");
    Ok(())
}

#[cfg(not(unix))]
fn copy_dir_recursive(src: &Path, dest: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dest)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let target = dest.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_recursive(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

// ── Clone ─────────────────────────────────────────────────────────────────────

fn clone_repo(dest: &Path, source: &Source) -> Result<()> {
    debug!("Cloning {} → {}", source.url, dest.display());

    run_git(
        Command::new("git")
            .arg("clone")
            .arg("--depth")
            .arg("1")
            .arg(&source.url)
            .arg(dest),
        source,
        "clone registry",
    )?;

    debug!("Clone complete: {}", dest.display());
    Ok(())
}

// ── Fetch + Reset ─────────────────────────────────────────────────────────────

fn fetch_and_reset(dest: &Path, source: &Source) -> Result<()> {
    debug!(
        "Fetching updates for '{}' at {}",
        source.name,
        dest.display()
    );

    run_git(
        Command::new("git")
            .arg("-C")
            .arg(dest)
            .arg("fetch")
            .arg("--depth")
            .arg("1")
            .arg("origin"),
        source,
        "fetch registry updates",
    )?;

    run_git(
        Command::new("git")
            .arg("-C")
            .arg(dest)
            .arg("reset")
            .arg("--hard")
            .arg("FETCH_HEAD"),
        source,
        "reset registry cache",
    )?;

    debug!("Reset {} to FETCH_HEAD", dest.display());
    Ok(())
}

fn run_git(command: &mut Command, source: &Source, action: &str) -> Result<()> {
    let output = command.output().map_err(|e| ApmError::RegistrySync {
        source_name: source.name.clone(),
        reason: format!("Failed to run `git` to {action}: {e}"),
    })?;

    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let detail = if stderr.is_empty() { stdout } else { stderr };

    Err(ApmError::RegistrySync {
        source_name: source.name.clone(),
        reason: format!("Failed to {action}. Details: {detail}"),
    }
    .into())
}
