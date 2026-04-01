// import command — parse an exported plugin list and install all listed plugins.

use std::path::Path;

use anyhow::{Context, Result};
use colored::Colorize;

use crate::commands::export_cmd::{ExportDocument, ExportedPlugin};
use apm_core::config::Config;
use apm_core::registry::Registry;
use apm_core::state::InstallState;

// ── Entry point ───────────────────────────────────────────────────────────────

pub async fn run(config: &Config, file: &Path, dry_run: bool) -> Result<()> {
    // ── Parse the export file ─────────────────────────────────────────────────

    let doc = load_export_file(file)?;

    if doc.plugins.is_empty() {
        println!("No plugins found in {}.", file.display());
        return Ok(());
    }

    println!(
        "Found {} plugin(s) in {}.",
        doc.plugins.len(),
        file.display()
    );

    if dry_run {
        println!("{}", "(dry-run mode — no changes will be made)".dimmed());
    }

    // ── Load registry ─────────────────────────────────────────────────────────

    let registry = Registry::load_all_sources(config)?;

    if registry.is_empty() {
        anyhow::bail!(
            "Registry cache is empty.\n\
             Hint: Run `apm sync` to populate the local registry cache."
        );
    }

    let mut state = InstallState::load(config)?;

    // ── Process each plugin ───────────────────────────────────────────────────

    let mut installed = 0usize;
    let mut skipped = 0usize;
    let mut failed = 0usize;

    for entry in &doc.plugins {
        match process_one(config, entry, &registry, &mut state, dry_run).await {
            PluginOutcome::Installed => {
                installed += 1;
            }
            PluginOutcome::Skipped => {
                skipped += 1;
            }
            PluginOutcome::Failed(reason) => {
                eprintln!(
                    "  {} {}: {}",
                    "FAILED".red().bold(),
                    entry.name,
                    reason
                );
                failed += 1;
            }
        }
    }

    // ── Summary ───────────────────────────────────────────────────────────────

    let total = doc.plugins.len();
    let suffix = if dry_run { " (dry-run)" } else { "" };
    let summary = format!(
        "Imported {total} plugins ({installed} installed, {skipped} skipped, {failed} failed){suffix}"
    );

    if failed == 0 {
        println!("\n{}", summary.green());
    } else {
        println!("\n{}", summary.yellow());
    }

    Ok(())
}

// ── Per-plugin logic ──────────────────────────────────────────────────────────

enum PluginOutcome {
    Installed,
    Skipped,
    Failed(String),
}

async fn process_one(
    config: &Config,
    entry: &ExportedPlugin,
    registry: &Registry,
    state: &mut InstallState,
    dry_run: bool,
) -> PluginOutcome {
    // Already installed?
    if state.is_installed(&entry.name) {
        println!("  {} {} (already installed)", "skip".dimmed(), entry.name);
        return PluginOutcome::Skipped;
    }

    // Look up in registry.
    let plugin = match registry.find(&entry.name) {
        Some(p) => p,
        None => {
            return PluginOutcome::Failed(
                "not found in registry (try `apm sync`)".to_string()
            );
        }
    };

    if dry_run {
        println!("  {} {} v{}", "would install".cyan(), entry.name, plugin.version);
        return PluginOutcome::Installed; // count as "would install"
    }

    println!("  {} {} v{}...", "installing".cyan(), entry.name, plugin.version);

    match crate::install::install_plugin(plugin, None, None, config, state, None).await {
        Ok(()) => {
            println!("  {} {} v{}", "installed".green(), entry.name, plugin.version);
            PluginOutcome::Installed
        }
        Err(e) => {
            PluginOutcome::Failed(e.to_string())
        }
    }
}

// ── File loading ──────────────────────────────────────────────────────────────

/// Load and parse an export file, auto-detecting format by extension.
fn load_export_file(path: &Path) -> Result<ExportDocument> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("Cannot read import file: {}", path.display()))?;

    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    if ext == "json" {
        return serde_json::from_str(&raw)
            .with_context(|| format!("Failed to parse JSON import file: {}", path.display()));
    }

    // Try TOML first (covers .toml and unknown extensions).
    if let Ok(doc) = toml::from_str::<ExportDocument>(&raw) {
        return Ok(doc);
    }

    // Fall back to JSON.
    serde_json::from_str(&raw)
        .with_context(|| format!("Failed to parse import file as TOML or JSON: {}", path.display()))
}
