// remove command — delete plugin bundle(s) from disk and remove from state.

use anyhow::Result;
use colored::Colorize;
use serde_json;

use apm_core::config::Config;
use apm_core::state::{InstallOrigin, InstallState};

pub async fn run(config: &Config, name: &str, json: bool, dry_run: bool) -> Result<()> {
    // ── Load state ────────────────────────────────────────────────────────────

    let mut state = InstallState::load(config)?;

    // ── Look up the plugin ────────────────────────────────────────────────────

    let plugin = match state.find(name) {
        Some(p) => p.clone(),
        None => {
            if json {
                println!(
                    "{{\"removed\":false,\"plugin\":{},\"reason\":\"not installed\"}}",
                    serde_json::json!(name)
                );
            } else {
                println!(
                    "Plugin '{}' is not installed via apm. Nothing to remove.",
                    name
                );
            }
            return Ok(());
        }
    };

    if plugin.origin == InstallOrigin::External {
        let all_paths_missing = plugin.formats.iter().all(|format| !format.path.exists());

        if all_paths_missing {
            if dry_run {
                if json {
                    println!(
                        "{}",
                        serde_json::json!({
                            "dry_run": true,
                            "would_remove": true,
                            "plugin": plugin.name,
                            "reason": "stale external state entry",
                        })
                    );
                } else {
                    println!(
                        "[dry-run] Would remove stale external state entry for {}.",
                        plugin.name.bold()
                    );
                    println!("          No external plugin files would be deleted.");
                }
                return Ok(());
            }

            state.remove(&plugin.name);
            state.save(config)?;

            if json {
                println!(
                    "{}",
                    serde_json::json!({
                        "removed": true,
                        "plugin": plugin.name,
                        "reason": "stale external state entry",
                        "formats_removed": [],
                    })
                );
            } else {
                println!(
                    "{}",
                    format!(
                        "Removed stale external state entry for {}. No plugin files were deleted.",
                        plugin.name
                    )
                    .green()
                );
            }
            return Ok(());
        }

        if json {
            println!(
                "{}",
                serde_json::json!({
                    "removed": false,
                    "plugin": plugin.name,
                    "reason": "external install still exists",
                })
            );
        } else {
            println!(
                "{} was discovered by `apm scan`; apm will not delete externally installed files.",
                plugin.name.bold()
            );
            println!(
                "Remove it with the vendor installer or Finder first, then run `apm remove {}` to clean apm state.",
                plugin.name
            );
        }
        return Ok(());
    }

    // ── Dry-run: show what would be removed and exit ─────────────────────────

    if dry_run {
        if json {
            let format_entries: Vec<serde_json::Value> = plugin
                .formats
                .iter()
                .map(|f| {
                    serde_json::json!({
                        "format": f.format.to_string(),
                        "path": f.path.display().to_string(),
                    })
                })
                .collect();
            println!(
                "{}",
                serde_json::json!({
                    "dry_run": true,
                    "would_remove": true,
                    "plugin": plugin.name,
                    "version": plugin.version,
                    "formats": format_entries,
                })
            );
        } else {
            println!(
                "[dry-run] Would remove {} v{}",
                plugin.name.bold(),
                plugin.version.cyan()
            );
            let format_details: Vec<String> = plugin
                .formats
                .iter()
                .map(|f| format!("{} ({})", f.format, f.path.display()))
                .collect();
            println!("          Formats: {}", format_details.join(", "));
        }
        return Ok(());
    }

    // ── Show what will be removed ─────────────────────────────────────────────

    let format_names: Vec<String> = plugin
        .formats
        .iter()
        .map(|f| f.format.to_string())
        .collect();

    if !json {
        println!(
            "Removing {} v{}...",
            plugin.name.bold(),
            plugin.version.cyan()
        );
    }

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
        } else if !json {
            eprintln!(
                "{} {} bundle not found at {} (already removed?)",
                "Warning:".yellow(),
                fmt.format,
                path.display()
            );
        }
    }

    // ── Remove from state and save ────────────────────────────────────────────

    state.remove(&plugin.name);
    state.save(config)?;

    // ── Success message ───────────────────────────────────────────────────────

    if json {
        println!(
            "{}",
            serde_json::json!({
                "removed": true,
                "plugin": plugin.name,
                "formats_removed": format_names,
            })
        );
    } else {
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
    }

    Ok(())
}
