// backup — copy plugin bundles before upgrading and restore them on rollback.


use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tracing::{debug, info};

use apm_core::config::Config;
use crate::install::quarantine;
use apm_core::state::{InstalledFormat, InstalledPlugin, InstallState};

// ── BackupEntry ───────────────────────────────────────────────────────────────

/// Metadata for a single plugin backup stored on disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupEntry {
    /// Plugin slug (e.g. `"valhalla-supermassive"`).
    pub slug: String,

    /// Plugin version that was backed up.
    pub version: String,

    /// Formats included in this backup.
    pub formats: Vec<String>,

    /// UTC timestamp when the backup was created.
    pub created_at: DateTime<Utc>,

    /// Absolute path to the backup directory (`<backups_dir>/<slug>/<version>/`).
    pub backup_dir: PathBuf,
}

impl BackupEntry {
    /// Return the total disk size of this backup in bytes.
    pub fn size_bytes(&self) -> u64 {
        dir_size(&self.backup_dir)
    }
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Back up the installed bundles for `plugin` into `<backups_dir>/<slug>/<version>/`.
///
/// Returns a `BackupEntry` describing what was saved.
/// Returns an error only for hard failures (e.g. cannot create backup directory).
pub fn backup_plugin(plugin: &InstalledPlugin, config: &Config) -> Result<BackupEntry> {
    let backup_root = config
        .backups_dir()
        .join(&plugin.name)
        .join(&plugin.version);

    apm_core::config::ensure_dir(&backup_root).with_context(|| {
        format!("Cannot create backup directory: {}", backup_root.display())
    })?;

    let mut backed_up_formats: Vec<String> = Vec::new();

    for fmt in &plugin.formats {
        let src = &fmt.path;
        if !src.exists() {
            tracing::warn!(
                "Backup: bundle path {} does not exist; skipping.",
                src.display()
            );
            continue;
        }

        let bundle_name = src
            .file_name()
            .ok_or_else(|| anyhow::anyhow!("Cannot get bundle filename from {}", src.display()))?;

        let dest = backup_root.join(bundle_name);

        copy_dir_all(src, &dest).with_context(|| {
            format!(
                "Failed to copy bundle {} to backup {}",
                src.display(),
                dest.display()
            )
        })?;

        backed_up_formats.push(fmt.format.to_string().to_lowercase());
        debug!("Backed up {} bundle to {}", fmt.format, dest.display());
    }

    let entry = BackupEntry {
        slug: plugin.name.clone(),
        version: plugin.version.clone(),
        formats: backed_up_formats,
        created_at: Utc::now(),
        backup_dir: backup_root,
    };

    info!(
        "Backed up '{}' v{} ({} format(s))",
        plugin.name,
        plugin.version,
        entry.formats.len()
    );

    Ok(entry)
}

/// Restore the most recent backup for `slug` back to its original install paths,
/// update the install state, and strip quarantine.
///
/// Returns an error if no backup is found or the restore fails.
pub fn restore_plugin(slug: &str, config: &Config, state: &mut InstallState) -> Result<()> {
    let entry = find_latest_backup(slug, config)?
        .ok_or_else(|| anyhow::anyhow!("No backup found for '{slug}'."))?;

    info!(
        "Restoring '{}' v{} from {}",
        slug,
        entry.version,
        entry.backup_dir.display()
    );

    // Determine the formats in the backup by listing the directory.
    let bundle_paths: Vec<PathBuf> = std::fs::read_dir(&entry.backup_dir)
        .with_context(|| {
            format!(
                "Cannot read backup directory: {}",
                entry.backup_dir.display()
            )
        })?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .collect();

    // Re-use the paths recorded in the state if the plugin is still tracked
    // (e.g. after a failed upgrade); otherwise infer from the backup names.
    let mut restored_formats: Vec<InstalledFormat> = Vec::new();

    if let Some(installed) = state.find(slug) {
        // Restore to the existing install paths.
        for fmt in &installed.formats {
            let bundle_name = fmt
                .path
                .file_name()
                .ok_or_else(|| anyhow::anyhow!("Cannot get bundle name from state path"))?;
            let src = entry.backup_dir.join(bundle_name);

            if !src.exists() {
                anyhow::bail!(
                    "Backup bundle not found at {}",
                    src.display()
                );
            }

            // Remove current (possibly broken) bundle.
            if fmt.path.exists() {
                std::fs::remove_dir_all(&fmt.path).with_context(|| {
                    format!("Cannot remove existing bundle at {}", fmt.path.display())
                })?;
            }

            copy_dir_all(&src, &fmt.path).with_context(|| {
                format!(
                    "Cannot restore bundle {} to {}",
                    src.display(),
                    fmt.path.display()
                )
            })?;

            quarantine::remove_quarantine(&fmt.path)?;
            restored_formats.push(fmt.clone());
            debug!("Restored {} to {}", src.display(), fmt.path.display());
        }
    } else {
        // Plugin not in state — restore each bundle to the inferred install dir.
        for src in &bundle_paths {
            let ext = src.extension().and_then(|e| e.to_str()).unwrap_or("");
            let dest_dir = match ext {
                "component" => apm_core::config::user_au_dir(),
                "vst3" => apm_core::config::user_vst3_dir(),
                _ => continue,
            };

            apm_core::config::ensure_dir(&dest_dir)?;

            let bundle_name = src
                .file_name()
                .ok_or_else(|| anyhow::anyhow!("Cannot get bundle name"))?;

            let dest = dest_dir.join(bundle_name);
            copy_dir_all(src, &dest).with_context(|| {
                format!(
                    "Cannot restore bundle {} to {}",
                    src.display(),
                    dest.display()
                )
            })?;

            quarantine::remove_quarantine(&dest)?;

            let fmt_str = entry
                .formats
                .iter()
                .find(|f| dest.to_string_lossy().contains(f.as_str()))
                .cloned()
                .unwrap_or_else(|| ext.to_string());

            let format = if fmt_str == "au" || fmt_str == "component" {
                apm_core::registry::PluginFormat::Au
            } else {
                apm_core::registry::PluginFormat::Vst3
            };

            restored_formats.push(InstalledFormat {
                format,
                path: dest.clone(),
                sha256: String::new(),
            });
        }
    }

    // Update state with the restored version.
    if let Some(existing) = state.find_mut(slug) {
        existing.version = entry.version.clone();
        existing.formats = restored_formats;
    } else {
        state.plugins.push(InstalledPlugin {
            name: slug.to_owned(),
            version: entry.version.clone(),
            vendor: String::new(),
            formats: restored_formats,
            installed_at: entry.created_at,
            source: "backup".to_owned(),
            pinned: false,
        });
    }

    state.save(config)?;
    println!("Restored '{}' v{} from backup.", slug, entry.version);
    Ok(())
}

/// List all backups in `<backups_dir>/`, one entry per slug+version.
pub fn list_backups(config: &Config) -> Result<Vec<BackupEntry>> {
    let backups_root = config.backups_dir();

    if !backups_root.exists() {
        return Ok(Vec::new());
    }

    let mut entries: Vec<BackupEntry> = Vec::new();

    // Walk <backups_dir>/<slug>/<version>/
    for slug_entry in std::fs::read_dir(&backups_root)
        .with_context(|| format!("Cannot read backups dir: {}", backups_root.display()))?
        .filter_map(|e| e.ok())
    {
        let slug_path = slug_entry.path();
        if !slug_path.is_dir() {
            continue;
        }

        let slug = slug_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();

        let version_entries = match std::fs::read_dir(&slug_path) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        for version_entry in version_entries.filter_map(|e| e.ok())
        {
            let version_path = version_entry.path();
            if !version_path.is_dir() {
                continue;
            }

            let version = version_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string();

            // Infer formats from contents.
            let formats: Vec<String> = std::fs::read_dir(&version_path)
                .map(|rd| {
                    rd.filter_map(|e| e.ok())
                        .filter_map(|e| {
                            let p = e.path();
                            p.extension()
                                .and_then(|ext| ext.to_str())
                                .map(|ext| match ext {
                                    "component" => "au".to_string(),
                                    "vst3" => "vst3".to_string(),
                                    _ => ext.to_string(),
                                })
                        })
                        .collect()
                })
                .unwrap_or_default();

            // Use directory mtime as a proxy for created_at.
            let created_at = std::fs::metadata(&version_path)
                .and_then(|m| m.modified())
                .map(DateTime::<Utc>::from)
                .unwrap_or_else(|_| Utc::now());

            entries.push(BackupEntry {
                slug: slug.clone(),
                version,
                formats,
                created_at,
                backup_dir: version_path,
            });
        }
    }

    // Sort by slug then version.
    entries.sort_by(|a, b| a.slug.cmp(&b.slug).then(a.version.cmp(&b.version)));
    Ok(entries)
}

/// Return the most recent backup entry for a given slug, or `None` if absent.
pub fn find_latest_backup(slug: &str, config: &Config) -> Result<Option<BackupEntry>> {
    let all = list_backups(config)?;
    let latest = all
        .into_iter()
        .filter(|e| e.slug.eq_ignore_ascii_case(slug))
        .max_by(|a, b| a.created_at.cmp(&b.created_at));
    Ok(latest)
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Recursively copy a directory tree from `src` to `dst`.
fn copy_dir_all(src: &Path, dst: &Path) -> Result<()> {
    if !src.is_dir() {
        // Single file copy.
        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(src, dst)?;
        return Ok(());
    }

    std::fs::create_dir_all(dst)?;

    for entry in std::fs::read_dir(src)
        .with_context(|| format!("Cannot read dir: {}", src.display()))?
    {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        if src_path.is_dir() {
            copy_dir_all(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path).with_context(|| {
                format!(
                    "Cannot copy {} to {}",
                    src_path.display(),
                    dst_path.display()
                )
            })?;
        }
    }

    Ok(())
}

/// Return the total size in bytes of all regular files under `path`.
fn dir_size(path: &Path) -> u64 {
    walkdir::WalkDir::new(path)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .filter_map(|e| std::fs::metadata(e.path()).ok())
        .map(|m| m.len())
        .sum()
}
