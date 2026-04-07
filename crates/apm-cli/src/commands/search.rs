use anyhow::Result;
use colored::Colorize;
use serde::Serialize;

use apm_core::config::Config;
use apm_core::registry::{self, search};

use crate::utils::{format_category, format_price};

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
    is_paid: bool,
    price_cents: Option<i64>,
    currency: Option<String>,
    price_display: String,
}

pub async fn run(
    config: &Config,
    query: &str,
    category: Option<&str>,
    vendor: Option<&str>,
    paid_only: bool,
    free_only: bool,
    json: bool,
) -> Result<()> {
    let registry = registry::Registry::load_all_sources(config)?;

    if registry.is_empty() {
        if json {
            println!("[]");
        } else {
            println!("Registry cache is empty. Run `apm sync` to download the plugin registry.");
        }
        return Ok(());
    }

    let results: Vec<_> = search::search(&registry, query, category, vendor)
        .into_iter()
        .filter(|plugin| {
            if paid_only && !plugin.is_paid {
                return false;
            }
            if free_only && plugin.is_paid {
                return false;
            }
            true
        })
        .collect();

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
                is_paid: p.is_paid,
                price_cents: p.price_cents,
                currency: p.currency.clone(),
                price_display: format_price(p.price_cents, p.currency.as_deref(), p.is_paid),
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
    const HDR_PRICE: &str = "Price";

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
        .map(|p| format_category(&p.category, p.subcategory.as_deref()).len())
        .max()
        .unwrap_or(0)
        .max(HDR_CAT.len());

    let w_lic = results
        .iter()
        .map(|p| p.license.len())
        .max()
        .unwrap_or(0)
        .max(HDR_LIC.len());

    let w_price = results
        .iter()
        .map(|p| format_price(p.price_cents, p.currency.as_deref(), p.is_paid).len())
        .max()
        .unwrap_or(0)
        .max(HDR_PRICE.len());

    // ── Header ────────────────────────────────────────────────────────────────
    println!(
        "{}",
        format!(
            "{:<w_name$}  {:<w_vendor$}  {:<w_ver$}  {:<w_cat$}  {:<w_lic$}  {}",
            HDR_NAME, HDR_VENDOR, HDR_VER, HDR_CAT, HDR_LIC, HDR_PRICE,
        )
        .bold()
    );

    let rule_len = w_name + 2 + w_vendor + 2 + w_ver + 2 + w_cat + 2 + w_lic + 2 + w_price;
    println!("{}", "\u{2500}".repeat(rule_len).dimmed());

    // ── Rows ──────────────────────────────────────────────────────────────────
    for p in &results {
        println!(
            "{:<w_name$}  {:<w_vendor$}  {:<w_ver$}  {:<w_cat$}  {:<w_lic$}  {}",
            p.slug.bold().to_string(),
            p.vendor,
            p.version.cyan().to_string(),
            format_category(&p.category, p.subcategory.as_deref()),
            p.license,
            format_price(p.price_cents, p.currency.as_deref(), p.is_paid),
        );
        // Description on an indented second line.
        if !p.description.is_empty() {
            println!("  {}", p.description.dimmed());
        }
    }

    // ── Summary ───────────────────────────────────────────────────────────────
    println!();
    println!(
        "{}",
        format!(
            "Found {} plugin{}",
            results.len(),
            if results.len() == 1 { "" } else { "s" }
        )
        .dimmed()
    );

    Ok(())
}

