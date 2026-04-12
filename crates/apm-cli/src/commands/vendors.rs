// vendors command — list all vendors in the registry with catalog item counts.

use std::collections::HashMap;

use anyhow::Result;
use colored::Colorize;
use serde::Serialize;

use apm_core::config::Config;
use apm_core::registry::Registry;

#[derive(Serialize)]
struct VendorEntry {
    name: String,
    item_count: usize,
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
            println!("No catalog items found. The registry cache is empty.");
            println!();
            println!("To get started:");
            println!("  apm sync    Download the registry");
            println!("  apm vendors Then list vendors");
        }
        return Ok(());
    }

    // Count catalog items per vendor.
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
                .map(|(name, item_count)| VendorEntry { name, item_count })
                .collect(),
        };
        println!("{}", serde_json::to_string_pretty(&output)?);
        return Ok(());
    }

    // ── Human output ─────────────────────────────────────────────────────────
    let item_suffix = if registry.len() == 1 { "" } else { "s" };
    println!(
        "{}",
        format!(
            "Vendors ({total} total, {} catalog item{item_suffix} in registry):",
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
        let item_word = if *count == 1 { "item" } else { "items" };
        println!("  {:<w_name$}  {:>w_count$} {item_word}", name, count,);
    }

    Ok(())
}
