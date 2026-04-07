use std::collections::HashMap;

use anyhow::Result;
use colored::Colorize;
use serde::Serialize;

use apm_core::config::Config;
use apm_core::registry;

/// JSON-serializable tag entry.
#[derive(Serialize)]
struct TagEntry {
    tag: String,
    count: usize,
}

/// Maximum number of tags to display in human-readable output.
const DEFAULT_DISPLAY_LIMIT: usize = 50;

pub async fn run(config: &Config, json: bool) -> Result<()> {
    let registry = registry::Registry::load_all_sources(config)?;

    if registry.is_empty() {
        if json {
            println!("{}", serde_json::json!({ "tags": [] }));
        } else {
            println!("Registry cache is empty. Run `apm sync` to download the plugin registry.");
        }
        return Ok(());
    }

    // Collect all tags and count occurrences.
    let mut tag_counts: HashMap<String, usize> = HashMap::new();
    for plugin in registry.plugins.values() {
        for tag in &plugin.tags {
            let normalised = tag.trim().to_lowercase();
            if !normalised.is_empty() {
                *tag_counts.entry(normalised).or_insert(0) += 1;
            }
        }
    }

    // Sort by frequency (descending), then alphabetically for ties.
    let mut sorted: Vec<(String, usize)> = tag_counts.into_iter().collect();
    sorted.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));

    let total_unique = sorted.len();

    // ── JSON output ──────────────────────────────────────────────────────────
    if json {
        let entries: Vec<TagEntry> = sorted
            .into_iter()
            .map(|(tag, count)| TagEntry { tag, count })
            .collect();
        let wrapper = serde_json::json!({ "tags": entries });
        println!("{}", serde_json::to_string_pretty(&wrapper)?);
        return Ok(());
    }

    // ── Human-readable word-cloud style output ───────────────────────────────
    if sorted.is_empty() {
        println!("No tags found in the registry.");
        return Ok(());
    }

    println!(
        "{}",
        format!("Tags ({total_unique} unique):").bold()
    );
    println!();

    // Display top N tags in a compact, wrapped layout.
    let display_tags: Vec<_> = sorted.iter().take(DEFAULT_DISPLAY_LIMIT).collect();

    // Build formatted entries: "tag (count)"
    let entries: Vec<String> = display_tags
        .iter()
        .map(|(tag, count)| format!("{} ({})", tag, count))
        .collect();

    // Wrap lines at ~78 columns with 2-space indent.
    let max_width: usize = 78;
    let indent = "  ";
    let mut line = String::from(indent);

    for (i, entry) in entries.iter().enumerate() {
        let separator = if i == 0 { "" } else { "  " };
        let needed = separator.len() + entry.len();

        if !line.trim().is_empty() && line.len() + needed > max_width {
            println!("{line}");
            line = String::from(indent);
        }

        if line.trim().is_empty() {
            line.push_str(entry);
        } else {
            line.push_str(separator);
            line.push_str(entry);
        }
    }
    if !line.trim().is_empty() {
        println!("{line}");
    }

    // If there are more tags than shown, note the truncation.
    if total_unique > DEFAULT_DISPLAY_LIMIT {
        println!();
        println!(
            "{}",
            format!(
                "Showing top {DEFAULT_DISPLAY_LIMIT} of {total_unique} tags. Use --json to see all."
            )
            .dimmed()
        );
    }

    Ok(())
}
