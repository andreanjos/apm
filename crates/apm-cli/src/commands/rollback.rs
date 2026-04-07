// rollback command — restore the most recent backup for a plugin.

use anyhow::Result;
use colored::Colorize;
use serde::Serialize;

use crate::backup;
use apm_core::config::Config;
use apm_core::state::InstallState;

#[derive(Serialize)]
struct BackupJson {
    plugin: String,
    version: String,
    size_bytes: u64,
    date: String,
}

#[derive(Serialize)]
struct BackupListJson {
    backups: Vec<BackupJson>,
}

#[derive(Serialize)]
struct RollbackResultJson {
    rolled_back: bool,
    plugin: String,
    version: String,
}

// ── Entry point ───────────────────────────────────────────────────────────────

pub async fn run(config: &Config, plugin: Option<&str>, list: bool, json: bool) -> Result<()> {
    if list {
        run_list(config, json)
    } else if let Some(name) = plugin {
        run_restore(config, name, json).await
    } else {
        // Neither --list nor a plugin name was given.
        anyhow::bail!("Provide a plugin name to roll back, or use --list to show all backups.");
    }
}

// ── List backups ──────────────────────────────────────────────────────────────

fn run_list(config: &Config, json: bool) -> Result<()> {
    let entries = backup::list_backups(config)?;

    if entries.is_empty() {
        if json {
            let result = BackupListJson { backups: vec![] };
            println!("{}", serde_json::to_string_pretty(&result)?);
        } else {
            println!("No backups found.");
            println!("Backups are local snapshots created automatically during upgrades.");
            println!(
                "`apm rollback` restores the latest local backup, not an arbitrary registry version."
            );
        }
        return Ok(());
    }

    if json {
        let backups: Vec<BackupJson> = entries
            .iter()
            .map(|entry| BackupJson {
                plugin: entry.slug.clone(),
                version: entry.version.clone(),
                size_bytes: entry.size_bytes(),
                date: entry.created_at.format("%Y-%m-%dT%H:%M:%SZ").to_string(),
            })
            .collect();
        let result = BackupListJson { backups };
        println!("{}", serde_json::to_string_pretty(&result)?);
        return Ok(());
    }

    println!("{}", "Available backups:".bold());
    println!();

    for entry in &entries {
        let size_mb = entry.size_bytes() as f64 / 1_048_576.0;
        println!(
            "  {} v{}  [{}]  {:.1} MB",
            entry.slug.cyan().bold(),
            entry.version,
            entry.created_at.format("%Y-%m-%d %H:%M UTC"),
            size_mb
        );
    }

    println!();
    println!(
        "Restore the latest local backup with: {}",
        "apm rollback <plugin>".bold()
    );

    Ok(())
}

// ── Restore a backup ──────────────────────────────────────────────────────────

async fn run_restore(config: &Config, slug: &str, json: bool) -> Result<()> {
    // Check that a backup exists before touching state.
    let entry = backup::find_latest_backup(slug, config)?;

    let entry = match entry {
        Some(e) => e,
        None => {
            if json {
                let result = RollbackResultJson {
                    rolled_back: false,
                    plugin: slug.to_owned(),
                    version: String::new(),
                };
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("No backup found for '{}'.", slug.bold());
                println!("Backups are local snapshots created automatically during upgrades.");
                println!("Use `apm install <plugin> --version <x.y.z>` for registry-backed historical installs.");
            }
            return Ok(());
        }
    };

    if !json {
        println!(
            "Rolling back {} to v{}...",
            slug.bold(),
            entry.version.cyan()
        );
    }

    let restored_version = entry.version.clone();

    let mut state = InstallState::load(config)?;

    backup::restore_plugin(slug, config, &mut state)?;

    if json {
        let result = RollbackResultJson {
            rolled_back: true,
            plugin: slug.to_owned(),
            version: restored_version,
        };
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        println!(
            "{}",
            format!("Rolled back '{}' to v{}.", slug, restored_version).green()
        );
    }

    Ok(())
}
