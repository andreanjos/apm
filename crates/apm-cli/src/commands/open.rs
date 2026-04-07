use anyhow::Result;

use apm_core::config::Config;
use apm_core::registry;

pub async fn run(config: &Config, name: &str) -> Result<()> {
    let registry = registry::Registry::load_all_sources(config)?;

    if registry.is_empty() {
        println!("Registry cache is empty. Run `apm sync` to download the plugin registry.");
        return Ok(());
    }

    let plugin = match registry.find(name) {
        Some(p) => p,
        None => {
            println!(
                "Plugin '{name}' not found. Try `apm search {name}` to find the correct name."
            );
            return Ok(());
        }
    };

    match &plugin.homepage {
        Some(homepage) => {
            println!("Opening {homepage} in browser...");
            std::process::Command::new("open")
                .arg(homepage)
                .spawn()
                .map_err(|e| anyhow::anyhow!("Failed to open browser: {e}"))?;
            Ok(())
        }
        None => {
            println!(
                "No homepage listed for {}. Try `apm info {}` for details.",
                plugin.slug, plugin.slug
            );
            Ok(())
        }
    }
}
