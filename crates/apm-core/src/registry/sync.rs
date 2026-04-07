// Registry sync — clones or fast-forward fetches a Git-backed registry source
// into the local cache directory. Uses git2 so no external `git` binary is
// required.

use std::path::{Path, PathBuf};

use anyhow::Result;
use git2::{FetchOptions, RemoteCallbacks, Repository};
use tracing::debug;

use crate::config::ensure_dir;
use crate::error::ApmError;
use crate::registry::Source;

/// Check whether a source URL refers to a local filesystem path rather than
/// a remote Git URL. Returns the resolved path if local, `None` otherwise.
pub fn local_path(url: &str) -> Option<PathBuf> {
    let expanded = if url.starts_with('~') {
        if let Some(home) = dirs::home_dir() {
            home.join(&url[1..].trim_start_matches('/'))
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

    let mut callbacks = RemoteCallbacks::new();
    callbacks.transfer_progress(|stats| {
        if stats.received_objects() == stats.total_objects() && stats.total_objects() > 0 {
            print!(
                "\r  Resolving deltas {}/{}...",
                stats.indexed_deltas(),
                stats.total_deltas()
            );
        } else if stats.total_objects() > 0 {
            print!(
                "\r  Receiving objects {}/{} ({:.0}%)...",
                stats.received_objects(),
                stats.total_objects(),
                stats.received_objects() as f64 / stats.total_objects() as f64 * 100.0,
            );
        }
        true
    });

    let mut fetch_opts = FetchOptions::new();
    fetch_opts.remote_callbacks(callbacks);

    let mut builder = git2::build::RepoBuilder::new();
    builder.fetch_options(fetch_opts);

    builder.clone(&source.url, dest).map_err(|e| {
        // Produce a user-friendly error: distinguish network from other issues.
        let reason = e.to_string();
        if reason.contains("failed to resolve")
            || reason.contains("network")
            || reason.contains("ssl")
            || reason.contains("connect")
        {
            anyhow::Error::from(ApmError::RegistrySync {
                source_name: source.name.clone(),
                reason: format!(
                    "Failed to clone registry. Check your internet connection.\n  Details: {reason}"
                ),
            })
        } else {
            anyhow::Error::from(ApmError::RegistrySync {
                source_name: source.name.clone(),
                reason,
            })
        }
    })?;

    // Print a newline after the inline progress line.
    println!();
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

    let repo = Repository::open(dest).map_err(|e| ApmError::RegistrySync {
        source_name: source.name.clone(),
        reason: format!("Cannot open local cache repository: {e}"),
    })?;

    // Fetch origin.
    {
        let mut callbacks = RemoteCallbacks::new();
        callbacks.transfer_progress(|stats| {
            if stats.total_objects() > 0 {
                print!(
                    "\r  Fetching {}/{} objects...",
                    stats.received_objects(),
                    stats.total_objects()
                );
            }
            true
        });

        let mut fetch_opts = FetchOptions::new();
        fetch_opts.remote_callbacks(callbacks);

        let mut remote = repo
            .find_remote("origin")
            .map_err(|e| ApmError::RegistrySync {
                source_name: source.name.clone(),
                reason: format!("Cannot find 'origin' remote: {e}"),
            })?;

        remote
            .fetch(&["refs/heads/*:refs/remotes/origin/*"], Some(&mut fetch_opts), None)
            .map_err(|e| {
                let reason = e.to_string();
                if reason.contains("failed to resolve")
                    || reason.contains("network")
                    || reason.contains("ssl")
                    || reason.contains("connect")
                {
                    ApmError::RegistrySync {
                        source_name: source.name.clone(),
                        reason: format!(
                            "Failed to fetch registry updates. Check your internet connection.\n  Details: {reason}"
                        ),
                    }
                } else {
                    ApmError::RegistrySync {
                        source_name: source.name.clone(),
                        reason,
                    }
                }
            })?;

        println!(); // newline after progress
    }

    // Find the remote tracking branch: try origin/main then origin/master.
    let remote_ref = find_remote_branch(&repo, &source.name)?;

    // Hard-reset the working tree to the fetched HEAD.
    let commit = repo
        .find_reference(&remote_ref)
        .and_then(|r| r.peel_to_commit())
        .map_err(|e| ApmError::RegistrySync {
            source_name: source.name.clone(),
            reason: format!("Cannot resolve remote branch '{remote_ref}': {e}"),
        })?;

    repo.reset(commit.as_object(), git2::ResetType::Hard, None)
        .map_err(|e| ApmError::RegistrySync {
            source_name: source.name.clone(),
            reason: format!("Failed to reset working tree: {e}"),
        })?;

    debug!("Reset to {} ({})", remote_ref, commit.id());
    Ok(())
}

/// Find a remote tracking branch — tries `refs/remotes/origin/main` first,
/// then `refs/remotes/origin/master`. Returns the ref name as a string.
fn find_remote_branch(repo: &Repository, source_name: &str) -> Result<String> {
    for branch in &["main", "master"] {
        let refname = format!("refs/remotes/origin/{branch}");
        if repo.find_reference(&refname).is_ok() {
            return Ok(refname);
        }
    }
    Err(ApmError::RegistrySync {
        source_name: source_name.to_string(),
        reason: "Cannot find refs/remotes/origin/main or refs/remotes/origin/master. \
                 The registry repository must have a 'main' or 'master' branch."
            .to_string(),
    }
    .into())
}
