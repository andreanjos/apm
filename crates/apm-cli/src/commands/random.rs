use std::time::SystemTime;

use anyhow::Result;
use colored::Colorize;
use serde::Serialize;

use apm_core::config::Config;
use apm_core::registry::{self, PluginDefinition};

use crate::utils::{format_category, format_price};

/// JSON-serializable view of a random plugin pick.
#[derive(Serialize)]
struct RandomPickJson<'a> {
    slug: &'a str,
    name: &'a str,
    vendor: &'a str,
    version: &'a str,
    category: &'a str,
    subcategory: Option<&'a str>,
    license: &'a str,
    description: &'a str,
    tags: &'a [String],
    homepage: Option<&'a str>,
    formats: Vec<String>,
    is_paid: bool,
    price_cents: Option<i64>,
    currency: Option<&'a str>,
    price_display: String,
}

pub async fn run(config: &Config, category: Option<&str>, json: bool) -> Result<()> {
    let registry = registry::Registry::load_all_sources(config)?;

    if registry.is_empty() {
        if json {
            println!("null");
        } else {
            println!("Registry cache is empty. Run `apm sync` to download the plugin registry.");
        }
        return Ok(());
    }

    // Collect all plugins, optionally filtered by category.
    let category_lower = category.map(|c| c.to_lowercase());
    let candidates: Vec<&PluginDefinition> = registry
        .plugins
        .values()
        .filter(|p| {
            if let Some(ref cat) = category_lower {
                p.category.to_lowercase().contains(cat.as_str())
                    || p.subcategory
                        .as_deref()
                        .map(|s| s.to_lowercase().contains(cat.as_str()))
                        .unwrap_or(false)
            } else {
                true
            }
        })
        .collect();

    if candidates.is_empty() {
        if json {
            println!("null");
        } else if let Some(cat) = category {
            println!("No plugins found in category \"{cat}\".");
            println!(
                "{}",
                "Hint: Try `apm search --category <name>` to browse available categories.".dimmed()
            );
        } else {
            println!("No plugins available.");
        }
        return Ok(());
    }

    // Pick a pseudo-random plugin using subsecond nanos — no extra dependency needed.
    let seed = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .subsec_nanos() as usize;
    let index = seed % candidates.len();
    let plugin = candidates[index];

    if json {
        let mut formats: Vec<String> = plugin.formats.keys().map(|f| f.to_string()).collect();
        formats.sort();
        let pick = RandomPickJson {
            slug: &plugin.slug,
            name: &plugin.name,
            vendor: &plugin.vendor,
            version: &plugin.version,
            category: &plugin.category,
            subcategory: plugin.subcategory.as_deref(),
            license: &plugin.license,
            description: &plugin.description,
            tags: &plugin.tags,
            homepage: plugin.homepage.as_deref(),
            formats,
            is_paid: plugin.is_paid,
            price_cents: plugin.price_cents,
            currency: plugin.currency.as_deref(),
            price_display: format_price(
                plugin.price_cents,
                plugin.currency.as_deref(),
                plugin.is_paid,
            ),
        };
        println!("{}", serde_json::to_string_pretty(&pick)?);
    } else {
        print_random_pick(plugin, category);
    }

    Ok(())
}

fn print_random_pick(p: &PluginDefinition, category_filter: Option<&str>) {
    // Vary the label when a category filter was applied.
    let label = match category_filter {
        Some(cat) => format!("Random {cat}"),
        None => "Random pick".to_string(),
    };

    println!("{} {}", format!("\u{1F3B2} {label}:").bold(), p.name.bold());
    println!(
        "   {:<11} {}",
        "Category:".dimmed(),
        format_category(&p.category, p.subcategory.as_deref())
    );
    println!("   {:<11} {}", "Vendor:".dimmed(), p.vendor);
    println!("   {:<11} {}", "Version:".dimmed(), p.version.cyan());
    println!(
        "   {:<11} {}",
        "Type:".dimmed(),
        if p.is_paid { "Paid" } else { "Free" }
    );
    println!(
        "   {:<11} {}",
        "Price:".dimmed(),
        format_price(p.price_cents, p.currency.as_deref(), p.is_paid)
    );
    println!("   {:<11} {}", "License:".dimmed(), p.license);

    if !p.description.is_empty() {
        println!();
        println!("   {}", p.description);
    }

    println!();
    println!("   Install: {}", format!("apm install {}", p.slug).green());
}
