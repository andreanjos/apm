// install command — look up plugin in registry, check install state, download,
// verify, extract, place, and record.

use std::path::Path;

use anyhow::Result;
use colored::Colorize;

use crate::config::{Config, InstallScope};
use crate::error::ApmError;
use crate::registry::{DownloadType, PluginFormat, Registry};
use crate::state::InstallState;

pub async fn run(
    config: &Config,
    name: &str,
    format: Option<PluginFormat>,
    scope: Option<InstallScope>,
    from_file: Option<&Path>,
) -> Result<()> {
    // ── Load registry ─────────────────────────────────────────────────────────

    let registry = Registry::load_all_sources(config)?;

    if registry.is_empty() {
        anyhow::bail!(
            "Registry cache is empty.\n\
             Hint: Run `apm sync` to populate the local registry cache."
        );
    }

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
            println!(
                "Plugin '{}' is already installed (v{}).",
                plugin.slug, existing.version
            );
            println!("Use `apm upgrade {}` to update.", plugin.slug);
            return Ok(());
        }
    }

    // ── Check for manual download type (when no --from-file provided) ─────────

    if from_file.is_none() {
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

    // ── Show install plan ─────────────────────────────────────────────────────

    let formats_to_show: Vec<String> = match format {
        Some(fmt) => vec![fmt.to_string()],
        None => plugin
            .formats
            .keys()
            .map(|f| f.to_string())
            .collect::<Vec<_>>(),
    };

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

    let install_base = match scope.unwrap_or(config.install_scope) {
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
