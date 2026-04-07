// pin command — pin or unpin a plugin, or list all pinned plugins.

use anyhow::Result;
use colored::Colorize;
use serde::Serialize;

use apm_core::config::Config;
use apm_core::state::InstallState;

#[derive(Serialize)]
struct PinnedEntry {
    name: String,
    version: String,
}

#[derive(Serialize)]
struct PinnedListJson {
    pinned: Vec<PinnedEntry>,
}

#[derive(Serialize)]
struct PinResultJson {
    pinned: bool,
    plugin: String,
    version: String,
}

#[derive(Serialize)]
struct UnpinResultJson {
    unpinned: bool,
    plugin: String,
}

pub async fn run(
    config: &Config,
    name: Option<&str>,
    unpin: bool,
    list: bool,
    json: bool,
) -> Result<()> {
    let mut state = InstallState::load(config)?;

    // ── List mode ─────────────────────────────────────────────────────────────

    if list {
        let pinned: Vec<_> = state.plugins.iter().filter(|p| p.pinned).collect();

        if pinned.is_empty() {
            if json {
                let result = PinnedListJson { pinned: vec![] };
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("No pinned plugins.");
                println!(
                    "Hint: Use `apm pin <plugin>` to prevent a plugin from being upgraded."
                );
            }
            return Ok(());
        }

        if json {
            let entries: Vec<PinnedEntry> = pinned
                .iter()
                .map(|p| PinnedEntry {
                    name: p.name.clone(),
                    version: p.version.clone(),
                })
                .collect();
            let result = PinnedListJson { pinned: entries };
            println!("{}", serde_json::to_string_pretty(&result)?);
            return Ok(());
        }

        let col_name = pinned
            .iter()
            .map(|p| p.name.len())
            .max()
            .unwrap_or(6)
            .max(6);

        println!(
            "{}",
            format!("{:<col_name$}  Version", "Plugin", col_name = col_name).bold()
        );
        println!("{}", "\u{2500}".repeat(col_name + 2 + 7).dimmed());

        for plugin in &pinned {
            println!(
                "{:<col_name$}  {}",
                plugin.name.bold().to_string(),
                plugin.version.cyan(),
                col_name = col_name,
            );
        }

        return Ok(());
    }

    // ── Pin / unpin mode ──────────────────────────────────────────────────────

    let plugin_name = match name {
        Some(n) => n,
        None => {
            anyhow::bail!(
                "Plugin name required.\n\
                 Usage: apm pin <plugin>       — pin a plugin\n\
                 Usage: apm pin -r <plugin>    — unpin a plugin\n\
                 Usage: apm pin --list         — list all pinned plugins"
            );
        }
    };

    // Check the plugin is installed.
    let plugin = match state.find(plugin_name) {
        Some(p) => p.clone(),
        None => {
            println!(
                "Plugin '{}' is not installed. Install it first with `apm install {}`.",
                plugin_name, plugin_name
            );
            return Ok(());
        }
    };

    if unpin {
        // Unpin.
        if let Some(p) = state.find_mut(plugin_name) {
            p.pinned = false;
        }
        state.save(config)?;

        if json {
            let result = UnpinResultJson {
                unpinned: true,
                plugin: plugin.name.clone(),
            };
            println!("{}", serde_json::to_string_pretty(&result)?);
        } else {
            println!(
                "{}",
                format!("Unpinned {} (v{})", plugin.name, plugin.version).green()
            );
        }
    } else {
        // Pin.
        if let Some(p) = state.find_mut(plugin_name) {
            p.pinned = true;
        }
        state.save(config)?;

        if json {
            let result = PinResultJson {
                pinned: true,
                plugin: plugin.name.clone(),
                version: plugin.version.clone(),
            };
            println!("{}", serde_json::to_string_pretty(&result)?);
        } else {
            println!(
                "{}",
                format!("Pinned {} at v{}", plugin.name, plugin.version).yellow()
            );
        }
    }

    Ok(())
}
