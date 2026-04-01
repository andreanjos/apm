// upgrade command — replace installed plugin(s) with newer registry versions.

use anyhow::Result;
use colored::Colorize;
use semver::Version;

use crate::config::Config;
use crate::registry::Registry;
use crate::state::InstallState;

pub async fn run(config: &Config, name: Option<&str>, dry_run: bool) -> Result<()> {
    // ── Load state and registry ───────────────────────────────────────────────

    let mut state = InstallState::load(config)?;

    if state.plugins.is_empty() {
        println!("No plugins installed via apm.");
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
                println!(
                    "Plugin '{}' is not installed via apm. Install it with `apm install {}`.",
                    target, target
                );
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

        let is_newer = is_version_newer(&installed.version, &registry_plugin.version);

        if !is_newer {
            println!(
                "{} v{} is already up to date.",
                installed.name, installed.version
            );
            return Ok(());
        }

        vec![UpgradeCandidate {
            slug: installed.name.clone(),
            installed_version: installed.version.clone(),
            available_version: registry_plugin.version.clone(),
            pinned: installed.pinned,
        }]
    } else {
        // Bulk upgrade — find all outdated, unpinned plugins.
        state
            .plugins
            .iter()
            .filter_map(|installed| {
                let reg = registry.find(&installed.name)?;
                if is_version_newer(&installed.version, &reg.version) {
                    Some(UpgradeCandidate {
                        slug: installed.name.clone(),
                        installed_version: installed.version.clone(),
                        available_version: reg.version.clone(),
                        pinned: installed.pinned,
                    })
                } else {
                    None
                }
            })
            .collect()
    };

    if candidates.is_empty() {
        println!("All plugins are up to date.");
        return Ok(());
    }

    // ── Dry-run: show what would be upgraded ──────────────────────────────────

    if dry_run {
        println!("[dry-run] The following plugins would be upgraded:");
        for candidate in &candidates {
            if candidate.pinned {
                println!(
                    "  {} {} -> {} {}",
                    candidate.slug.bold(),
                    candidate.installed_version.cyan(),
                    candidate.available_version.cyan(),
                    "(pinned — would be skipped)".dimmed()
                );
            } else {
                println!(
                    "  {} {} -> {}",
                    candidate.slug.bold(),
                    candidate.installed_version.cyan(),
                    candidate.available_version.cyan()
                );
            }
        }
        return Ok(());
    }

    // ── Process each candidate ────────────────────────────────────────────────

    let mut upgraded = 0usize;

    for candidate in &candidates {
        // Handle pinned plugins.
        if candidate.pinned {
            if name.is_some() {
                // Specific plugin requested — error out clearly.
                println!(
                    "Plugin '{}' is pinned at v{}. Use 'apm pin --unpin {}' to unpin it first.",
                    candidate.slug, candidate.installed_version, candidate.slug
                );
                return Ok(());
            } else {
                // Bulk upgrade — skip pinned plugins with a message.
                println!(
                    "Skipping {} (pinned at v{})",
                    candidate.slug, candidate.installed_version
                );
                continue;
            }
        }

        // Look up the full registry definition.
        let registry_plugin = registry.find(&candidate.slug).expect("already checked above");

        println!(
            "Upgrading {} {} -> {}...",
            candidate.slug.bold(),
            candidate.installed_version.cyan(),
            candidate.available_version.cyan()
        );

        // Back up the current version before overwriting.
        let installed = state.find(&candidate.slug).cloned().unwrap();
        match crate::backup::backup_plugin(&installed, config) {
            Ok(entry) => {
                println!(
                    "  {} v{} backed up to {}",
                    candidate.slug,
                    candidate.installed_version,
                    entry.backup_dir.display()
                );
            }
            Err(e) => {
                eprintln!(
                    "  {} Could not back up '{}' before upgrade: {e} (continuing anyway)",
                    "Warning:".yellow(),
                    candidate.slug
                );
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
            } else {
                eprintln!(
                    "Warning: old {} bundle not found at {} (already removed?)",
                    fmt.format,
                    path.display()
                );
            }
        }

        // Remove the old state entry so install_plugin doesn't see it as "already installed"
        // and records cleanly.
        state.remove(&candidate.slug);

        // Install the new version (this records it in state and saves).
        crate::install::install_plugin(registry_plugin, None, None, config, &mut state, None)
            .await
            .map_err(|e| e.context(format!("Failed to upgrade '{}'", candidate.slug)))?;

        println!(
            "{}",
            format!("Upgraded {} to v{}", candidate.slug, candidate.available_version).green()
        );
        upgraded += 1;
    }

    // ── Summary ───────────────────────────────────────────────────────────────

    if upgraded == 0 {
        println!("{}", "Nothing was upgraded.".dimmed());
    } else {
        println!("\n{}", format!("Upgraded {} plugin(s).", upgraded).green());
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
