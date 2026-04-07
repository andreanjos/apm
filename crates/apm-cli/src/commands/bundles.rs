// bundles command — list and inspect plugin bundles (meta-packages).

use anyhow::Result;
use colored::Colorize;

use apm_core::config::Config;
use apm_core::registry::Registry;

// ── Entry point ───────────────────────────────────────────────────────────────

pub async fn run(config: &Config, info_name: Option<&str>, json: bool) -> Result<()> {
    let registry = Registry::load_all_sources(config)?;

    if let Some(name) = info_name {
        run_info(&registry, name, json)
    } else {
        run_list(&registry, json)
    }
}

// ── List all bundles ──────────────────────────────────────────────────────────

fn run_list(registry: &Registry, json: bool) -> Result<()> {
    if registry.bundles.is_empty() {
        if json {
            println!("{}", serde_json::json!({ "bundles": [] }));
        } else {
            println!("No bundles found in the registry cache.");
            println!("Hint: Run `apm sync` to populate the registry cache.");
        }
        return Ok(());
    }

    let mut bundles: Vec<_> = registry.bundles.values().collect();
    bundles.sort_by(|a, b| a.slug.cmp(&b.slug));

    if json {
        let entries: Vec<serde_json::Value> = bundles
            .iter()
            .map(|b| {
                serde_json::json!({
                    "slug": b.slug,
                    "name": b.name,
                    "description": b.description,
                    "plugin_count": b.plugins.len(),
                })
            })
            .collect();
        println!("{}", serde_json::json!({ "bundles": entries }));
        return Ok(());
    }

    println!("{}", "Available bundles:".bold());
    println!();

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

fn run_info(registry: &Registry, name: &str, json: bool) -> Result<()> {
    let bundle = registry.find_bundle(name).ok_or_else(|| {
        anyhow::anyhow!(
            "Bundle '{}' not found.\nHint: Run `apm bundles` to see available bundles.",
            name
        )
    })?;

    if json {
        println!(
            "{}",
            serde_json::json!({
                "slug": bundle.slug,
                "name": bundle.name,
                "description": bundle.description,
                "plugins": bundle.plugins,
            })
        );
        return Ok(());
    }

    println!("{}", bundle.name.bold());
    println!("  Slug:        {}", bundle.slug.cyan());
    println!("  Description: {}", bundle.description);
    println!("  Plugins ({}):", bundle.plugins.len());

    let mut missing: Vec<&str> = Vec::new();

    for plugin in &bundle.plugins {
        // Try to enrich with registry info.
        let label = match registry.find(plugin) {
            Some(def) => format!("{} — {} v{}", plugin.cyan(), def.vendor, def.version),
            None => {
                missing.push(plugin);
                format!("{} {}", plugin.cyan(), "(not in registry cache)".dimmed())
            }
        };
        println!("    - {label}");
    }

    if !missing.is_empty() {
        println!();
        println!(
            "  {} {} plugin{} in this bundle {} not in the registry:",
            "⚠".yellow(),
            missing.len(),
            if missing.len() == 1 { "" } else { "s" },
            if missing.len() == 1 { "is" } else { "are" }
        );
        for slug in &missing {
            println!("    - {slug}");
        }
    }

    println!();
    println!(
        "Install this bundle with: {}",
        format!("apm install --bundle {}", bundle.slug).bold()
    );

    Ok(())
}
