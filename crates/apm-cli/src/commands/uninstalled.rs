use std::collections::HashSet;

use anyhow::Result;
use colored::Colorize;
use serde::Serialize;

use apm_core::config::Config;
use apm_core::registry::Registry;
use apm_core::state::InstallState;

use crate::utils::format_category;

/// JSON-serializable view of an uninstalled standalone plugin.
#[derive(Serialize)]
struct UninstalledPluginJson {
    slug: String,
    name: String,
    vendor: String,
    category: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    subcategory: Option<String>,
}

/// JSON wrapper with total count.
#[derive(Serialize)]
struct UninstalledOutputJson {
    uninstalled: Vec<UninstalledPluginJson>,
    total: usize,
}

pub async fn run(
    config: &Config,
    category: Option<&str>,
    limit: Option<usize>,
    json: bool,
) -> Result<()> {
    let registry = Registry::load_all_sources(config)?;

    if registry.is_empty() {
        if json {
            let output = UninstalledOutputJson {
                uninstalled: vec![],
                total: 0,
            };
            println!("{}", serde_json::to_string_pretty(&output)?);
        } else {
            println!("No catalog items found. The registry cache is empty.");
            println!();
            println!("To get started:");
            println!("  apm sync    Download the registry");
        }
        return Ok(());
    }

    let state = InstallState::load(config)?;

    // Build a set of installed plugin slugs for fast lookup.
    let installed_slugs: HashSet<&str> = state.plugins.iter().map(|p| p.name.as_str()).collect();

    // Collect standalone registry plugins that are NOT installed, optionally
    // filtering by category. Bundles/upgrades/subscriptions are discoverable via
    // search/info, but this command is specifically for installable plugins.
    let mut uninstalled: Vec<_> = registry
        .plugins
        .values()
        .filter(|p| p.is_standalone_plugin())
        .filter(|p| !installed_slugs.contains(p.slug.as_str()))
        .filter(|p| {
            if let Some(cat) = category {
                let cat_lower = cat.to_lowercase();
                let matches_cat = p.category.to_lowercase().contains(&cat_lower);
                let matches_sub = p
                    .subcategory
                    .as_deref()
                    .map(|s| s.to_lowercase().contains(&cat_lower))
                    .unwrap_or(false);
                matches_cat || matches_sub
            } else {
                true
            }
        })
        .collect();

    // Sort alphabetically by slug for stable output.
    uninstalled.sort_by(|a, b| a.slug.to_lowercase().cmp(&b.slug.to_lowercase()));

    let total = uninstalled.len();
    let total_registry_plugins = registry
        .plugins
        .values()
        .filter(|p| p.is_standalone_plugin())
        .count();

    if total == 0 {
        if json {
            let output = UninstalledOutputJson {
                uninstalled: vec![],
                total: 0,
            };
            println!("{}", serde_json::to_string_pretty(&output)?);
        } else if category.is_some() {
            println!(
                "All standalone plugins in that category are already installed (or none match the filter)."
            );
        } else {
            println!("All standalone registry plugins are already installed.");
        }
        return Ok(());
    }

    // Apply limit.
    let display: Vec<_> = if let Some(n) = limit {
        uninstalled.into_iter().take(n).collect()
    } else {
        uninstalled
    };

    // ── JSON output ──────────────────────────────────────────────────────────
    if json {
        let json_plugins: Vec<UninstalledPluginJson> = display
            .iter()
            .map(|p| UninstalledPluginJson {
                slug: p.slug.clone(),
                name: p.name.clone(),
                vendor: p.vendor.clone(),
                category: p.category.clone(),
                subcategory: p.subcategory.clone(),
            })
            .collect();
        let output = UninstalledOutputJson {
            uninstalled: json_plugins,
            total,
        };
        println!("{}", serde_json::to_string_pretty(&output)?);
        return Ok(());
    }

    // ── Human-readable output ────────────────────────────────────────────────

    let header = format!(
        "Available standalone plugins not installed ({} of {}):",
        total, total_registry_plugins
    );
    println!();
    println!("{}", header.bold());
    println!();

    const HDR_NAME: &str = "Name";
    const HDR_VENDOR: &str = "Vendor";
    const HDR_CAT: &str = "Category";

    let w_name = display
        .iter()
        .map(|p| p.slug.len())
        .max()
        .unwrap_or(0)
        .max(HDR_NAME.len());

    let w_vendor = display
        .iter()
        .map(|p| p.vendor.len())
        .max()
        .unwrap_or(0)
        .max(HDR_VENDOR.len());

    // Header row.
    println!(
        "  {:<w_name$}  {:<w_vendor$}  {}",
        HDR_NAME.bold(),
        HDR_VENDOR.bold(),
        HDR_CAT.bold(),
    );

    let rule_len = 2 + w_name + 2 + w_vendor + 2 + HDR_CAT.len() + 8;
    println!("  {}", "\u{2500}".repeat(rule_len).dimmed());

    for p in &display {
        let cat = format_category(&p.category, p.subcategory.as_deref());
        println!(
            "  {:<w_name$}  {:<w_vendor$}  {}",
            p.slug.bold().to_string(),
            p.vendor,
            cat.dimmed(),
        );
    }

    // ── Footer ───────────────────────────────────────────────────────────────
    println!();
    let displayed = display.len();
    if displayed < total {
        println!(
            "{}",
            format!("Showing {displayed} of {total}. Use --limit to see more.").dimmed()
        );
    } else {
        println!(
            "{}",
            format!(
                "{} plugin{} available to install.",
                total,
                if total == 1 { "" } else { "s" }
            )
            .dimmed()
        );
    }

    Ok(())
}
