use anyhow::Result;
use colored::Colorize;

use apm_core::config::Config;
use apm_core::engine::{ApmEngine, EngineEvent, RegistrySyncSourceResult};

pub async fn run(config: &Config, json: bool, quiet: bool) -> Result<()> {
    let engine = ApmEngine::new(config.clone());
    let mut sink = |event| {
        if let EngineEvent::RegistrySourceSyncStarted { source } = event {
            if !json && !quiet {
                println!("Syncing registry '{source}'...");
            }
        }
    };
    let result = engine.sync_registries(&mut sink)?;

    if json {
        let json_results: Vec<serde_json::Value> =
            result.sources.iter().map(sync_source_json).collect();
        println!("{}", serde_json::json!({ "sources": json_results }));
    } else {
        for source in &result.sources {
            print_source_result(source, quiet);
        }
    }

    if result.has_errors() {
        anyhow::bail!("One or more registry sources failed to sync.");
    }

    Ok(())
}

fn sync_source_json(source: &RegistrySyncSourceResult) -> serde_json::Value {
    match source {
        RegistrySyncSourceResult::Ok {
            name,
            catalog_item_count,
            installable_product_count,
        } => serde_json::json!({
            "name": name,
            "status": "ok",
            "installable_product_count": installable_product_count,
            "catalog_item_count": catalog_item_count,
        }),
        RegistrySyncSourceResult::Error { name, error } => serde_json::json!({
            "name": name,
            "status": "error",
            "error": error,
        }),
    }
}

fn print_source_result(source: &RegistrySyncSourceResult, quiet: bool) {
    match source {
        RegistrySyncSourceResult::Ok {
            name,
            catalog_item_count,
            installable_product_count,
        } => {
            if quiet {
                return;
            }
            println!(
                "{}",
                format!(
                    "Registry '{}' updated. {} installable product{} ({} catalog item{}) available.",
                    name,
                    installable_product_count,
                    if *installable_product_count == 1 { "" } else { "s" },
                    catalog_item_count,
                    if *catalog_item_count == 1 { "" } else { "s" },
                )
                .green()
            );
        }
        RegistrySyncSourceResult::Error { name, error } => {
            eprintln!(
                "{}",
                format!("Failed to sync registry '{name}': {error}").red()
            );
        }
    }
}
