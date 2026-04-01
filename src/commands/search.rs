use anyhow::Result;

use crate::config::Config;
use crate::registry::{self, search};

pub async fn run(config: &Config, query: &str, category: Option<&str>) -> Result<()> {
    let registry = registry::Registry::load_all_sources(config)?;

    if registry.is_empty() {
        println!(
            "Registry cache is empty. Run `apm sync` to download the plugin registry."
        );
        return Ok(());
    }

    let results = search::search(&registry, query, category);

    if results.is_empty() {
        let filter_msg = category
            .map(|c| format!(" in category '{c}'"))
            .unwrap_or_default();
        if query.is_empty() {
            println!("No plugins found{filter_msg}.");
        } else {
            println!("No plugins found matching '{query}'{filter_msg}.");
        }
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
