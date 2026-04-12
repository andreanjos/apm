// count command — output installed or available standalone plugin counts for scripting.

use anyhow::Result;
use serde::Serialize;

use apm_core::config::Config;
use apm_core::registry::Registry;
use apm_core::state::InstallState;

#[derive(Serialize)]
struct CountJson {
    installed: usize,
    available: usize,
    catalog_items: usize,
}

pub async fn run(config: &Config, json: bool, available: bool) -> Result<()> {
    if json {
        // JSON mode always includes both counts.
        let state = InstallState::load(config)?;
        let registry = Registry::load_all_sources(config).unwrap_or_default();
        let available = registry
            .plugins
            .values()
            .filter(|p| p.is_standalone_plugin())
            .count();
        let output = CountJson {
            installed: state.plugins.len(),
            available,
            catalog_items: registry.len(),
        };
        println!("{}", serde_json::to_string(&output)?);
        return Ok(());
    }

    if available {
        let registry = Registry::load_all_sources(config)?;
        let available_plugins = registry
            .plugins
            .values()
            .filter(|p| p.is_standalone_plugin())
            .count();
        println!("{available_plugins}");
    } else {
        let state = InstallState::load(config)?;
        println!("{}", state.plugins.len());
    }

    Ok(())
}
