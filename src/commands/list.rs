use anyhow::Result;

use crate::config::Config;
use crate::state::{InstallState, InstalledPlugin};

pub async fn run(config: &Config) -> Result<()> {
    let state = InstallState::load(config)?;

    if state.plugins.is_empty() {
        println!("No plugins installed via apm. Use 'apm install <plugin>' to get started.");
        return Ok(());
    }

    // ── Column widths ─────────────────────────────────────────────────────────

    const HDR_NAME: &str = "Name";
    const HDR_VER: &str = "Version";
    const HDR_FMT: &str = "Format";
    const HDR_PATH: &str = "Path";

    let w_name = state
        .plugins
        .iter()
        .map(|p| p.name.len())
        .max()
        .unwrap_or(0)
        .max(HDR_NAME.len());

    let w_ver = state
        .plugins
        .iter()
        .map(|p| p.version.len())
        .max()
        .unwrap_or(0)
        .max(HDR_VER.len());

    let w_fmt = state
        .plugins
        .iter()
        .map(|p| format_label(p).len())
        .max()
        .unwrap_or(0)
        .max(HDR_FMT.len());

    // ── Header ────────────────────────────────────────────────────────────────
    println!(
        "{:<w_name$}  {:<w_ver$}  {:<w_fmt$}  {}",
        HDR_NAME, HDR_VER, HDR_FMT, HDR_PATH,
    );

    let rule_len = w_name + 2 + w_ver + 2 + w_fmt + 2 + HDR_PATH.len();
    println!("{}", "\u{2500}".repeat(rule_len));

    // ── Rows ──────────────────────────────────────────────────────────────────
    for plugin in &state.plugins {
        let fmt_label = format_label(plugin);

        // Show the parent directory that contains all installed bundles for
        // this plugin. If a plugin has formats in multiple locations we show
        // the first one; in practice all bundles for one plugin share a root.
        let path_str = plugin
            .formats
            .first()
            .and_then(|f| f.path.parent())
            .map(display_path)
            .unwrap_or_default();

        println!(
            "{:<w_name$}  {:<w_ver$}  {:<w_fmt$}  {}",
            plugin.name, plugin.version, fmt_label, path_str,
        );
    }

    // ── Summary ───────────────────────────────────────────────────────────────
    println!();
    println!(
        "{} plugin{} managed by apm.",
        state.plugins.len(),
        if state.plugins.len() == 1 { "" } else { "s" }
    );

    Ok(())
}

/// Build a combined format label like "AU", "VST3", or "VST3+AU".
fn format_label(plugin: &InstalledPlugin) -> String {
    let mut parts: Vec<String> = plugin
        .formats
        .iter()
        .map(|f| f.format.to_string())
        .collect();
    // Deterministic order: VST3 before AU.
    parts.sort();
    parts.dedup();
    parts.join("+")
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
