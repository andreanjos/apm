use anyhow::Result;

use crate::config::Config;
use crate::registry::{self, sync};

pub async fn run(config: &Config) -> Result<()> {
    let sources = config.sources();

    if sources.is_empty() {
        println!("No registry sources configured. Nothing to sync.");
        return Ok(());
    }

    let registries_cache_dir = config.registries_cache_dir();
    let mut any_error = false;

    for source in &sources {
        println!("Syncing registry '{}'...", source.name);

        match sync::sync_source(source, &registries_cache_dir) {
            Ok(()) => {
                // Count how many plugins are now in the cache.
                let source_cache = registries_cache_dir.join(&source.name);
                let count = registry::Registry::load_from_cache(&source_cache)
                    .map(|r| r.len())
                    .unwrap_or(0);

                println!(
                    "Registry '{}' updated. {} plugin{} available.",
                    source.name,
                    count,
                    if count == 1 { "" } else { "s" }
                );
            }
            Err(e) => {
                eprintln!("Failed to sync registry '{}': {e}", source.name);
                any_error = true;
            }
        }
    }

    if any_error {
        anyhow::bail!("One or more registry sources failed to sync.");
    }

    Ok(())
}
