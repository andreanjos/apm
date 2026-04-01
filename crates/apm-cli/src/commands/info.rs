use anyhow::Result;
use colored::Colorize;
use serde::Serialize;

use crate::config::Config;
use crate::registry::{self, PluginDefinition};
use crate::state::InstallState;

/// JSON-serializable view of a plugin info result.
#[derive(Serialize)]
struct PluginInfoJson<'a> {
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
    installed: bool,
    installed_version: Option<String>,
}

pub async fn run(config: &Config, name: &str, json: bool) -> Result<()> {
    let registry = registry::Registry::load_all_sources(config)?;

    if registry.is_empty() {
        if json {
            println!("null");
        } else {
            println!(
                "Registry cache is empty. Run `apm sync` to download the plugin registry."
            );
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
            category: &plugin.category,
            subcategory: plugin.subcategory.as_deref(),
            license: &plugin.license,
            description: &plugin.description,
            tags: &plugin.tags,
            homepage: plugin.homepage.as_deref(),
            formats,
            installed: installed.is_some(),
            installed_version: installed.map(|i| i.version.clone()),
        };
        println!("{}", serde_json::to_string_pretty(&info)?);
    } else {
        print_plugin_info(plugin, installed);
    }
    Ok(())
}

// ── Display ───────────────────────────────────────────────────────────────────

fn print_plugin_info(p: &PluginDefinition, installed: Option<&crate::state::InstalledPlugin>) {
    // Title
    println!("{}", p.slug.bold());
    println!("{}", "\u{2550}".repeat(47).dimmed()); // ═══════

    println!("{:<13} {}", "Name:".dimmed(), p.name.bold());
    println!("{:<13} {}", "Vendor:".dimmed(), p.vendor);
    println!("{:<13} {}", "Version:".dimmed(), p.version.cyan());

    // Category
    let cat = match &p.subcategory {
        Some(sub) => format!("{} / {}", p.category, sub),
        None => p.category.clone(),
    };
    println!("{:<13} {}", "Category:".dimmed(), cat);

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
