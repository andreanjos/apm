use anyhow::Result;

use crate::config::Config;
use crate::scanner::{self, PluginFormat};
use crate::state::InstallState;

// Maximum column widths for the scan table.
const MAX_NAME: usize = 35;
const MAX_VER: usize = 12;
const MAX_VENDOR: usize = 25;

pub async fn run(config: &Config) -> Result<()> {
    let plugins = scanner::scan_plugins(config);

    if plugins.is_empty() {
        println!("No audio plugins found in standard directories.");
        return Ok(());
    }

    // Load apm-managed install state for source annotation.
    // A missing or unreadable state file is treated as empty (no managed plugins).
    let state = InstallState::load(config).unwrap_or_default();

    // ── Column widths ─────────────────────────────────────────────────────────
    // Compute widths from data, capped at the defined maximums.

    const HDR_NAME: &str = "Name";
    const HDR_VER: &str = "Version";
    const HDR_VENDOR: &str = "Vendor";
    const HDR_FMT: &str = "Format";
    const HDR_SRC: &str = "Source";
    const HDR_LOC: &str = "Location";

    let w_name = plugins
        .iter()
        .map(|p| p.name.len().min(MAX_NAME))
        .max()
        .unwrap_or(0)
        .max(HDR_NAME.len());

    let w_ver = plugins
        .iter()
        .map(|p| p.version.len().min(MAX_VER))
        .max()
        .unwrap_or(0)
        .max(HDR_VER.len());

    let w_vendor = plugins
        .iter()
        .map(|p| p.vendor.len().min(MAX_VENDOR))
        .max()
        .unwrap_or(0)
        .max(HDR_VENDOR.len());

    // Format column is at most 4 chars ("VST3") — header wins.
    let w_fmt = HDR_FMT.len();
    // Source column: "apm" (3) or "-" (1) — header "Source" wins.
    let w_src = HDR_SRC.len();

    // ── Header ────────────────────────────────────────────────────────────────
    println!(
        "{:<w_name$}  {:<w_ver$}  {:<w_vendor$}  {:<w_fmt$}  {:<w_src$}  {}",
        HDR_NAME,
        HDR_VER,
        HDR_VENDOR,
        HDR_FMT,
        HDR_SRC,
        HDR_LOC,
    );

    let rule_len = w_name + 2 + w_ver + 2 + w_vendor + 2 + w_fmt + 2 + w_src + 2 + HDR_LOC.len();
    println!("{}", "\u{2500}".repeat(rule_len)); // ─────

    // ── Rows ──────────────────────────────────────────────────────────────────
    for p in &plugins {
        // Display the path in a human-friendly way: abbreviate $HOME to ~
        let path_str = display_path(&p.path);

        let name_cell = truncate(&p.name, MAX_NAME);
        let ver_cell = truncate(&p.version, MAX_VER);
        let vendor_cell = truncate(&p.vendor, MAX_VENDOR);

        // Determine if this plugin was installed by apm: match by path (most
        // precise) or by name as a fallback.
        let is_managed = state.plugins.iter().any(|sp| {
            sp.formats.iter().any(|f| f.path == p.path)
                || sp.name.eq_ignore_ascii_case(&p.name)
        });
        let source_cell = if is_managed { "apm" } else { "-" };

        println!(
            "{:<w_name$}  {:<w_ver$}  {:<w_vendor$}  {:<w_fmt$}  {:<w_src$}  {}",
            name_cell,
            ver_cell,
            vendor_cell,
            p.format.to_string(),
            source_cell,
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

/// Truncate `s` to `max` characters, appending "..." if truncated.
fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_owned()
    } else {
        // Ensure the suffix fits: we always have max >= 3 from our constants.
        format!("{}...", &s[..max.saturating_sub(3)])
    }
}
