use anyhow::Result;
use colored::Colorize;

use crate::api::storefront::{CompareResponse, StorefrontHttpClient, StorefrontPlugin};

pub async fn run(left: &str, right: &str, json: bool) -> Result<()> {
    let client = StorefrontHttpClient::from_env();
    let comparison = client.compare(left, right).await?;

    if json {
        println!("{}", serde_json::to_string_pretty(&comparison)?);
        return Ok(());
    }

    print_human(&comparison);
    Ok(())
}

fn print_human(response: &CompareResponse) {
    println!(
        "{}",
        format!("Compare: {} vs {}", response.left.slug, response.right.slug).bold()
    );
    println!("{}", "\u{2500}".repeat(74).dimmed());
    print_row("Name", &response.left.name, &response.right.name);
    print_row("Vendor", &response.left.vendor, &response.right.vendor);
    print_row("Version", &response.left.version, &response.right.version);
    print_row(
        "Category",
        &response.left.category,
        &response.right.category,
    );
    print_row(
        "Type",
        &type_label(&response.left),
        &type_label(&response.right),
    );
    print_row(
        "Price",
        &price_label(&response.left),
        &price_label(&response.right),
    );
    print_row(
        "Formats",
        &response.left.formats.join(", "),
        &response.right.formats.join(", "),
    );
    print_row(
        "Tags",
        &join_or_none(&response.left.tags),
        &join_or_none(&response.right.tags),
    );
    print_row(
        "Description",
        &response.left.description,
        &response.right.description,
    );
}

fn print_row(label: &str, left: &str, right: &str) {
    println!(
        "{:<12} {:<28} {}",
        format!("{label}:").dimmed(),
        left,
        right
    );
}

fn price_label(plugin: &StorefrontPlugin) -> String {
    if !plugin.is_paid {
        return "free".to_string();
    }

    let major = plugin.price_cents / 100;
    let minor = plugin.price_cents.abs() % 100;
    format!("{} {}.{minor:02}", plugin.currency.to_uppercase(), major)
}

fn type_label(plugin: &StorefrontPlugin) -> String {
    if plugin.is_paid {
        "Paid".to_string()
    } else {
        "Free".to_string()
    }
}

fn join_or_none(items: &[String]) -> String {
    if items.is_empty() {
        "(none)".to_string()
    } else {
        items.join(", ")
    }
}
