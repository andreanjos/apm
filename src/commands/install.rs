// install command — look up plugin in registry, check install state, download,
// verify, extract, place, and record.

use anyhow::Result;

use crate::config::{Config, InstallScope};
use crate::error::ApmError;
use crate::registry::{PluginFormat, Registry};
use crate::state::InstallState;

pub async fn run(
    config: &Config,
    name: &str,
    format: Option<PluginFormat>,
    scope: Option<InstallScope>,
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

    // ── Show install plan ─────────────────────────────────────────────────────

    let formats_to_show: Vec<String> = match format {
        Some(fmt) => vec![fmt.to_string()],
        None => plugin
            .formats
            .keys()
            .map(|f| f.to_string())
            .collect::<Vec<_>>(),
    };

    println!(
        "Installing {} v{} ({})...",
        plugin.name,
        plugin.version,
        formats_to_show.join(", ")
    );

    // ── Install ───────────────────────────────────────────────────────────────

    crate::install::install_plugin(plugin, format, scope, config, &mut state)
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
        "\nInstalled {} v{} to {}",
        plugin.name, plugin.version, install_base
    );

    Ok(())
}
