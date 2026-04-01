// bundles command — list and inspect plugin bundles (meta-packages).

use anyhow::Result;
use colored::Colorize;

use crate::config::Config;
use crate::registry::Registry;

// ── Entry point ───────────────────────────────────────────────────────────────

pub async fn run(config: &Config, info_name: Option<&str>) -> Result<()> {
    let registry = Registry::load_all_sources(config)?;

    if let Some(name) = info_name {
        run_info(&registry, name)
    } else {
        run_list(&registry)
    }
}

// ── List all bundles ──────────────────────────────────────────────────────────

fn run_list(registry: &Registry) -> Result<()> {
    if registry.bundles.is_empty() {
        println!("No bundles found in the registry cache.");
        println!("Hint: Run `apm sync` to populate the registry cache.");
        return Ok(());
    }

    println!("{}", "Available bundles:".bold());
    println!();

    let mut bundles: Vec<_> = registry.bundles.values().collect();
    bundles.sort_by(|a, b| a.slug.cmp(&b.slug));

    for bundle in bundles {
        let plugin_count = bundle.plugins.len();
        println!(
            "  {} — {} ({} plugin{})",
            bundle.slug.cyan().bold(),
            bundle.description,
            plugin_count,
            if plugin_count == 1 { "" } else { "s" }
        );
    }

    println!();
    println!(
        "Install a bundle with: {}",
        "apm install --bundle <name>".bold()
    );
    println!(
        "Show bundle details with: {}",
        "apm bundles info <name>".bold()
    );

    Ok(())
}

// ── Show bundle details ───────────────────────────────────────────────────────

fn run_info(registry: &Registry, name: &str) -> Result<()> {
    let bundle = registry.find_bundle(name).ok_or_else(|| {
        anyhow::anyhow!(
            "Bundle '{}' not found.\nHint: Run `apm bundles` to see available bundles.",
            name
        )
    })?;

    println!("{}", bundle.name.bold());
    println!("  Slug:        {}", bundle.slug.cyan());
    println!("  Description: {}", bundle.description);
    println!("  Plugins ({}):", bundle.plugins.len());
    for plugin in &bundle.plugins {
        // Try to enrich with registry info.
        let label = match registry.find(plugin) {
            Some(def) => format!("{} — {} v{}", plugin.cyan(), def.vendor, def.version),
            None => format!("{} {}", plugin.cyan(), "(not in registry cache)".dimmed()),
        };
        println!("    - {label}");
    }
    println!();
    println!(
        "Install this bundle with: {}",
        format!("apm install --bundle {}", bundle.slug).bold()
    );

    Ok(())
}
