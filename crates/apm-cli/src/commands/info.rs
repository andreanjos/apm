use anyhow::Result;
use colored::Colorize;
use serde::Serialize;

use apm_core::config::Config;
use apm_core::engine::{ApmEngine, PackageDetails, PackageDetailsResult};

use crate::utils::{format_category, format_price};

/// JSON-serializable view of a plugin info result.
#[derive(Serialize)]
struct PluginInfoJson<'a> {
    slug: &'a str,
    name: &'a str,
    vendor: &'a str,
    version: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    available_versions: Option<Vec<String>>,
    product_type: String,
    category: &'a str,
    subcategory: Option<&'a str>,
    license: &'a str,
    description: &'a str,
    tags: &'a [String],
    homepage: Option<&'a str>,
    formats: Vec<String>,
    installed: bool,
    installed_version: Option<String>,
    is_paid: bool,
    price_cents: Option<i64>,
    currency: Option<&'a str>,
    price_display: String,
}

pub async fn run(config: &Config, name: &str, json: bool, versions: bool) -> Result<()> {
    let engine = ApmEngine::new(config.clone());
    let result = engine.package_details(name, versions)?;

    let details = match result {
        PackageDetailsResult::CatalogEmpty => {
            if json {
                println!("null");
            } else {
                println!(
                    "Registry cache is empty. Run `apm sync` to download the plugin registry."
                );
            }
            return Ok(());
        }
        PackageDetailsResult::NotFound => {
            if json {
                println!("null");
            } else {
                println!(
                    "Plugin '{name}' not found. Try `apm search {name}` to find the correct name."
                );
            }
            return Ok(());
        }
        PackageDetailsResult::Found { package } => package,
    };

    if json {
        let summary = &details.summary;
        let info = PluginInfoJson {
            slug: &summary.slug,
            name: &summary.name,
            vendor: &summary.vendor,
            version: &summary.version,
            available_versions: if versions {
                Some(details.available_versions.clone())
            } else {
                None
            },
            product_type: summary.product_type.to_string(),
            category: &summary.category,
            subcategory: summary.subcategory.as_deref(),
            license: &summary.license,
            description: &summary.description,
            tags: &summary.tags,
            homepage: details.homepage.as_deref(),
            formats: details
                .summary
                .formats
                .iter()
                .map(|format| format.format.to_string())
                .collect(),
            installed: summary.installed,
            installed_version: summary.installed_version.clone(),
            is_paid: summary.is_paid,
            price_cents: summary.price_cents,
            currency: summary.currency.as_deref(),
            price_display: format_price(
                summary.price_cents,
                summary.currency.as_deref(),
                summary.is_paid,
            ),
        };
        println!("{}", serde_json::to_string_pretty(&info)?);
    } else {
        print_plugin_info(&details, versions);
    }
    Ok(())
}

// ── Display ───────────────────────────────────────────────────────────────────

fn print_plugin_info(details: &PackageDetails, show_versions: bool) {
    let p = &details.summary;

    // Title
    println!("{}", p.slug.bold());
    println!("{}", "\u{2550}".repeat(47).dimmed()); // ═══════

    println!("{:<13} {}", "Name:".dimmed(), p.name.bold());
    println!("{:<13} {}", "Vendor:".dimmed(), p.vendor);
    println!("{:<13} {}", "Version:".dimmed(), p.version.cyan());
    println!("{:<13} {}", "Product:".dimmed(), p.product_type);
    println!(
        "{:<13} {}",
        "Access:".dimmed(),
        if p.is_paid { "Paid" } else { "Free" }
    );
    println!(
        "{:<13} {}",
        "Price:".dimmed(),
        format_price(p.price_cents, p.currency.as_deref(), p.is_paid)
    );

    // Category
    println!(
        "{:<13} {}",
        "Category:".dimmed(),
        format_category(&p.category, p.subcategory.as_deref())
    );

    println!("{:<13} {}", "License:".dimmed(), p.license);

    if let Some(hp) = &details.homepage {
        println!("{:<13} {}", "Homepage:".dimmed(), hp);
    }

    if p.is_paid {
        println!(
            "{:<13} {}",
            "Buy:".dimmed(),
            format!("apm buy {}", p.slug).bold()
        );
    }

    // Tags
    if !p.tags.is_empty() {
        println!("{:<13} {}", "Tags:".dimmed(), p.tags.join(", "));
    }

    // Description
    println!();
    println!("{}", "Description:".bold());
    if p.description.is_empty() {
        println!("  {}", "(no description)".dimmed());
    } else {
        // Word-wrap at 72 chars.
        for line in wrap_text(&p.description, 70) {
            println!("  {line}");
        }
    }

    // Available formats
    println!();
    println!("{}", "Available Formats:".bold());
    if p.formats.is_empty() {
        println!("  {}", "(none listed)".dimmed());
    } else {
        for format in &p.formats {
            println!(
                "  {:<6} ({})",
                format.format.to_string().cyan(),
                format.install_type
            );
        }
    }

    // Install status
    println!();
    match p.installed_version.as_deref() {
        Some(version) => {
            println!(
                "Status:       {}",
                format!("Installed (v{version})").green()
            );
        }
        None => {
            println!("Status:       {}", "Not installed".yellow());
        }
    }

    // Available versions (only when --versions flag is active)
    if show_versions {
        let versions = &details.available_versions;
        println!();
        println!("{}", "Available Versions:".bold());
        for (i, v) in versions.iter().enumerate() {
            if i == 0 && versions.len() == 1 {
                println!("  {}  {}", v.cyan(), "(latest, only version)".dimmed());
            } else if i == 0 {
                println!("  {}  {}", v.cyan(), "(latest)".dimmed());
            } else {
                println!("  {}", v);
            }
        }
    }
}

/// Very basic word-wrap: split on spaces and reflow to fit `width` columns.
fn wrap_text(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();

    for word in text.split_whitespace() {
        if current.is_empty() {
            current.push_str(word);
        } else if current.len() + 1 + word.len() <= width {
            current.push(' ');
            current.push_str(word);
        } else {
            lines.push(current.clone());
            current = word.to_string();
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}
