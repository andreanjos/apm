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
use sha2::{Digest, Sha256};
use tracing::{debug, info};

use apm_core::config::{Config, InstallScope};
use apm_core::error::ApmError;
use apm_core::registry::{FormatSource, InstallType, PluginDefinition, PluginFormat};
use apm_core::state::{InstallOrigin, InstallState, InstalledFormat, InstalledPlugin};

// ── SHA256 placeholder detection ──────────────────────────────────────────────

/// Returns `true` when the sha256 value is an empty/placeholder that should
/// be treated as "no checksum available".
fn is_placeholder_sha256(sha256: &str) -> bool {
    let s = sha256.trim();
    s.is_empty() || s.eq_ignore_ascii_case("manual") || s.chars().all(|c| c == '0')
}

// ── Public entry point ────────────────────────────────────────────────────────

/// Install `plugin`, optionally filtered to a single `format`.
///
/// When `from_file` is `Some`, the download step is skipped and the provided
/// file path is used as the archive directly.
///
/// # Atomicity
/// If any format fails mid-install, all bundles placed by this run are
/// removed before the error is returned — no partial installs are left on
/// disk.
///
/// # Flow
/// 1. Determine which formats to install.
/// 2. For each format:  download (or use local file) → verify SHA256 → extract → place → strip quarantine.
/// 3. Record all formats in `state` and save.
pub async fn install_plugin(
    plugin: &PluginDefinition,
    format_filter: Option<PluginFormat>,
    scope: Option<InstallScope>,
    config: &Config,
    state: &mut InstallState,
    from_file: Option<&Path>,
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

        match install_one_format(FormatInstallCtx {
            plugin,
            fmt: *fmt,
            fmt_label: &fmt_label,
            source,
            scope: effective_scope,
            config,
            mp: &mp,
            from_file,
        })
        .await
        {
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
        source: plugin
            .source_name
            .clone()
            .unwrap_or_else(|| "official".to_owned()),
        pinned: false,
        origin: InstallOrigin::Apm,
    };

    state.record_install(record);
    state.save(config).with_context(|| {
        format!(
            "Failed to save install state after installing '{}'",
            plugin.slug
        )
    })?;

    info!("Recorded '{}' in install state", plugin.slug);
    Ok(())
}

// ── Per-format installation ───────────────────────────────────────────────────

/// Bundles the arguments shared across a single-format install to stay within
/// clippy's `too_many_arguments` limit.
struct FormatInstallCtx<'a> {
    plugin: &'a PluginDefinition,
    fmt: PluginFormat,
    fmt_label: &'a str,
    source: &'a FormatSource,
    scope: InstallScope,
    config: &'a Config,
    mp: &'a MultiProgress,
    from_file: Option<&'a Path>,
}

async fn install_one_format(ctx: FormatInstallCtx<'_>) -> Result<PathBuf> {
    let FormatInstallCtx {
        plugin,
        fmt,
        fmt_label,
        source,
        scope,
        config,
        mp,
        from_file,
    } = ctx;
    let dest_dir = plugin_dest_dir(fmt, scope);

    // ── Step 1: Obtain archive (download or use local file) ───────────────────

    let archive_path: PathBuf = if let Some(local_path) = from_file {
        // Use the provided local file directly; validate it exists.
        if !local_path.exists() {
            anyhow::bail!(
                "Local file not found: {}\n\
                 Hint: Check the path and try again.",
                local_path.display()
            );
        }

        // Verify SHA256 of the local file if the registry has a real checksum.
        let sha = &source.sha256;
        if is_placeholder_sha256(sha) {
            println!(
                "    {}: {}",
                fmt_label.cyan(),
                "Warning: No SHA256 checksum available for this plugin. Skipping integrity verification."
                    .yellow()
            );
        } else {
            println!(
                "    {}: {}",
                fmt_label.cyan(),
                "Verifying checksum...".dimmed()
            );
            verify_local_file_sha256(local_path, sha).with_context(|| {
                format!(
                    "Checksum verification failed for local file '{}'",
                    local_path.display()
                )
            })?;
        }

        local_path.to_path_buf()
    } else {
        // Normal download flow.
        let archive_name = archive_filename(plugin, fmt, source);
        let archive_path = config.downloads_cache_dir().join(&archive_name);

        // Build a per-format progress bar with the format name as prefix.
        let pb = build_format_progress_bar(mp, fmt_label, None);

        crate::download::download_file_with_progress_cached(
            &source.url,
            &archive_path,
            &source.sha256,
            pb,
            config,
        )
        .await
        .with_context(|| format!("Failed to download {} archive for '{}'", fmt, plugin.slug))?;

        archive_path
    };

    // ── Step 2: Extract + place ───────────────────────────────────────────────

    println!("    {}: {}", fmt_label.cyan(), "Extracting...".dimmed());

    let bundle_path = match source.install_type {
        InstallType::Dmg => {
            debug!("Using DMG installer");
            dmg::install_from_dmg(&archive_path, &dest_dir, fmt, source.bundle_path.as_deref())
                .with_context(|| format!("DMG install failed for '{}' ({})", plugin.slug, fmt))?
        }
        InstallType::Pkg => {
            debug!("Using PKG installer");
            let paths = pkg::install_from_pkg(&archive_path)
                .with_context(|| format!("PKG install failed for '{}' ({})", plugin.slug, fmt))?;
            // PKG installs to system paths; select the bundle that matches the
            // requested format or expected bundle path.
            pkg::select_installed_bundle(paths, fmt, source.bundle_path.as_deref()).map_err(
                |error| ApmError::Install {
                    plugin: plugin.slug.clone(),
                    reason: error.to_string(),
                    hint: "Check ~/Library/Audio/Plug-Ins/ and /Library/Audio/Plug-Ins/ manually."
                        .to_owned(),
                },
            )?
        }
        InstallType::Zip => {
            debug!("Using ZIP installer");
            zip::install_from_zip(&archive_path, &dest_dir, fmt, source.bundle_path.as_deref())
                .with_context(|| format!("ZIP install failed for '{}' ({})", plugin.slug, fmt))?
        }
        InstallType::Mas => {
            anyhow::bail!(
                "'{}' is distributed through the Mac App Store and cannot be installed from an archive.\n\
                 Hint: Open the product page and install it with the App Store app.",
                plugin.slug
            );
        }
    };

    // ── Step 3: Strip quarantine ──────────────────────────────────────────────

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

// ── SHA256 verification for local files ───────────────────────────────────────

fn verify_local_file_sha256(path: &Path, expected_sha256: &str) -> Result<()> {
    use std::io::Read;

    let mut file = std::fs::File::open(path)
        .with_context(|| format!("Cannot open file for checksum: {}", path.display()))?;

    let mut hasher = Sha256::new();
    let mut buf = [0u8; 65536];

    loop {
        let n = file
            .read(&mut buf)
            .with_context(|| format!("Read error while hashing: {}", path.display()))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }

    let actual_hex = hex::encode(hasher.finalize());
    let expected_lower = expected_sha256.to_lowercase();
    let actual_lower = actual_hex.to_lowercase();

    if expected_lower != actual_lower {
        return Err(ApmError::Checksum {
            expected: expected_sha256.to_owned(),
            actual: actual_hex,
        }
        .into());
    }

    debug!("SHA256 OK for local file: {actual_hex}");
    Ok(())
}

// ── Rollback ──────────────────────────────────────────────────────────────────

/// Remove all bundles placed in this install run (best-effort).
fn rollback(installed: &[(PluginFormat, PathBuf)], plugin_slug: &str) {
    if installed.is_empty() {
        return;
    }
    eprintln!(
        "  {}",
        format!("Rolling back partial install of '{plugin_slug}'...").yellow()
    );
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
        (PluginFormat::Au, InstallScope::User) => apm_core::config::user_au_dir(),
        (PluginFormat::Au, InstallScope::System) => apm_core::config::system_au_dir(),
        (PluginFormat::Vst3, InstallScope::User) => apm_core::config::user_vst3_dir(),
        (PluginFormat::Vst3, InstallScope::System) => apm_core::config::system_vst3_dir(),
        (PluginFormat::App, InstallScope::User) => dirs::home_dir()
            .map(|home| home.join("Applications"))
            .unwrap_or_else(|| PathBuf::from("/Applications")),
        (PluginFormat::App, InstallScope::System) => PathBuf::from("/Applications"),
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
            InstallType::Mas => "app",
        });

    format!(
        "{}-{}-{}.{}",
        plugin.slug,
        plugin.version,
        fmt.to_string().to_lowercase(),
        ext
    )
}

/// Build a per-format progress bar attached to the multi-progress container.
fn build_format_progress_bar(
    mp: &MultiProgress,
    fmt_label: &str,
    total: Option<u64>,
) -> ProgressBar {
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
