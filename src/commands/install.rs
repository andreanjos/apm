// install command — look up plugin(s) in registry, check install state,
// download, verify, extract, place, and record.
//
// Supports batch installs: `apm install vital surge-xt dexed`

use std::path::Path;

use anyhow::Result;
use colored::Colorize;

use crate::config::{Config, InstallScope};
use crate::error::ApmError;
use crate::registry::{DownloadType, PluginFormat, Registry};
use crate::state::InstallState;

pub async fn run(
    config: &Config,
    plugins: &[String],
    format: Option<PluginFormat>,
    scope: Option<InstallScope>,
    from_file: Option<&Path>,
    dry_run: bool,
    bundle: Option<&str>,
) -> Result<()> {
    // ── Validate --from-file with multiple plugins ────────────────────────────

    if from_file.is_some() && plugins.len() > 1 {
        anyhow::bail!(
            "--from-file can only be used when installing a single plugin.\n\
             Hint: Remove extra plugin names or omit --from-file."
        );
    }

    // ── Load registry ─────────────────────────────────────────────────────────

    let registry = Registry::load_all_sources(config)?;

    if registry.is_empty() {
        anyhow::bail!(
            "Registry cache is empty.\n\
             Hint: Run `apm sync` to populate the local registry cache."
        );
    }

    // ── Bundle resolution ─────────────────────────────────────────────────────

    if let Some(bundle_slug) = bundle {
        let b = registry.find_bundle(bundle_slug).ok_or_else(|| {
            anyhow::anyhow!(
                "Bundle '{}' not found. Use `apm bundles` to list available bundles.",
                bundle_slug
            )
        })?;

        println!(
            "Installing bundle '{}' ({} plugins)...",
            b.name,
            b.plugins.len()
        );

        let bundle_plugins: Vec<String> = b.plugins.clone();
        return Box::pin(run(config, &bundle_plugins, format, scope, None, dry_run, None)).await;
    }

    // ── Single-plugin fast path (original behaviour) ──────────────────────────

    if plugins.len() == 1 {
        let name = &plugins[0];
        return run_single(config, name, &registry, format, scope, from_file, dry_run).await;
    }

    // ── Batch install ─────────────────────────────────────────────────────────

    let mut succeeded: Vec<String> = Vec::new();
    let mut failed: Vec<(String, String)> = Vec::new(); // (name, reason)

    for name in plugins {
        match run_single(config, name, &registry, format, scope, None, dry_run).await {
            Ok(()) => {
                succeeded.push(name.clone());
            }
            Err(e) => {
                // Print the per-plugin failure immediately so the user can see it
                // as it happens (other plugins still continue).
                eprintln!("  {} {}: {}", "FAILED".red().bold(), name, e);
                failed.push((name.clone(), e.to_string()));
            }
        }
    }

    // ── Summary ───────────────────────────────────────────────────────────────

    if dry_run {
        return Ok(());
    }

    let total = plugins.len();
    let n_ok = succeeded.len();
    let n_fail = failed.len();

    println!();
    if n_fail == 0 {
        println!(
            "{}",
            format!("Installed {n_ok}/{total} plugins.").green()
        );
    } else {
        let failed_names: Vec<String> = failed
            .iter()
            .map(|(name, reason)| {
                // Produce a short reason by taking the first line.
                let short = reason.lines().next().unwrap_or(reason.as_str());
                format!("{name} — {short}")
            })
            .collect();

        println!(
            "{}",
            format!(
                "Installed {n_ok}/{total} plugins ({n_fail} failed: {})",
                failed_names.join(", ")
            )
            .yellow()
        );
    }

    Ok(())
}

// ── Single-plugin installation ────────────────────────────────────────────────

async fn run_single(
    config: &Config,
    name: &str,
    registry: &Registry,
    format: Option<PluginFormat>,
    scope: Option<InstallScope>,
    from_file: Option<&Path>,
    dry_run: bool,
) -> Result<()> {
    // ── Look up the plugin ────────────────────────────────────────────────────

    let plugin = registry.find(name).ok_or_else(|| ApmError::PluginNotFound {
        name: name.to_owned(),
    })?;

    // ── Check if already installed ────────────────────────────────────────────

    let mut state = InstallState::load(config)?;

    if let Some(existing) = state.find(&plugin.slug) {
        // If the user requested a specific format, check if it's already there.
        let already_has_format = match format {
            Some(fmt) => existing.formats.iter().any(|f| f.format == fmt),
            None => !existing.formats.is_empty(),
        };

        if already_has_format {
            if dry_run {
                println!(
                    "[dry-run] '{}' is already installed (v{}). Nothing to do.",
                    plugin.slug, existing.version
                );
            } else {
                println!(
                    "Plugin '{}' is already installed (v{}).",
                    plugin.slug, existing.version
                );
                println!("Use `apm upgrade {}` to update.", plugin.slug);
            }
            return Ok(());
        }
    }

    // ── Check for manual download type (when no --from-file provided) ─────────

    if from_file.is_none() && !dry_run {
        // Check whether any of the formats we'd install are manual.
        let formats_to_check: Vec<_> = match format {
            Some(fmt) => {
                if let Some(src) = plugin.formats.get(&fmt) {
                    vec![(fmt, src)]
                } else {
                    vec![]
                }
            }
            None => plugin.formats.iter().map(|(&f, s)| (f, s)).collect(),
        };

        let is_manual = formats_to_check
            .iter()
            .any(|(_, src)| src.download_type == DownloadType::Manual);

        if is_manual {
            let homepage = plugin
                .homepage
                .as_deref()
                .unwrap_or("(no homepage listed)");

            println!(
                "{} requires manual download (account signup needed).\n",
                plugin.name.bold()
            );
            println!("1. Download the installer from: {}", homepage.cyan());
            println!("   (Opening in your browser...)\n");
            println!(
                "2. Once downloaded, run:\n   {}",
                format!("apm install {} --from-file ~/Downloads/<installer>", plugin.slug).bold()
            );

            // Try to open the homepage in the default browser (macOS `open`).
            let _ = std::process::Command::new("open").arg(homepage).spawn();

            return Ok(());
        }
    }

    // ── Determine formats and install paths ───────────────────────────────────

    let effective_scope = scope.unwrap_or(config.install_scope);

    let mut formats_to_install: Vec<(PluginFormat, &crate::registry::FormatSource)> = match format {
        Some(fmt) => {
            if let Some(src) = plugin.formats.get(&fmt) {
                vec![(fmt, src)]
            } else {
                vec![]
            }
        }
        None => plugin.formats.iter().map(|(&f, s)| (f, s)).collect(),
    };
    formats_to_install.sort_by_key(|(f, _)| f.to_string());

    let formats_to_show: Vec<String> = formats_to_install
        .iter()
        .map(|(f, _)| f.to_string())
        .collect();

    // ── Dry-run output ────────────────────────────────────────────────────────

    if dry_run {
        let install_base = match effective_scope {
            InstallScope::User => "~/Library/Audio/Plug-Ins/",
            InstallScope::System => "/Library/Audio/Plug-Ins/",
        };

        println!(
            "[dry-run] Would install {} v{} ({})",
            plugin.name.bold(),
            plugin.version.cyan(),
            formats_to_show.join(", ")
        );
        println!("          Destination: {}", install_base.yellow());

        for (fmt, src) in &formats_to_install {
            let dl_type = match src.download_type {
                DownloadType::Direct => "direct download",
                DownloadType::Manual => "manual download required",
            };
            println!(
                "          {}: {} ({})",
                fmt.to_string().cyan(),
                src.url,
                dl_type
            );
        }
        return Ok(());
    }

    // ── Show install plan ─────────────────────────────────────────────────────

    if let Some(path) = from_file {
        println!(
            "Installing {} v{} ({}) from file {}...",
            plugin.name.bold(),
            plugin.version.cyan(),
            formats_to_show.join(", "),
            path.display().to_string().yellow()
        );
    } else {
        println!(
            "Installing {} v{} ({})...",
            plugin.name.bold(),
            plugin.version.cyan(),
            formats_to_show.join(", ")
        );
    }

    // ── Install ───────────────────────────────────────────────────────────────

    crate::install::install_plugin(plugin, format, scope, config, &mut state, from_file)
        .await
        .map_err(|e| {
            // Wrap with top-level context so the error shows the plugin name.
            e.context(format!("Failed to install '{}'", plugin.slug))
        })?;

    // ── Success message ───────────────────────────────────────────────────────

    let install_base = match effective_scope {
        InstallScope::User => "~/Library/Audio/Plug-Ins/",
        InstallScope::System => "/Library/Audio/Plug-Ins/",
    };

    println!(
        "\n{}",
        format!(
            "Installed {} v{} to {}",
            plugin.name, plugin.version, install_base
        )
        .green()
    );

    Ok(())
}
