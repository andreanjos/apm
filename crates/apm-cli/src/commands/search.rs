use std::collections::HashSet;

use anyhow::Result;
use colored::Colorize;
use serde::Serialize;

use apm_core::config::Config;
use apm_core::registry::{self, search};
use apm_core::state::InstallState;

use crate::utils::{format_category, format_price};

/// JSON-serializable view of a search result.
#[derive(Serialize)]
struct SearchResultJson {
    slug: String,
    name: String,
    vendor: String,
    version: String,
    product_type: String,
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

#[allow(clippy::too_many_arguments)]
pub async fn run(
    config: &Config,
    query: &str,
    category: Option<&str>,
    vendor: Option<&str>,
    paid_only: bool,
    free_only: bool,
    tag: Option<&str>,
    limit: Option<usize>,
    installed: bool,
    new: bool,
    json: bool,
) -> Result<()> {
    let registry = registry::Registry::load_all_sources(config)?;

    if registry.is_empty() {
        if json {
            println!("[]");
        } else {
            println!("No plugins found. The registry cache is empty.");
            println!();
            println!("To get started:");
            println!("  apm sync    Download the plugin registry");
            println!("  apm search  Then search for plugins");
        }
        return Ok(());
    }

    // When --installed or --new is set, load the install state and build a lookup set.
    let installed_slugs: Option<HashSet<String>> = if installed || new {
        let state = InstallState::load(config).unwrap_or_default();
        Some(state.plugins.iter().map(|p| p.name.clone()).collect())
    } else {
        None
    };

    let results: Vec<_> = search::search(&registry, query, category, vendor, tag)
        .into_iter()
        .filter(|plugin| {
            if paid_only && !plugin.is_paid {
                return false;
            }
            if free_only && plugin.is_paid {
                return false;
            }
            if let Some(ref slugs) = installed_slugs {
                if installed && !slugs.contains(&plugin.slug) {
                    return false;
                }
                if new && slugs.contains(&plugin.slug) {
                    return false;
                }
            }
            true
        })
        .collect();

    let total_matches = results.len();

    if results.is_empty() {
        if json {
            println!("[]");
            return Ok(());
        }
        let mut filter_msg = String::new();
        if installed {
            filter_msg.push_str(" among installed plugins");
        }
        if new {
            filter_msg.push_str(" among not-yet-installed plugins");
        }
        if let Some(c) = category {
            filter_msg.push_str(&format!(" in category \"{c}\""));
        }
        if let Some(v) = vendor {
            filter_msg.push_str(&format!(" by vendor \"{v}\""));
        }
        if let Some(t) = tag {
            filter_msg.push_str(&format!(" tagged \"{t}\""));
        }
        if query.is_empty() {
            println!("No plugins found{filter_msg}.");
        } else {
            println!("No plugins found matching \"{query}\"{filter_msg}.");
        }
        println!(
            "{}",
            "Hint: Try a broader search, or run `apm sync` to update the registry.".dimmed()
        );
        return Ok(());
    }

    // Apply limit.
    let display_results: Vec<_> = if let Some(n) = limit {
        results.into_iter().take(n).collect()
    } else {
        results
    };

    // ── JSON output ───────────────────────────────────────────────────────────
    if json {
        let json_results: Vec<SearchResultJson> = display_results
            .iter()
            .map(|p| SearchResultJson {
                slug: p.slug.clone(),
                name: p.name.clone(),
                vendor: p.vendor.clone(),
                version: p.version.clone(),
                product_type: p.product_type.to_string(),
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
    const HDR_PROD: &str = "Product";
    const HDR_VER: &str = "Version";
    const HDR_CAT: &str = "Category";
    const HDR_LIC: &str = "License";
    const HDR_PRICE: &str = "Price";

    let w_name = display_results
        .iter()
        .map(|p| p.slug.len())
        .max()
        .unwrap_or(0)
        .max(HDR_NAME.len());

    let w_vendor = display_results
        .iter()
        .map(|p| p.vendor.len())
        .max()
        .unwrap_or(0)
        .max(HDR_VENDOR.len());

    let w_prod = display_results
        .iter()
        .map(|p| p.product_type.to_string().len())
        .max()
        .unwrap_or(0)
        .max(HDR_PROD.len());

    let w_ver = display_results
        .iter()
        .map(|p| p.version.len())
        .max()
        .unwrap_or(0)
        .max(HDR_VER.len());

    let w_cat = display_results
        .iter()
        .map(|p| format_category(&p.category, p.subcategory.as_deref()).len())
        .max()
        .unwrap_or(0)
        .max(HDR_CAT.len());

    let w_lic = display_results
        .iter()
        .map(|p| p.license.len())
        .max()
        .unwrap_or(0)
        .max(HDR_LIC.len());

    let w_price = display_results
        .iter()
        .map(|p| format_price(p.price_cents, p.currency.as_deref(), p.is_paid).len())
        .max()
        .unwrap_or(0)
        .max(HDR_PRICE.len());

    // ── Header ────────────────────────────────────────────────────────────────
    println!(
        "{}",
        format!(
            "{:<w_name$}  {:<w_vendor$}  {:<w_prod$}  {:<w_ver$}  {:<w_cat$}  {:<w_lic$}  {}",
            HDR_NAME, HDR_VENDOR, HDR_PROD, HDR_VER, HDR_CAT, HDR_LIC, HDR_PRICE,
        )
        .bold()
    );

    let rule_len =
        w_name + 2 + w_vendor + 2 + w_prod + 2 + w_ver + 2 + w_cat + 2 + w_lic + 2 + w_price;
    println!("{}", "\u{2500}".repeat(rule_len).dimmed());

    // ── Rows ──────────────────────────────────────────────────────────────────
    for p in &display_results {
        println!(
            "{:<w_name$}  {:<w_vendor$}  {:<w_prod$}  {:<w_ver$}  {:<w_cat$}  {:<w_lic$}  {}",
            p.slug.bold().to_string(),
            p.vendor,
            p.product_type.to_string(),
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

    // Build a descriptive footer: "Found 5 free plugins matching "reverb" in category "effects""
    let price_qualifier = if free_only {
        " free"
    } else if paid_only {
        " paid"
    } else {
        ""
    };
    let installed_qualifier = if installed {
        " installed"
    } else if new {
        " new"
    } else {
        ""
    };
    let item_word = if total_matches == 1 { "item" } else { "items" };

    let mut footer =
        format!("Found {total_matches}{price_qualifier}{installed_qualifier} {item_word}");

    if !query.is_empty() {
        footer.push_str(&format!(" matching \"{query}\""));
    }
    if let Some(c) = category {
        footer.push_str(&format!(" in category \"{c}\""));
    }
    if let Some(v) = vendor {
        footer.push_str(&format!(" by vendor \"{v}\""));
    }
    if let Some(t) = tag {
        footer.push_str(&format!(" tagged \"{t}\""));
    }

    // If limit truncated results, note how many are shown.
    let displayed = display_results.len();
    if displayed < total_matches {
        footer.push_str(&format!(" (showing {displayed})"));
    }

    println!("{}", footer.dimmed());

    Ok(())
}
