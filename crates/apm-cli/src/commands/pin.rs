// pin command — pin or unpin a plugin, or list all pinned plugins.

use anyhow::Result;
use colored::Colorize;
use serde::Serialize;

use apm_core::config::Config;
use apm_core::engine::{
    ApmEngine, InstalledPackageSummary, PinnedPackagesRequest, SetPackagePinRequest,
    SetPackagePinResult,
};

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

#[derive(Serialize)]
struct PinMissingJson {
    plugin: String,
    installed: bool,
    changed: bool,
    reason: String,
}

pub async fn run(
    config: &Config,
    name: Option<&str>,
    unpin: bool,
    list: bool,
    json: bool,
) -> Result<()> {
    let engine = ApmEngine::new(config.clone());

    // ── List mode ─────────────────────────────────────────────────────────────

    if list {
        let pinned = engine.pinned_packages(PinnedPackagesRequest)?;

        if pinned.is_empty() {
            if json {
                let result = PinnedListJson { pinned: vec![] };
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("No pinned plugins.");
                println!("Hint: Use `apm pin <plugin>` to prevent a plugin from being upgraded.");
            }
            return Ok(());
        }

        if json {
            let entries: Vec<PinnedEntry> = pinned
                .iter()
                .map(|p| PinnedEntry {
                    name: p.slug.clone(),
                    version: p.version.clone(),
                })
                .collect();
            let result = PinnedListJson { pinned: entries };
            println!("{}", serde_json::to_string_pretty(&result)?);
            return Ok(());
        }

        let col_name = pinned
            .iter()
            .map(|p| p.slug.len())
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
                plugin.slug.bold().to_string(),
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

    let result = engine.set_package_pin(SetPackagePinRequest {
        slug: plugin_name.to_string(),
        pinned: !unpin,
    })?;
    print_pin_result(plugin_name, unpin, json, result)?;

    Ok(())
}

fn print_pin_result(
    plugin_name: &str,
    unpin: bool,
    json: bool,
    result: SetPackagePinResult,
) -> Result<()> {
    match result {
        SetPackagePinResult::NotInstalled { .. } => print_missing(plugin_name, json),
        SetPackagePinResult::Changed { package, .. }
        | SetPackagePinResult::Unchanged { package, .. } => {
            if unpin {
                print_unpinned(&package, json)
            } else {
                print_pinned(&package, json)
            }
        }
    }
}

fn print_missing(plugin_name: &str, json: bool) -> Result<()> {
    if json {
        let result = PinMissingJson {
            plugin: plugin_name.to_string(),
            installed: false,
            changed: false,
            reason: "not installed".to_string(),
        };
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        println!(
            "Plugin '{}' is not installed. Install it first with `apm install {}`.",
            plugin_name, plugin_name
        );
    }
    Ok(())
}

fn print_unpinned(package: &InstalledPackageSummary, json: bool) -> Result<()> {
    if json {
        let result = UnpinResultJson {
            unpinned: true,
            plugin: package.slug.clone(),
        };
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        println!(
            "{}",
            format!("Unpinned {} (v{})", package.slug, package.version).green()
        );
    }
    Ok(())
}

fn print_pinned(package: &InstalledPackageSummary, json: bool) -> Result<()> {
    if json {
        let result = PinResultJson {
            pinned: true,
            plugin: package.slug.clone(),
            version: package.version.clone(),
        };
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        println!(
            "{}",
            format!("Pinned {} at v{}", package.slug, package.version).yellow()
        );
    }
    Ok(())
}
