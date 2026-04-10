use anyhow::Result;
use colored::Colorize;

use apm_core::config::Config;
use apm_core::registry;

/// Plugin Boutique affiliate tracking code.
const PLUGINBOUTIQUE_AFFILIATE_AID: &str = "69d5e87c5f2e9";

pub async fn run(config: &Config, name: &str) -> Result<()> {
    let registry = registry::Registry::load_all_sources(config)?;

    if registry.is_empty() {
        anyhow::bail!(
            "Registry cache is empty.\n\
             Hint: Run `apm sync` to populate the local registry cache."
        );
    }

    let plugin = registry.find(name).ok_or_else(|| {
        anyhow::anyhow!(
            "Plugin '{name}' not found.\n\
             Hint: Try `apm search {name}` to find the correct name."
        )
    })?;

    if !plugin.is_paid {
        println!(
            "'{}' is free — install it directly with: {}",
            plugin.name,
            format!("apm install {}", plugin.slug).bold()
        );
        return Ok(());
    }

    let url = purchase_url(plugin);

    println!(
        "Opening purchase page for {} ({})...",
        plugin.name.bold(),
        crate::utils::format_price(
            plugin.price_cents,
            plugin.currency.as_deref(),
            plugin.is_paid
        )
        .cyan()
    );
    println!("  {}", url.dimmed());

    std::process::Command::new("open")
        .arg(&url)
        .spawn()
        .map_err(|e| anyhow::anyhow!("Failed to open browser: {e}"))?;

    println!(
        "\nAfter purchasing, install with: {}",
        format!("apm install {}", plugin.slug).bold()
    );

    Ok(())
}

/// Construct the best purchase URL for a plugin.
///
/// Priority:
/// 1. Explicit `purchase_url` from the registry (curated deep link)
/// 2. Plugin Boutique search with affiliate tracking
fn purchase_url(plugin: &registry::PluginDefinition) -> String {
    if let Some(url) = &plugin.purchase_url {
        return url.clone();
    }

    // Fall back to Plugin Boutique search.
    let query = url_encode(&plugin.name);
    format!("https://www.pluginboutique.com/search?s={query}&a_aid={PLUGINBOUTIQUE_AFFILIATE_AID}")
}

/// Minimal percent-encoding for URL query parameters.
fn url_encode(s: &str) -> String {
    let mut encoded = String::with_capacity(s.len());
    for byte in s.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(byte as char);
            }
            b' ' => encoded.push('+'),
            _ => {
                encoded.push('%');
                encoded.push_str(&format!("{byte:02X}"));
            }
        }
    }
    encoded
}
