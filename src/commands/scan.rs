use anyhow::Result;

use crate::config::Config;
use crate::scanner::{self, PluginFormat};

pub async fn run(config: &Config) -> Result<()> {
    let plugins = scanner::scan_plugins(config);

    if plugins.is_empty() {
        println!("No audio plugins found in standard directories.");
        return Ok(());
    }

    // ── Column widths ─────────────────────────────────────────────────────────
    // Compute widths from data so the table always fits.

    const HDR_NAME: &str = "Name";
    const HDR_VER: &str = "Version";
    const HDR_VENDOR: &str = "Vendor";
    const HDR_FMT: &str = "Format";
    const HDR_LOC: &str = "Location";

    let w_name = plugins
        .iter()
        .map(|p| p.name.len())
        .max()
        .unwrap_or(0)
        .max(HDR_NAME.len());

    let w_ver = plugins
        .iter()
        .map(|p| p.version.len())
        .max()
        .unwrap_or(0)
        .max(HDR_VER.len());

    let w_vendor = plugins
        .iter()
        .map(|p| p.vendor.len())
        .max()
        .unwrap_or(0)
        .max(HDR_VENDOR.len());

    // Format column is at most 4 chars ("VST3") — header wins.
    let w_fmt = HDR_FMT.len();

    // ── Header ────────────────────────────────────────────────────────────────
    println!(
        "{:<w_name$}  {:<w_ver$}  {:<w_vendor$}  {:<w_fmt$}  {}",
        HDR_NAME,
        HDR_VER,
        HDR_VENDOR,
        HDR_FMT,
        HDR_LOC,
    );

    let rule_len = w_name + 2 + w_ver + 2 + w_vendor + 2 + w_fmt + 2 + HDR_LOC.len();
    println!("{}", "\u{2500}".repeat(rule_len)); // ─────

    // ── Rows ──────────────────────────────────────────────────────────────────
    for p in &plugins {
        // Display the path in a human-friendly way: abbreviate $HOME to ~
        let path_str = display_path(&p.path);

        println!(
            "{:<w_name$}  {:<w_ver$}  {:<w_vendor$}  {:<w_fmt$}  {}",
            p.name,
            p.version,
            p.vendor,
            p.format.to_string(),
            path_str,
        );
    }

    // ── Summary ───────────────────────────────────────────────────────────────
    let n_au = plugins.iter().filter(|p| p.format == PluginFormat::Au).count();
    let n_vst3 = plugins
        .iter()
        .filter(|p| p.format == PluginFormat::Vst3)
        .count();

    println!();
    println!(
        "Found {} plugin{} ({} AU, {} VST3)",
        plugins.len(),
        if plugins.len() == 1 { "" } else { "s" },
        n_au,
        n_vst3,
    );

    Ok(())
}

/// Replace the user's home directory prefix with `~` for readability.
fn display_path(path: &std::path::Path) -> String {
    let path_str = path.to_string_lossy();
    if let Some(home) = dirs::home_dir() {
        let home_str = home.to_string_lossy();
        if let Some(rest) = path_str.strip_prefix(home_str.as_ref()) {
            return format!("~{rest}");
        }
    }
    path_str.into_owned()
}
