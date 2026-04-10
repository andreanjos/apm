// outdated command — compare installed versions against the registry and show
// plugins that have newer versions available.

use anyhow::Result;
use colored::Colorize;
use semver::Version;
use serde::Serialize;

use apm_core::config::Config;
use apm_core::registry::Registry;
use apm_core::state::InstallState;

// ── JSON types ───────────────────────────────────────────────────────────────

/// Top-level JSON output for the outdated command.
#[derive(Serialize)]
struct OutdatedResultJson {
    outdated: Vec<OutdatedPluginJson>,
    up_to_date_count: usize,
    pinned_count: usize,
}

/// JSON-serializable view of a single outdated plugin.
#[derive(Serialize)]
struct OutdatedPluginJson {
    name: String,
    installed: String,
    available: String,
    pinned: bool,
}

// ── Internal types ───────────────────────────────────────────────────────────

struct OutdatedEntry {
    name: String,
    installed: String,
    available: String,
    pinned: bool,
}

pub async fn run(config: &Config, json: bool) -> Result<()> {
    // ── Load state and registry ───────────────────────────────────────────────

    let state = InstallState::load(config)?;

    if state.plugins.is_empty() {
        if json {
            let empty = OutdatedResultJson {
                outdated: vec![],
                up_to_date_count: 0,
                pinned_count: 0,
            };
            println!("{}", serde_json::to_string_pretty(&empty)?);
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

    let mut outdated: Vec<OutdatedEntry> = Vec::new();
    let mut up_to_date_count: usize = 0;

    for installed in &state.plugins {
        let Some(registry_plugin) = registry.find(&installed.name) else {
            // Not in registry — skip silently (could be from a removed source).
            continue;
        };

        let latest_release = registry_plugin.latest_release();
        let registry_version = &latest_release.version;

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
        } else {
            up_to_date_count += 1;
        }
    }

    let pinned_count = outdated.iter().filter(|e| e.pinned).count();

    // ── Display results ───────────────────────────────────────────────────────

    if outdated.is_empty() {
        if json {
            let result = OutdatedResultJson {
                outdated: vec![],
                up_to_date_count,
                pinned_count: 0,
            };
            println!("{}", serde_json::to_string_pretty(&result)?);
        } else {
            println!("All {} plugins are up to date.", up_to_date_count);
        }
        return Ok(());
    }

    // ── JSON output ───────────────────────────────────────────────────────────
    if json {
        let result = OutdatedResultJson {
            outdated: outdated
                .iter()
                .map(|e| OutdatedPluginJson {
                    name: e.name.clone(),
                    installed: e.installed.clone(),
                    available: e.available.clone(),
                    pinned: e.pinned,
                })
                .collect(),
            up_to_date_count,
            pinned_count,
        };
        println!("{}", serde_json::to_string_pretty(&result)?);
        return Ok(());
    }

    // ── Human-readable table ─────────────────────────────────────────────────

    // Calculate column widths.
    let col_name = outdated
        .iter()
        .map(|e| e.name.len())
        .max()
        .unwrap_or(6)
        .max(6);
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
            "Plugin",
            "Installed",
            "Available",
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

    // ── Summary ──────────────────────────────────────────────────────────────

    let upgradeable = outdated.len() - pinned_count;

    let mut summary_parts: Vec<String> = Vec::new();
    summary_parts.push(format!(
        "{} outdated, {} up to date",
        outdated.len(),
        up_to_date_count
    ));
    if pinned_count > 0 {
        summary_parts.push(format!("{} pinned", pinned_count));
    }
    println!("\n{}", summary_parts.join(", ").dimmed());

    if upgradeable > 0 {
        println!(
            "{}",
            format!(
                "{} plugin(s) can be upgraded. Run 'apm upgrade' to upgrade all.",
                upgradeable
            )
            .yellow()
        );
    } else {
        println!("{}", "All outdated plugins are pinned.".yellow());
    }

    Ok(())
}
