// outdated command — compare installed versions against the registry and show
// plugins that have newer versions available.

use anyhow::Result;
use colored::Colorize;
use semver::Version;
use serde::Serialize;

use apm_core::config::Config;
use apm_core::registry::Registry;
use apm_core::state::InstallState;

/// JSON-serializable view of an outdated plugin.
#[derive(Serialize)]
struct OutdatedPluginJson {
    name: String,
    installed_version: String,
    available_version: String,
    pinned: bool,
}

pub async fn run(config: &Config, json: bool) -> Result<()> {
    // ── Load state and registry ───────────────────────────────────────────────

    let state = InstallState::load(config)?;

    if state.plugins.is_empty() {
        if json {
            println!("[]");
        } else {
            println!("No plugins installed via apm.");
        }
        return Ok(());
    }

    let registry = Registry::load_all_sources(config)?;

    if registry.is_empty() {
        anyhow::bail!(
            "Registry cache is empty.\n\
             Hint: Run `apm sync` to populate the local registry cache."
        );
    }

    // ── Compare installed vs registry versions ────────────────────────────────

    struct OutdatedEntry {
        name: String,
        installed: String,
        available: String,
        pinned: bool,
    }

    let mut outdated: Vec<OutdatedEntry> = Vec::new();

    for installed in &state.plugins {
        let Some(registry_plugin) = registry.find(&installed.name) else {
            // Not in registry — skip silently (could be from a removed source).
            continue;
        };

        let registry_version = &registry_plugin.version;

        // Check if registry version is newer using semver; fall back to string
        // comparison when versions are not valid semver.
        let is_newer = match (
            Version::parse(&installed.version),
            Version::parse(registry_version),
        ) {
            (Ok(inst_v), Ok(reg_v)) => reg_v > inst_v,
            _ => registry_version.as_str() != installed.version.as_str(),
        };

        if is_newer {
            outdated.push(OutdatedEntry {
                name: installed.name.clone(),
                installed: installed.version.clone(),
                available: registry_version.clone(),
                pinned: installed.pinned,
            });
        }
    }

    // ── Display results ───────────────────────────────────────────────────────

    if outdated.is_empty() {
        if json {
            println!("[]");
        } else {
            println!("All plugins are up to date.");
        }
        return Ok(());
    }

    // ── JSON output ───────────────────────────────────────────────────────────
    if json {
        let json_results: Vec<OutdatedPluginJson> = outdated
            .iter()
            .map(|e| OutdatedPluginJson {
                name: e.name.clone(),
                installed_version: e.installed.clone(),
                available_version: e.available.clone(),
                pinned: e.pinned,
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&json_results)?);
        return Ok(());
    }

    // Calculate column widths.
    let col_name = outdated.iter().map(|e| e.name.len()).max().unwrap_or(6).max(6);
    let col_inst = outdated
        .iter()
        .map(|e| e.installed.len())
        .max()
        .unwrap_or(9)
        .max(9);
    let col_avail = outdated
        .iter()
        .map(|e| e.available.len())
        .max()
        .unwrap_or(9)
        .max(9);

    // Header.
    println!(
        "{}",
        format!(
            "{:<col_name$}  {:<col_inst$}  {:<col_avail$}  Status",
            "Plugin", "Installed", "Available",
            col_name = col_name,
            col_inst = col_inst,
            col_avail = col_avail,
        )
        .bold()
    );

    // Separator line.
    let total_width = col_name + 2 + col_inst + 2 + col_avail + 2 + 6;
    println!("{}", "\u{2500}".repeat(total_width).dimmed());

    for entry in &outdated {
        let status = if entry.pinned {
            "pinned".yellow().to_string()
        } else {
            String::new()
        };
        println!(
            "{:<col_name$}  {:<col_inst$}  {:<col_avail$}  {}",
            entry.name.bold().to_string(),
            entry.installed.cyan().to_string(),
            entry.available.green().to_string(),
            status,
            col_name = col_name,
            col_inst = col_inst,
            col_avail = col_avail,
        );
    }

    // Summary.
    let upgradeable = outdated.iter().filter(|e| !e.pinned).count();
    if upgradeable > 0 {
        println!(
            "\n{}",
            format!(
                "{} plugin(s) can be upgraded. Run 'apm upgrade' to upgrade all.",
                upgradeable
            )
            .yellow()
        );
    } else {
        println!("\n{}", "All upgradeable plugins are pinned.".yellow());
    }

    Ok(())
}
