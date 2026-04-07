use anyhow::Result;
use colored::Colorize;
use serde::Serialize;

use apm_core::config::Config;
use apm_core::registry::{self, PluginDefinition};
use apm_core::state::InstallState;

use crate::utils::{format_category, format_price};

/// JSON-serializable view of a plugin info result.
#[derive(Serialize)]
struct PluginInfoJson<'a> {
    slug: &'a str,
    name: &'a str,
    vendor: &'a str,
    version: &'a str,
    available_versions: Vec<String>,
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

pub async fn run(config: &Config, name: &str, json: bool) -> Result<()> {
    let registry = registry::Registry::load_all_sources(config)?;

    if registry.is_empty() {
        if json {
            println!("null");
        } else {
            println!("Registry cache is empty. Run `apm sync` to download the plugin registry.");
        }
        return Ok(());
    }

    let plugin = match registry.find(name) {
        Some(p) => p,
        None => {
            if json {
                println!("null");
            } else {
                println!(
                    "Plugin '{name}' not found. Try `apm search {name}` to find the correct name."
                );
            }
            return Ok(());
        }
    };

    // Check install state.
    let state = InstallState::load(config)?;
    let installed = state.find(&plugin.slug);

    if json {
        let mut formats: Vec<String> = plugin.formats.keys().map(|f| f.to_string()).collect();
        formats.sort();
        let info = PluginInfoJson {
            slug: &plugin.slug,
            name: &plugin.name,
            vendor: &plugin.vendor,
            version: &plugin.version,
            available_versions: plugin.available_versions(),
            category: &plugin.category,
            subcategory: plugin.subcategory.as_deref(),
            license: &plugin.license,
            description: &plugin.description,
            tags: &plugin.tags,
            homepage: plugin.homepage.as_deref(),
            formats,
            installed: installed.is_some(),
            installed_version: installed.map(|i| i.version.clone()),
            is_paid: plugin.is_paid,
            price_cents: plugin.price_cents,
            currency: plugin.currency.as_deref(),
            price_display: format_price(plugin.price_cents, plugin.currency.as_deref(), plugin.is_paid),
        };
        println!("{}", serde_json::to_string_pretty(&info)?);
    } else {
        print_plugin_info(plugin, installed);
    }
    Ok(())
}

// ── Display ───────────────────────────────────────────────────────────────────

fn print_plugin_info(p: &PluginDefinition, installed: Option<&apm_core::state::InstalledPlugin>) {
    // Title
    println!("{}", p.slug.bold());
    println!("{}", "\u{2550}".repeat(47).dimmed()); // ═══════

    println!("{:<13} {}", "Name:".dimmed(), p.name.bold());
    println!("{:<13} {}", "Vendor:".dimmed(), p.vendor);
    println!("{:<13} {}", "Version:".dimmed(), p.version.cyan());
    let available_versions = p.available_versions();
    if available_versions.len() > 1 {
        println!(
            "{:<13} {}",
            "Versions:".dimmed(),
            available_versions.join(", ")
        );
    }
    println!(
        "{:<13} {}",
        "Type:".dimmed(),
        if p.is_paid { "Paid" } else { "Free" }
    );
    println!("{:<13} {}", "Price:".dimmed(), format_price(p.price_cents, p.currency.as_deref(), p.is_paid));

    // Category
    println!(
        "{:<13} {}",
        "Category:".dimmed(),
        format_category(&p.category, p.subcategory.as_deref())
    );

    println!("{:<13} {}", "License:".dimmed(), p.license);

    if let Some(hp) = &p.homepage {
        println!("{:<13} {}", "Homepage:".dimmed(), hp);
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
        let mut formats: Vec<_> = p.formats.iter().collect();
        formats.sort_by_key(|(fmt, _)| fmt.to_string());
        for (fmt, src) in formats {
            println!("  {:<6} ({})", fmt.to_string().cyan(), src.install_type);
        }
    }

    // Install status
    println!();
    match installed {
        Some(inst) => {
            println!(
                "Status:       {}",
                format!("Installed (v{})", inst.version).green()
            );
        }
        None => {
            println!("Status:       {}", "Not installed".yellow());
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

