// upgrade command — replace installed plugin(s) with newer registry versions.

use anyhow::Result;
use colored::Colorize;
use semver::Version;
use serde::Serialize;

use apm_core::config::Config;
use apm_core::registry::Registry;
use apm_core::state::InstallState;

#[derive(Serialize)]
struct UpgradeResult {
    upgraded: Vec<UpgradeEntry>,
    skipped: Vec<UpgradeEntry>,
    failed: Vec<UpgradeEntry>,
}

#[derive(Serialize)]
struct UpgradeEntry {
    name: String,
    version: String,
}

pub async fn run(
    config: &Config,
    name: Option<&str>,
    dry_run: bool,
    json: bool,
    yes: bool,
) -> Result<()> {
    // ── Load state and registry ───────────────────────────────────────────────

    let mut state = InstallState::load(config)?;

    if state.plugins.is_empty() {
        if json {
            let result = UpgradeResult {
                upgraded: vec![],
                skipped: vec![],
                failed: vec![],
            };
            println!("{}", serde_json::to_string_pretty(&result)?);
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

    // ── Determine which plugins to upgrade ────────────────────────────────────

    // Collect upgrade candidates: (installed_name, registry_plugin).
    struct UpgradeCandidate {
        slug: String,
        installed_version: String,
        available_version: String,
        pinned: bool,
    }

    let candidates: Vec<UpgradeCandidate> = if let Some(target) = name {
        // Single-plugin upgrade.
        let installed = match state.find(target) {
            Some(p) => p.clone(),
            None => {
                if json {
                    let result = UpgradeResult {
                        upgraded: vec![],
                        skipped: vec![],
                        failed: vec![],
                    };
                    println!("{}", serde_json::to_string_pretty(&result)?);
                } else {
                    println!(
                        "Plugin '{}' is not installed via apm. Install it with `apm install {}`.",
                        target, target
                    );
                }
                return Ok(());
            }
        };

        let registry_plugin = match registry.find(target) {
            Some(p) => p,
            None => {
                anyhow::bail!(
                    "Plugin '{}' is not found in any configured registry.\n\
                     Hint: Run `apm sync` to update the registry cache.",
                    target
                );
            }
        };

        let latest_release = registry_plugin.latest_release();
        let is_newer = is_version_newer(&installed.version, &latest_release.version);

        if !is_newer {
            if json {
                let result = UpgradeResult {
                    upgraded: vec![],
                    skipped: vec![],
                    failed: vec![],
                };
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!(
                    "{} v{} is already up to date.",
                    installed.name, installed.version
                );
            }
            return Ok(());
        }

        vec![UpgradeCandidate {
            slug: installed.name.clone(),
            installed_version: installed.version.clone(),
            available_version: latest_release.version,
            pinned: installed.pinned,
        }]
    } else {
        // Bulk upgrade — find all outdated, unpinned plugins.
        state
            .plugins
            .iter()
            .filter_map(|installed| {
                let reg = registry.find(&installed.name)?;
                let latest_release = reg.latest_release();
                if is_version_newer(&installed.version, &latest_release.version) {
                    Some(UpgradeCandidate {
                        slug: installed.name.clone(),
                        installed_version: installed.version.clone(),
                        available_version: latest_release.version,
                        pinned: installed.pinned,
                    })
                } else {
                    None
                }
            })
            .collect()
    };

    if candidates.is_empty() {
        if json {
            let result = UpgradeResult {
                upgraded: vec![],
                skipped: vec![],
                failed: vec![],
            };
            println!("{}", serde_json::to_string_pretty(&result)?);
        } else {
            println!("All plugins are up to date.");
        }
        return Ok(());
    }

    // ── Dry-run: show what would be upgraded ──────────────────────────────────

    if dry_run {
        if !json {
            println!("[dry-run] The following plugins would be upgraded:");
        }
        let mut dry_upgraded = Vec::new();
        let mut dry_skipped = Vec::new();
        for candidate in &candidates {
            if candidate.pinned {
                if !json {
                    println!(
                        "  {} {} -> {} {}",
                        candidate.slug.bold(),
                        candidate.installed_version.cyan(),
                        candidate.available_version.cyan(),
                        "(pinned — would be skipped)".dimmed()
                    );
                }
                dry_skipped.push(UpgradeEntry {
                    name: candidate.slug.clone(),
                    version: candidate.installed_version.clone(),
                });
            } else {
                if !json {
                    println!(
                        "  {} {} -> {}",
                        candidate.slug.bold(),
                        candidate.installed_version.cyan(),
                        candidate.available_version.cyan()
                    );
                }
                dry_upgraded.push(UpgradeEntry {
                    name: candidate.slug.clone(),
                    version: candidate.available_version.clone(),
                });
            }
        }
        if json {
            let result = UpgradeResult {
                upgraded: dry_upgraded,
                skipped: dry_skipped,
                failed: vec![],
            };
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        return Ok(());
    }

    // ── Confirmation prompt (bulk upgrade only) ────────────────────────────────

    if name.is_none() && !json && !yes {
        println!("\nThe following plugins will be upgraded:\n");
        // Compute alignment: find the longest slug among non-pinned candidates.
        let max_name = candidates
            .iter()
            .filter(|c| !c.pinned)
            .map(|c| c.slug.len())
            .max()
            .unwrap_or(0);
        for candidate in &candidates {
            if candidate.pinned {
                continue;
            }
            println!(
                "  {:<width$}  {} -> {}",
                candidate.slug,
                candidate.installed_version,
                candidate.available_version,
                width = max_name,
            );
        }
        let upgradable = candidates.iter().filter(|c| !c.pinned).count();
        let pinned = candidates.iter().filter(|c| c.pinned).count();
        if pinned > 0 {
            print!(
                "\n{} plugin(s) to upgrade ({} pinned, skipped). ",
                upgradable, pinned
            );
        } else {
            print!("\n{} plugin(s) to upgrade. ", upgradable);
        }
        print!("Proceed? [Y/n] ");
        std::io::Write::flush(&mut std::io::stdout())?;
        let mut input_buf = String::new();
        std::io::stdin()
            .read_line(&mut input_buf)
            .map_err(|e| anyhow::anyhow!("Cannot read user input: {e}"))?;
        let answer = input_buf.trim();
        if !answer.is_empty() && !answer.eq_ignore_ascii_case("y") {
            println!("Aborted.");
            return Ok(());
        }
    }

    // ── Process each candidate ────────────────────────────────────────────────

    let mut upgraded_count = 0usize;
    let mut failed_count = 0usize;
    let mut upgraded_entries = Vec::new();
    let mut skipped_entries = Vec::new();
    let mut failed_entries = Vec::new();

    for candidate in &candidates {
        // Handle pinned plugins.
        if candidate.pinned {
            if name.is_some() {
                // Specific plugin requested — error out clearly.
                if !json {
                    println!(
                        "Plugin '{}' is pinned at v{}. Use 'apm pin --unpin {}' to unpin it first.",
                        candidate.slug, candidate.installed_version, candidate.slug
                    );
                }
                skipped_entries.push(UpgradeEntry {
                    name: candidate.slug.clone(),
                    version: candidate.installed_version.clone(),
                });
                if json {
                    let result = UpgradeResult {
                        upgraded: upgraded_entries,
                        skipped: skipped_entries,
                        failed: failed_entries,
                    };
                    println!("{}", serde_json::to_string_pretty(&result)?);
                }
                return Ok(());
            } else {
                // Bulk upgrade — skip pinned plugins with a message.
                if !json {
                    println!(
                        "Skipping {} (pinned at v{})",
                        candidate.slug, candidate.installed_version
                    );
                }
                skipped_entries.push(UpgradeEntry {
                    name: candidate.slug.clone(),
                    version: candidate.installed_version.clone(),
                });
                continue;
            }
        }

        // Look up the full registry definition.
        let registry_plugin = match registry.find(&candidate.slug) {
            Some(p) => p,
            None => {
                tracing::warn!(
                    "Plugin '{}' not found in registry, skipping",
                    candidate.slug
                );
                failed_count += 1;
                failed_entries.push(UpgradeEntry {
                    name: candidate.slug.clone(),
                    version: candidate.available_version.clone(),
                });
                continue;
            }
        };

        let latest_release = registry_plugin.latest_release();
        let mut selected_plugin = registry_plugin.clone();
        selected_plugin.version = latest_release.version;
        selected_plugin.formats = latest_release.formats;

        if !json {
            println!(
                "Upgrading {} {} -> {}...",
                candidate.slug.bold(),
                candidate.installed_version.cyan(),
                candidate.available_version.cyan()
            );
        }

        // Back up the current version before overwriting.
        let installed = match state.find(&candidate.slug).cloned() {
            Some(p) => p,
            None => {
                tracing::warn!("Plugin '{}' not in install state, skipping", candidate.slug);
                failed_count += 1;
                failed_entries.push(UpgradeEntry {
                    name: candidate.slug.clone(),
                    version: candidate.available_version.clone(),
                });
                continue;
            }
        };
        match crate::backup::backup_plugin(&installed, config) {
            Ok(entry) => {
                if !json {
                    println!(
                        "  {} v{} backed up to {}",
                        candidate.slug,
                        candidate.installed_version,
                        entry.backup_dir.display()
                    );
                }
            }
            Err(e) => {
                if !json {
                    eprintln!(
                        "  {} Could not back up '{}' before upgrade: {e} (continuing anyway)",
                        "Warning:".yellow(),
                        candidate.slug
                    );
                }
            }
        }

        // Remove old bundles from disk.
        for fmt in &installed.formats {
            let path = &fmt.path;
            if path.exists() {
                std::fs::remove_dir_all(path).map_err(|e| {
                    anyhow::anyhow!(
                        "Failed to remove old {} bundle at {}: {}",
                        fmt.format,
                        path.display(),
                        e
                    )
                })?;
            } else if !json {
                eprintln!(
                    "{} old {} bundle not found at {} (already removed?)",
                    "Warning:".yellow(),
                    fmt.format,
                    path.display()
                );
            }
        }

        // Remove the old state entry so install_plugin doesn't see it as "already installed"
        // and records cleanly.
        state.remove(&candidate.slug);

        // Install the new version (this records it in state and saves).
        crate::install::install_plugin(&selected_plugin, None, None, config, &mut state, None)
            .await
            .map_err(|e| e.context(format!("Failed to upgrade '{}'", candidate.slug)))?;

        if !json {
            println!(
                "{}",
                format!(
                    "Upgraded {} to v{}",
                    candidate.slug, candidate.available_version
                )
                .green()
            );
        }
        upgraded_count += 1;
        upgraded_entries.push(UpgradeEntry {
            name: candidate.slug.clone(),
            version: candidate.available_version.clone(),
        });
    }

    // ── Summary ───────────────────────────────────────────────────────────────

    if json {
        let result = UpgradeResult {
            upgraded: upgraded_entries,
            skipped: skipped_entries,
            failed: failed_entries,
        };
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else if upgraded_count == 0 && failed_count == 0 {
        println!("{}", "Nothing was upgraded.".dimmed());
    } else if failed_count > 0 {
        println!(
            "\n{}",
            format!(
                "Upgraded {} plugin(s), {} failed.",
                upgraded_count, failed_count
            )
            .yellow()
        );
    } else {
        println!(
            "\n{}",
            format!("Upgraded {} plugin(s).", upgraded_count).green(),
        );
    }

    Ok(())
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn is_version_newer(installed: &str, candidate: &str) -> bool {
    match (Version::parse(installed), Version::parse(candidate)) {
        (Ok(inst_v), Ok(cand_v)) => cand_v > inst_v,
        _ => candidate != installed,
    }
}
