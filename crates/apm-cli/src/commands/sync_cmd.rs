use anyhow::Result;
use colored::Colorize;

use apm_core::config::Config;
use apm_core::registry::{self, sync};

pub async fn run(config: &Config, json: bool, quiet: bool) -> Result<()> {
    let sources = config.sources();

    if sources.is_empty() {
        if json {
            println!("{}", serde_json::json!({ "sources": [] }));
        } else if !quiet {
            println!("No registry sources configured. Nothing to sync.");
        }
        return Ok(());
    }

    let registries_cache_dir = config.registries_cache_dir();
    let mut any_error = false;
    let mut json_results: Vec<serde_json::Value> = Vec::new();

    for source in &sources {
        if !json && !quiet {
            println!("Syncing registry '{}'...", source.name);
        }

        match sync::sync_source(source, &registries_cache_dir) {
            Ok(()) => {
                // Count how many plugins are now in the cache.
                let source_cache = registries_cache_dir.join(&source.name);
                let count = registry::Registry::load_from_cache(&source_cache)
                    .map(|r| r.len())
                    .unwrap_or(0);

                if json {
                    json_results.push(serde_json::json!({
                        "name": source.name,
                        "status": "ok",
                        "plugin_count": count,
                    }));
                } else if !quiet {
                    println!(
                        "{}",
                        format!(
                            "Registry '{}' updated. {} plugin{} available.",
                            source.name,
                            count,
                            if count == 1 { "" } else { "s" }
                        )
                        .green()
                    );
                }
            }
            Err(e) => {
                if json {
                    json_results.push(serde_json::json!({
                        "name": source.name,
                        "status": "error",
                        "error": format!("{e}"),
                    }));
                } else {
                    eprintln!(
                        "{}",
                        format!("Failed to sync registry '{}': {e}", source.name).red()
                    );
                }
                any_error = true;
            }
        }
    }

    if json {
        println!("{}", serde_json::json!({ "sources": json_results }));
    }

    if any_error {
        anyhow::bail!("One or more registry sources failed to sync.");
    }

    Ok(())
}
