// Install orchestrator — coordinates download, extraction, placement, and
// quarantine removal for a single plugin. Atomic: if any format fails, all
// formats installed in this run are rolled back before returning the error.

pub mod dmg;
pub mod pkg;
pub mod quarantine;
pub mod zip;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::Utc;
use colored::Colorize;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use tracing::{debug, info};

use crate::config::{Config, InstallScope};
use crate::error::ApmError;
use crate::registry::{FormatSource, InstallType, PluginDefinition, PluginFormat};
use crate::state::{InstalledFormat, InstalledPlugin, InstallState};

// ── Public entry point ────────────────────────────────────────────────────────

/// Install `plugin`, optionally filtered to a single `format`.
///
/// # Atomicity
/// If any format fails mid-install, all bundles placed by this run are
/// removed before the error is returned — no partial installs are left on
/// disk.
///
/// # Flow
/// 1. Determine which formats to install.
/// 2. For each format:  download → verify SHA256 → extract → place → strip quarantine.
/// 3. Record all formats in `state` and save.
pub async fn install_plugin(
    plugin: &PluginDefinition,
    format_filter: Option<PluginFormat>,
    scope: Option<InstallScope>,
    config: &Config,
    state: &mut InstallState,
) -> Result<()> {
    let effective_scope = scope.unwrap_or(config.install_scope);

    // Collect (format, source) pairs to install.
    let mut to_install: Vec<(PluginFormat, &FormatSource)> = match format_filter {
        Some(fmt) => {
            let src = plugin.formats.get(&fmt).ok_or_else(|| ApmError::Install {
                plugin: plugin.slug.clone(),
                reason: format!(
                    "Plugin '{}' does not have a {} format available in the registry.",
                    plugin.slug, fmt
                ),
                hint: format!(
                    "Available formats: {}",
                    plugin
                        .formats
                        .keys()
                        .map(|f| f.to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            })?;
            vec![(fmt, src)]
        }
        None => plugin
            .formats
            .iter()
            .map(|(&fmt, src)| (fmt, src))
            .collect(),
    };

    // Sort formats for deterministic order: VST3 before AU.
    to_install.sort_by_key(|(fmt, _)| fmt.to_string());

    let multi_format = to_install.len() > 1;

    // ── Download + install each format ────────────────────────────────────────

    // Use a MultiProgress container so per-format progress bars render cleanly
    // alongside each other (indicatif handles cursor management).
    let mp = MultiProgress::new();

    let mut installed_paths: Vec<(PluginFormat, PathBuf)> = Vec::new();

    for (fmt, source) in &to_install {
        let fmt_label = if multi_format {
            format!("{:<5}", fmt.to_string())
        } else {
            fmt.to_string()
        };

        match install_one_format(plugin, *fmt, &fmt_label, source, effective_scope, config, &mp).await {
            Ok(bundle_path) => {
                // Print per-format installed path.
                let display = display_path(&bundle_path);
                println!(
                    "    {}: {}",
                    fmt_label.cyan(),
                    format!("Installed to {display}").green()
                );
                installed_paths.push((*fmt, bundle_path));
            }
            Err(e) => {
                // Roll back any formats already placed in this run.
                rollback(&installed_paths, &plugin.slug);
                return Err(e);
            }
        }
    }

    // ── Record in state ───────────────────────────────────────────────────────

    let formats: Vec<InstalledFormat> = installed_paths
        .iter()
        .map(|(fmt, path)| InstalledFormat {
            format: *fmt,
            path: path.clone(),
            sha256: String::new(), // bundle hash could be added in a future phase
        })
        .collect();

    let record = InstalledPlugin {
        name: plugin.slug.clone(),
        version: plugin.version.clone(),
        vendor: plugin.vendor.clone(),
        formats,
        installed_at: Utc::now(),
        source: "official".to_owned(),
        pinned: false,
    };

    state.record_install(record);
    state.save(config).with_context(|| {
        format!("Failed to save install state after installing '{}'", plugin.slug)
    })?;

    info!("Recorded '{}' in install state", plugin.slug);
    Ok(())
}

// ── Per-format installation ───────────────────────────────────────────────────

async fn install_one_format(
    plugin: &PluginDefinition,
    fmt: PluginFormat,
    fmt_label: &str,
    source: &FormatSource,
    scope: InstallScope,
    config: &Config,
    mp: &MultiProgress,
) -> Result<PathBuf> {
    let dest_dir = plugin_dest_dir(fmt, scope);

    // ── Step 1: Download ──────────────────────────────────────────────────────

    let archive_name = archive_filename(plugin, fmt, source);
    let archive_path = config.downloads_cache_dir().join(&archive_name);

    // Build a per-format progress bar with the format name as prefix.
    let pb = build_format_progress_bar(mp, fmt_label, None);

    crate::download::download_file_with_progress(
        &source.url,
        &archive_path,
        &source.sha256,
        pb,
    )
    .await
    .with_context(|| {
        format!(
            "Failed to download {} archive for '{}'",
            fmt, plugin.slug
        )
    })?;

    // ── Step 2: Verify (already done inside download_file_with_progress) ─────

    // ── Step 3: Extract + place ───────────────────────────────────────────────

    println!(
        "    {}: {}",
        fmt_label.cyan(),
        "Extracting...".dimmed()
    );

    let bundle_path = match source.install_type {
        InstallType::Dmg => {
            debug!("Using DMG installer");
            dmg::install_from_dmg(&archive_path, &dest_dir, fmt)
                .with_context(|| format!("DMG install failed for '{}' ({})", plugin.slug, fmt))?
        }
        InstallType::Pkg => {
            debug!("Using PKG installer");
            let paths = pkg::install_from_pkg(&archive_path).with_context(|| {
                format!("PKG install failed for '{}' ({})", plugin.slug, fmt)
            })?;
            // PKG installs to system paths; track the first bundle we found.
            paths.into_iter().next().ok_or_else(|| ApmError::Install {
                plugin: plugin.slug.clone(),
                reason: "PKG installer ran but no plugin bundle was located afterward".to_owned(),
                hint: "Check ~/Library/Audio/Plug-Ins/ and /Library/Audio/Plug-Ins/ manually."
                    .to_owned(),
            })?
        }
        InstallType::Zip => {
            debug!("Using ZIP installer");
            zip::install_from_zip(&archive_path, &dest_dir, fmt)
                .with_context(|| format!("ZIP install failed for '{}' ({})", plugin.slug, fmt))?
        }
    };

    // ── Step 4: Strip quarantine ──────────────────────────────────────────────

    quarantine::remove_quarantine(&bundle_path).with_context(|| {
        format!(
            "Quarantine removal failed for '{}' ({})",
            bundle_path.display(),
            fmt
        )
    })?;

    info!(
        "Installed {} {} ({}) at {}",
        plugin.name,
        plugin.version,
        fmt,
        bundle_path.display()
    );

    Ok(bundle_path)
}

// ── Rollback ──────────────────────────────────────────────────────────────────

/// Remove all bundles placed in this install run (best-effort).
fn rollback(installed: &[(PluginFormat, PathBuf)], plugin_slug: &str) {
    if installed.is_empty() {
        return;
    }
    eprintln!("  {}", format!("Rolling back partial install of '{plugin_slug}'...").yellow());
    for (fmt, path) in installed {
        if path.exists() {
            if let Err(e) = std::fs::remove_dir_all(path) {
                eprintln!(
                    "  {}: Could not remove partial {fmt} bundle at {}: {e}",
                    "Warning".yellow(),
                    path.display()
                );
            } else {
                eprintln!("  Removed partial {fmt} bundle: {}", path.display());
            }
        }
    }
}

// ── Path helpers ──────────────────────────────────────────────────────────────

fn plugin_dest_dir(fmt: PluginFormat, scope: InstallScope) -> PathBuf {
    match (fmt, scope) {
        (PluginFormat::Au, InstallScope::User) => crate::config::user_au_dir(),
        (PluginFormat::Au, InstallScope::System) => crate::config::system_au_dir(),
        (PluginFormat::Vst3, InstallScope::User) => crate::config::user_vst3_dir(),
        (PluginFormat::Vst3, InstallScope::System) => crate::config::system_vst3_dir(),
    }
}

/// Build a cache filename for the downloaded archive.
fn archive_filename(plugin: &PluginDefinition, fmt: PluginFormat, source: &FormatSource) -> String {
    // Try to preserve the original file extension from the URL.
    let url_path = Path::new(source.url.split('?').next().unwrap_or(&source.url));
    let ext = url_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or(match source.install_type {
            InstallType::Dmg => "dmg",
            InstallType::Pkg => "pkg",
            InstallType::Zip => "zip",
        });

    format!("{}-{}-{}.{}", plugin.slug, plugin.version, fmt.to_string().to_lowercase(), ext)
}

/// Build a per-format progress bar attached to the multi-progress container.
fn build_format_progress_bar(mp: &MultiProgress, fmt_label: &str, total: Option<u64>) -> ProgressBar {
    let pb = if let Some(n) = total {
        mp.add(ProgressBar::new(n))
    } else {
        mp.add(ProgressBar::new_spinner())
    };

    let prefix = format!("    {:<5}: Downloading", fmt_label);
    let style = ProgressStyle::with_template(
        "{prefix} [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({bytes_per_sec}, ETA {eta})",
    )
    .unwrap_or_else(|_| ProgressStyle::default_bar())
    .progress_chars("=>-");

    pb.set_style(style);
    pb.set_prefix(prefix);
    pb
}

/// Replace the user's home directory prefix with `~` for readability.
fn display_path(path: &Path) -> String {
    let path_str = path.to_string_lossy();
    if let Some(home) = dirs::home_dir() {
        let home_str = home.to_string_lossy();
        if let Some(rest) = path_str.strip_prefix(home_str.as_ref()) {
            return format!("~{rest}");
        }
    }
    path_str.into_owned()
}
