use anyhow::Result;
use colored::Colorize;

use crate::api::storefront::{StorefrontHttpClient, StorefrontPlugin, StorefrontSection};

pub async fn run(json: bool) -> Result<()> {
    let client = StorefrontHttpClient::from_env();
    let response = client.explore().await?;

    if json {
        println!("{}", serde_json::to_string_pretty(&response)?);
        return Ok(());
    }

    if response.categories.is_empty() {
        println!("No explore categories are available.");
        return Ok(());
    }

    println!("{}", "Explore".bold());
    println!("{}", "\u{2550}".repeat(36).dimmed());
    for section in &response.categories {
        print_section(section);
    }
    Ok(())
}

fn print_section(section: &StorefrontSection) {
    println!();
    println!("{}", section.title.bold());
    if let Some(description) = &section.description {
        println!("{}", description.dimmed());
    }

    for plugin in &section.plugins {
        println!(
            "  {}  {}  {}  {}",
            plugin.slug.bold(),
            plugin.category,
            plugin.version.cyan(),
            price_label(plugin),
        );
        println!("    {}", plugin.description.dimmed());
    }
}

fn price_label(plugin: &StorefrontPlugin) -> String {
    if !plugin.is_paid {
        return "free".to_string();
    }

    let major = plugin.price_cents / 100;
    let minor = plugin.price_cents.abs() % 100;
    format!("{} {}.{minor:02}", plugin.currency.to_uppercase(), major)
}
