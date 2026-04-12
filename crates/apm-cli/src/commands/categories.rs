// categories command — list catalog categories and subcategories with counts.

use std::collections::BTreeMap;

use anyhow::Result;
use colored::Colorize;
use serde::Serialize;

use apm_core::config::Config;
use apm_core::registry::Registry;

// ── JSON types ───────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct CategoriesJson {
    categories: Vec<CategoryJson>,
}

#[derive(Serialize)]
struct CategoryJson {
    name: String,
    count: usize,
    subcategories: Vec<SubcategoryJson>,
}

#[derive(Serialize)]
struct SubcategoryJson {
    name: String,
    count: usize,
}

// ── Entry point ──────────────────────────────────────────────────────────────

pub async fn run(config: &Config, json: bool) -> Result<()> {
    let registry = Registry::load_all_sources(config)?;

    if registry.is_empty() {
        if json {
            println!(r#"{{"categories":[]}}"#);
        } else {
            println!("Registry cache is empty. Run `apm sync` to download the plugin registry.");
        }
        return Ok(());
    }

    // Accumulate counts per (category, subcategory).
    // BTreeMap gives us sorted keys for free.
    let mut categories: BTreeMap<String, BTreeMap<String, usize>> = BTreeMap::new();

    for plugin in registry.plugins.values() {
        let cat = plugin.category.to_lowercase();
        let subs = categories.entry(cat).or_default();

        if let Some(ref sub) = plugin.subcategory {
            if !sub.is_empty() {
                *subs.entry(sub.to_lowercase()).or_insert(0) += 1;
            }
        }
    }

    // Also compute total count per category (may exceed sum of subcategories
    // when some catalog records have no subcategory).
    let mut category_totals: BTreeMap<String, usize> = BTreeMap::new();
    for plugin in registry.plugins.values() {
        *category_totals
            .entry(plugin.category.to_lowercase())
            .or_insert(0) += 1;
    }

    if json {
        print_json(&categories, &category_totals)?;
    } else {
        print_human(&categories, &category_totals);
    }

    Ok(())
}

// ── JSON output ──────────────────────────────────────────────────────────────

fn print_json(
    categories: &BTreeMap<String, BTreeMap<String, usize>>,
    totals: &BTreeMap<String, usize>,
) -> Result<()> {
    let cats: Vec<CategoryJson> = categories
        .iter()
        .map(|(name, subs)| {
            let subcategories: Vec<SubcategoryJson> = subs
                .iter()
                .map(|(sub_name, &count)| SubcategoryJson {
                    name: sub_name.clone(),
                    count,
                })
                .collect();

            CategoryJson {
                name: name.clone(),
                count: *totals.get(name).unwrap_or(&0),
                subcategories,
            }
        })
        .collect();

    let output = CategoriesJson { categories: cats };
    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

// ── Human-readable output ────────────────────────────────────────────────────

fn print_human(
    categories: &BTreeMap<String, BTreeMap<String, usize>>,
    totals: &BTreeMap<String, usize>,
) {
    // Determine the widest category/subcategory name for alignment.
    let max_cat_width = categories.keys().map(|k| k.len()).max().unwrap_or(0);
    let max_sub_width = categories
        .values()
        .flat_map(|subs| subs.keys().map(|k| k.len()))
        .max()
        .unwrap_or(0);

    // Find widest count for right-alignment.
    let max_count = totals.values().copied().max().unwrap_or(0);
    let count_width = max_count.to_string().len();

    println!("{}", "Categories:".bold());
    println!();

    for (cat_name, subs) in categories {
        let total = totals.get(cat_name).unwrap_or(&0);
        let item_word = if *total == 1 { "item" } else { "items" };
        println!(
            "  {:<width$}  {:>cw$} {}",
            cat_name.bold().to_string(),
            total,
            item_word.dimmed(),
            width = max_cat_width,
            cw = count_width,
        );

        // Sort subcategories by count descending, then name ascending.
        let mut sorted_subs: Vec<(&String, &usize)> = subs.iter().collect();
        sorted_subs.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));

        for (sub_name, count) in &sorted_subs {
            println!(
                "    {:<width$}  {:>cw$}",
                sub_name,
                count,
                width = max_sub_width,
                cw = count_width,
            );
        }
    }

    // Summary line.
    let total_items: usize = totals.values().sum();
    let total_cats = categories.len();
    println!();
    println!(
        "{}",
        format!(
            "{total_items} catalog items across {total_cats} {}.",
            if total_cats == 1 {
                "category"
            } else {
                "categories"
            }
        )
        .dimmed()
    );
}
