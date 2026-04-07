// remove command — delete plugin bundle(s) from disk and remove from state.

use anyhow::Result;
use colored::Colorize;

use apm_core::config::Config;
use apm_core::state::InstallState;

pub async fn run(config: &Config, name: &str) -> Result<()> {
    // ── Load state ────────────────────────────────────────────────────────────

    let mut state = InstallState::load(config)?;

    // ── Look up the plugin ────────────────────────────────────────────────────

    let plugin = match state.find(name) {
        Some(p) => p.clone(),
        None => {
            println!(
                "Plugin '{}' is not installed via apm. Nothing to remove.",
                name
            );
            return Ok(());
        }
    };

    // ── Show what will be removed ─────────────────────────────────────────────

    let format_names: Vec<String> = plugin
        .formats
        .iter()
        .map(|f| f.format.to_string())
        .collect();
    println!(
        "Removing {} v{}...",
        plugin.name.bold(),
        plugin.version.cyan()
    );

    // ── Delete each bundle from disk ──────────────────────────────────────────

    for fmt in &plugin.formats {
        let path = &fmt.path;
        if path.exists() {
            std::fs::remove_dir_all(path).map_err(|e| {
                anyhow::anyhow!(
                    "Failed to remove {} bundle at {}: {}",
                    fmt.format,
                    path.display(),
                    e
                )
            })?;
        } else {
            eprintln!(
                "Warning: {} bundle not found at {} (already removed?)",
                fmt.format,
                path.display()
            );
        }
    }

    // ── Remove from state and save ────────────────────────────────────────────

    state.remove(&plugin.name);
    state.save(config)?;

    // ── Success message ───────────────────────────────────────────────────────

    println!(
        "{}",
        format!(
            "Removed {} v{} ({})",
            plugin.name,
            plugin.version,
            format_names.join(", ")
        )
        .green()
    );

    Ok(())
}
