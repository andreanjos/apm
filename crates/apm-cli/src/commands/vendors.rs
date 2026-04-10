// vendors command — list all plugin vendors in the registry with plugin counts.

use std::collections::HashMap;

use anyhow::Result;
use colored::Colorize;
use serde::Serialize;

use apm_core::config::Config;
use apm_core::registry::Registry;

#[derive(Serialize)]
struct VendorEntry {
    name: String,
    plugin_count: usize,
}

#[derive(Serialize)]
struct VendorsJson {
    vendors: Vec<VendorEntry>,
}

pub async fn run(config: &Config, json: bool) -> Result<()> {
    let registry = Registry::load_all_sources(config)?;

    if registry.is_empty() {
        if json {
            println!(
                "{}",
                serde_json::to_string_pretty(&VendorsJson { vendors: vec![] })?
            );
        } else {
            println!("No plugins found. The registry cache is empty.");
            println!();
            println!("To get started:");
            println!("  apm sync    Download the plugin registry");
            println!("  apm vendors Then list vendors");
        }
        return Ok(());
    }

    // Count plugins per vendor.
    let mut vendor_counts: HashMap<String, usize> = HashMap::new();
    for plugin in registry.plugins.values() {
        *vendor_counts.entry(plugin.vendor.clone()).or_insert(0) += 1;
    }

    // Sort by count descending, then by name ascending for ties.
    let mut vendors: Vec<(String, usize)> = vendor_counts.into_iter().collect();
    vendors.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));

    let total = vendors.len();

    // ── JSON output ──────────────────────────────────────────────────────────
    if json {
        let output = VendorsJson {
            vendors: vendors
                .into_iter()
                .map(|(name, plugin_count)| VendorEntry { name, plugin_count })
                .collect(),
        };
        println!("{}", serde_json::to_string_pretty(&output)?);
        return Ok(());
    }

    // ── Human output ─────────────────────────────────────────────────────────
    let suffix = if total == 1 { "" } else { "s" };
    println!(
        "{}",
        format!(
            "Vendors ({total} total, {} plugin{suffix} in registry):",
            registry.len()
        )
        .bold()
    );
    println!();

    // Compute column widths.
    let w_name = vendors
        .iter()
        .map(|(name, _)| name.len())
        .max()
        .unwrap_or(0);
    let w_count = vendors
        .iter()
        .map(|(_, count)| count.to_string().len())
        .max()
        .unwrap_or(0);

    for (name, count) in &vendors {
        let plugin_word = if *count == 1 { "plugin" } else { "plugins" };
        println!("  {:<w_name$}  {:>w_count$} {plugin_word}", name, count,);
    }

    Ok(())
}
