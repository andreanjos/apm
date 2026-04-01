// rollback command — restore the most recent backup for a plugin.

use anyhow::Result;
use colored::Colorize;

use crate::backup;
use apm_core::config::Config;
use apm_core::state::InstallState;

// ── Entry point ───────────────────────────────────────────────────────────────

pub async fn run(config: &Config, plugin: Option<&str>, list: bool) -> Result<()> {
    if list {
        run_list(config)
    } else if let Some(name) = plugin {
        run_restore(config, name).await
    } else {
        // Neither --list nor a plugin name was given.
        anyhow::bail!("Provide a plugin name to roll back, or use --list to show all backups.");
    }
}

// ── List backups ──────────────────────────────────────────────────────────────

fn run_list(config: &Config) -> Result<()> {
    let entries = backup::list_backups(config)?;

    if entries.is_empty() {
        println!("No backups found.");
        println!("Backups are created automatically during upgrades.");
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
        "Restore a backup with: {}",
        "apm rollback <plugin>".bold()
    );

    Ok(())
}

// ── Restore a backup ──────────────────────────────────────────────────────────

async fn run_restore(config: &Config, slug: &str) -> Result<()> {
    // Check that a backup exists before touching state.
    let entry = backup::find_latest_backup(slug, config)?;

    let entry = match entry {
        Some(e) => e,
        None => {
            println!(
                "No backup found for '{}'.",
                slug.bold()
            );
            println!("Backups are created automatically during upgrades.");
            return Ok(());
        }
    };

    println!(
        "Rolling back {} to v{}...",
        slug.bold(),
        entry.version.cyan()
    );

    let mut state = InstallState::load(config)?;

    backup::restore_plugin(slug, config, &mut state)?;

    println!(
        "{}",
        format!("Rolled back '{}' to v{}.", slug, entry.version).green()
    );

    Ok(())
}
