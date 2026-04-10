// stats command — show a quick summary of the user's apm environment.

use anyhow::Result;
use colored::Colorize;
use serde::Serialize;

use apm_core::config::Config;
use apm_core::registry::Registry;
use apm_core::state::InstallState;

#[derive(Serialize)]
struct StatsJson {
    installed: usize,
    available: usize,
    pinned: usize,
    sources: usize,
    cache_bytes: u64,
}

pub async fn run(config: &Config, json: bool) -> Result<()> {
    let state = InstallState::load(config)?;
    let registry = Registry::load_all_sources(config)?;
    let sources = config.sources();

    let installed = state.plugins.len();
    let available = registry.len();
    let pinned = state.plugins.iter().filter(|p| p.pinned).count();
    let source_count = sources.len();

    // Count AU and VST3 format installs across all plugins.
    let au_count = state
        .plugins
        .iter()
        .filter(|p| {
            p.formats
                .iter()
                .any(|f| f.format.to_string().eq_ignore_ascii_case("au"))
        })
        .count();
    let vst3_count = state
        .plugins
        .iter()
        .filter(|p| {
            p.formats
                .iter()
                .any(|f| f.format.to_string().eq_ignore_ascii_case("vst3"))
        })
        .count();

    // Walk the downloads cache directory and sum file sizes.
    let cache_bytes = cache_size(config);

    // ── JSON output ──────────────────────────────────────────────────────────
    if json {
        let stats = StatsJson {
            installed,
            available,
            pinned,
            sources: source_count,
            cache_bytes,
        };
        println!("{}", serde_json::to_string(&stats)?);
        return Ok(());
    }

    // ── Human output ─────────────────────────────────────────────────────────
    let label_width = 12;

    // Installed line with format breakdown.
    let installed_suffix = if installed == 1 { "" } else { "s" };
    let installed_value = if installed > 0 {
        format!("{installed} plugin{installed_suffix} ({au_count} AU, {vst3_count} VST3)")
    } else {
        format!("{installed} plugin{installed_suffix}")
    };
    println!("  {:<label_width$}{installed_value}", "Installed:".bold());

    // Available.
    let available_suffix = if available == 1 { "" } else { "s" };
    println!(
        "  {:<label_width$}{available} plugin{available_suffix} in registry",
        "Available:".bold(),
    );

    // Pinned.
    println!("  {:<label_width$}{}", "Pinned:".bold(), pinned,);

    // Sources.
    let source_names: Vec<&str> = sources.iter().map(|s| s.name.as_str()).collect();
    let source_label = if source_count <= 3 {
        format!("{} ({})", source_count, source_names.join(", "))
    } else {
        format!("{}", source_count)
    };
    println!("  {:<label_width$}{}", "Sources:".bold(), source_label,);

    // Cache size.
    println!(
        "  {:<label_width$}{}",
        "Cache:".bold(),
        format_bytes(cache_bytes),
    );

    // Last sync time.
    let sync_time = last_sync_time(config);
    if let Some(time_str) = sync_time {
        println!("  {:<label_width$}{}", "Last sync:".bold(), time_str,);
    }

    Ok(())
}

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Walk the downloads cache directory and return total size in bytes.
fn cache_size(config: &Config) -> u64 {
    let cache_dir = config.downloads_cache_dir();
    if !cache_dir.exists() {
        return 0;
    }

    walkdir::WalkDir::new(&cache_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .map(|e| e.metadata().map(|m| m.len()).unwrap_or(0))
        .sum()
}

/// Get the last sync time by reading the mtime of the registry cache directory.
fn last_sync_time(config: &Config) -> Option<String> {
    let reg_dir = config.registries_cache_dir();
    if !reg_dir.exists() {
        return None;
    }

    let mtime = std::fs::metadata(&reg_dir).ok()?.modified().ok()?;

    let dt: chrono::DateTime<chrono::Local> = mtime.into();
    Some(dt.format("%Y-%m-%d %H:%M").to_string())
}

/// Format a byte count into a human-readable string.
fn format_bytes(bytes: u64) -> String {
    if bytes == 0 {
        return "0 B".to_string();
    }

    const KB: f64 = 1024.0;
    const MB: f64 = 1024.0 * 1024.0;
    const GB: f64 = 1024.0 * 1024.0 * 1024.0;

    let b = bytes as f64;
    if b >= GB {
        format!("{:.1} GB", b / GB)
    } else if b >= MB {
        format!("{:.1} MB", b / MB)
    } else if b >= KB {
        format!("{:.1} KB", b / KB)
    } else {
        format!("{} B", bytes)
    }
}
