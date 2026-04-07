// history command — show install/remove/upgrade timeline.

use anyhow::Result;
use colored::Colorize;
use serde::Serialize;

use apm_core::config::Config;
use apm_core::state::InstallState;

/// JSON-serializable entry for `apm history`.
#[derive(Serialize)]
struct HistoryEntryJson {
    plugin: String,
    version: String,
    installed_at: String,
    source: String,
    formats: Vec<String>,
}

/// JSON-serializable output for `apm history`.
#[derive(Serialize)]
struct HistoryJson {
    history: Vec<HistoryEntryJson>,
}

pub async fn run(config: &Config, limit: Option<usize>, json: bool) -> Result<()> {
    let state = InstallState::load(config)?;

    // Sort by installed_at descending (most recent first).
    let mut plugins = state.plugins.clone();
    plugins.sort_by(|a, b| b.installed_at.cmp(&a.installed_at));

    // Apply limit if specified.
    if let Some(n) = limit {
        plugins.truncate(n);
    }

    if plugins.is_empty() {
        if json {
            let output = HistoryJson { history: vec![] };
            println!("{}", serde_json::to_string_pretty(&output)?);
        } else {
            println!("No plugins installed via apm. Use 'apm install <plugin>' to get started.");
        }
        return Ok(());
    }

    if json {
        let entries: Vec<HistoryEntryJson> = plugins
            .iter()
            .map(|p| {
                let mut formats: Vec<String> =
                    p.formats.iter().map(|f| f.format.to_string()).collect();
                formats.sort();
                formats.dedup();
                HistoryEntryJson {
                    plugin: p.name.clone(),
                    version: p.version.clone(),
                    installed_at: p.installed_at.format("%Y-%m-%dT%H:%M:%SZ").to_string(),
                    source: p.source.clone(),
                    formats,
                }
            })
            .collect();

        let output = HistoryJson { history: entries };
        println!("{}", serde_json::to_string_pretty(&output)?);
        return Ok(());
    }

    println!("{}", "Plugin history (most recent first):".bold());
    println!();

    for plugin in &plugins {
        let date = plugin.installed_at.format("%Y-%m-%d").to_string();
        let mut formats: Vec<String> = plugin
            .formats
            .iter()
            .map(|f| f.format.to_string())
            .collect();
        formats.sort();
        formats.dedup();
        let fmt_label = formats.join(", ");

        println!(
            "  {}  {} {} ({}) from {}",
            date.dimmed(),
            plugin.name.bold(),
            plugin.version.cyan(),
            fmt_label,
            plugin.source.dimmed(),
        );
    }

    println!();
    println!(
        "{}",
        format!(
            "{} plugin{} total.",
            plugins.len(),
            if plugins.len() == 1 { "" } else { "s" }
        )
        .dimmed()
    );

    Ok(())
}
