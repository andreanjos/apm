// diff command — compare installed plugins against the registry, showing a full
// picture: outdated, not-in-registry, and up-to-date plugins.

use anyhow::Result;
use colored::Colorize;
use semver::Version;
use serde::Serialize;

use apm_core::config::Config;
use apm_core::registry::Registry;
use apm_core::state::InstallState;

// ── JSON types ───────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct DiffJson {
    outdated: Vec<OutdatedJson>,
    not_in_registry: Vec<NotInRegistryJson>,
    up_to_date: Vec<UpToDateJson>,
}

#[derive(Serialize)]
struct OutdatedJson {
    name: String,
    installed_version: String,
    available_version: String,
    pinned: bool,
}

#[derive(Serialize)]
struct NotInRegistryJson {
    name: String,
    installed_version: String,
    source: String,
}

#[derive(Serialize)]
struct UpToDateJson {
    name: String,
    version: String,
}

// ── Internal types ───────────────────────────────────────────────────────────

struct OutdatedEntry {
    name: String,
    installed: String,
    available: String,
    pinned: bool,
}

struct NotInRegistryEntry {
    name: String,
    version: String,
    source: String,
}

struct UpToDateEntry {
    name: String,
    version: String,
}

// ── Command ──────────────────────────────────────────────────────────────────

pub async fn run(config: &Config, json: bool) -> Result<()> {
    let state = InstallState::load(config)?;

    if state.plugins.is_empty() {
        if json {
            let empty = DiffJson {
                outdated: vec![],
                not_in_registry: vec![],
                up_to_date: vec![],
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

    // ── Categorise each installed plugin ─────────────────────────────────────

    let mut outdated: Vec<OutdatedEntry> = Vec::new();
    let mut not_in_registry: Vec<NotInRegistryEntry> = Vec::new();
    let mut up_to_date: Vec<UpToDateEntry> = Vec::new();

    for installed in &state.plugins {
        let Some(registry_plugin) = registry.find(&installed.name) else {
            not_in_registry.push(NotInRegistryEntry {
                name: installed.name.clone(),
                version: installed.version.clone(),
                source: installed.source.clone(),
            });
            continue;
        };

        let latest_release = registry_plugin.latest_release();
        let registry_version = &latest_release.version;

        // Semver comparison with string fallback.
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
            up_to_date.push(UpToDateEntry {
                name: installed.name.clone(),
                version: installed.version.clone(),
            });
        }
    }

    // Sort each category alphabetically.
    outdated.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    not_in_registry.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    up_to_date.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

    // ── JSON output ──────────────────────────────────────────────────────────

    if json {
        let result = DiffJson {
            outdated: outdated
                .iter()
                .map(|e| OutdatedJson {
                    name: e.name.clone(),
                    installed_version: e.installed.clone(),
                    available_version: e.available.clone(),
                    pinned: e.pinned,
                })
                .collect(),
            not_in_registry: not_in_registry
                .iter()
                .map(|e| NotInRegistryJson {
                    name: e.name.clone(),
                    installed_version: e.version.clone(),
                    source: e.source.clone(),
                })
                .collect(),
            up_to_date: up_to_date
                .iter()
                .map(|e| UpToDateJson {
                    name: e.name.clone(),
                    version: e.version.clone(),
                })
                .collect(),
        };
        println!("{}", serde_json::to_string_pretty(&result)?);
        return Ok(());
    }

    // ── Human-readable output ────────────────────────────────────────────────

    let mut printed_section = false;

    // Outdated.
    if !outdated.is_empty() {
        let col_name = outdated
            .iter()
            .map(|e| e.name.len())
            .max()
            .unwrap_or(0);
        let col_inst = outdated
            .iter()
            .map(|e| e.installed.len())
            .max()
            .unwrap_or(0);

        println!(
            "{}",
            format!("Outdated ({}):", outdated.len()).yellow().bold()
        );
        for entry in &outdated {
            let pin_marker = if entry.pinned { " (pinned)" } else { "" };
            println!(
                "  {:<col_name$}  {:<col_inst$} {} {}{}",
                entry.name.bold(),
                entry.installed.cyan(),
                "\u{2192}".dimmed(),
                entry.available.green(),
                pin_marker.yellow(),
                col_name = col_name,
                col_inst = col_inst,
            );
        }
        printed_section = true;
    }

    // Not in registry.
    if !not_in_registry.is_empty() {
        if printed_section {
            println!();
        }
        let col_name = not_in_registry
            .iter()
            .map(|e| e.name.len())
            .max()
            .unwrap_or(0);

        println!(
            "{}",
            format!("Not in registry ({}):", not_in_registry.len())
                .red()
                .bold()
        );
        for entry in &not_in_registry {
            println!(
                "  {:<col_name$}  {}  {}",
                entry.name.bold(),
                entry.version.cyan(),
                format!("(installed from: {})", entry.source).dimmed(),
                col_name = col_name,
            );
        }
        printed_section = true;
    }

    // Up to date.
    if !up_to_date.is_empty() {
        if printed_section {
            println!();
        }
        println!(
            "{}",
            format!("Up to date ({}):", up_to_date.len()).green().bold()
        );
        // Compact comma-separated list for up-to-date since they are the boring
        // majority — no need to take lots of vertical space.
        let names: Vec<&str> = up_to_date.iter().map(|e| e.name.as_str()).collect();
        let line = format!("  {}", names.join(", "));
        println!("{}", line.dimmed());
    }

    // Summary hint.
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
    }

    Ok(())
}
