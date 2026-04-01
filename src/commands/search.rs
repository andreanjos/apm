use anyhow::Result;
use serde::Serialize;

use crate::config::Config;
use crate::registry::{self, search};

/// JSON-serializable view of a search result.
#[derive(Serialize)]
struct SearchResultJson {
    slug: String,
    name: String,
    vendor: String,
    version: String,
    category: String,
    subcategory: Option<String>,
    license: String,
    description: String,
    tags: Vec<String>,
}

pub async fn run(config: &Config, query: &str, category: Option<&str>, vendor: Option<&str>, json: bool) -> Result<()> {
    let registry = registry::Registry::load_all_sources(config)?;

    if registry.is_empty() {
        if json {
            println!("[]");
        } else {
            println!(
                "Registry cache is empty. Run `apm sync` to download the plugin registry."
            );
        }
        return Ok(());
    }

    let results = search::search(&registry, query, category, vendor);

    if results.is_empty() {
        if json {
            println!("[]");
            return Ok(());
        }
        let mut filter_msg = String::new();
        if let Some(c) = category {
            filter_msg.push_str(&format!(" in category '{c}'"));
        }
        if let Some(v) = vendor {
            filter_msg.push_str(&format!(" by vendor '{v}'"));
        }
        if query.is_empty() {
            println!("No plugins found{filter_msg}.");
        } else {
            println!("No plugins found matching '{query}'{filter_msg}.");
        }
        return Ok(());
    }

    // ── JSON output ───────────────────────────────────────────────────────────
    if json {
        let json_results: Vec<SearchResultJson> = results
            .iter()
            .map(|p| SearchResultJson {
                slug: p.slug.clone(),
                name: p.name.clone(),
                vendor: p.vendor.clone(),
                version: p.version.clone(),
                category: p.category.clone(),
                subcategory: p.subcategory.clone(),
                license: p.license.clone(),
                description: p.description.clone(),
                tags: p.tags.clone(),
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&json_results)?);
        return Ok(());
    }

    // ── Column widths ─────────────────────────────────────────────────────────

    const HDR_NAME: &str = "Name";
    const HDR_VENDOR: &str = "Vendor";
    const HDR_VER: &str = "Version";
    const HDR_CAT: &str = "Category";
    const HDR_LIC: &str = "License";

    let w_name = results
        .iter()
        .map(|p| p.slug.len())
        .max()
        .unwrap_or(0)
        .max(HDR_NAME.len());

    let w_vendor = results
        .iter()
        .map(|p| p.vendor.len())
        .max()
        .unwrap_or(0)
        .max(HDR_VENDOR.len());

    let w_ver = results
        .iter()
        .map(|p| p.version.len())
        .max()
        .unwrap_or(0)
        .max(HDR_VER.len());

    let w_cat = results
        .iter()
        .map(|p| category_display(p).len())
        .max()
        .unwrap_or(0)
        .max(HDR_CAT.len());

    let w_lic = results
        .iter()
        .map(|p| p.license.len())
        .max()
        .unwrap_or(0)
        .max(HDR_LIC.len());

    // ── Header ────────────────────────────────────────────────────────────────
    println!(
        "{:<w_name$}  {:<w_vendor$}  {:<w_ver$}  {:<w_cat$}  {}",
        HDR_NAME, HDR_VENDOR, HDR_VER, HDR_CAT, HDR_LIC,
    );

    let rule_len = w_name + 2 + w_vendor + 2 + w_ver + 2 + w_cat + 2 + w_lic;
    println!("{}", "\u{2500}".repeat(rule_len));

    // ── Rows ──────────────────────────────────────────────────────────────────
    for p in &results {
        println!(
            "{:<w_name$}  {:<w_vendor$}  {:<w_ver$}  {:<w_cat$}  {}",
            p.slug,
            p.vendor,
            p.version,
            category_display(p),
            p.license,
        );
        // Description on an indented second line.
        if !p.description.is_empty() {
            println!("  {}", p.description);
        }
    }

    // ── Summary ───────────────────────────────────────────────────────────────
    println!();
    println!(
        "Found {} plugin{}",
        results.len(),
        if results.len() == 1 { "" } else { "s" }
    );

    Ok(())
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Format category / subcategory as "category" or "category / subcategory".
fn category_display(p: &crate::registry::PluginDefinition) -> String {
    match &p.subcategory {
        Some(sub) => format!("{} / {}", p.category, sub),
        None => p.category.clone(),
    }
}
