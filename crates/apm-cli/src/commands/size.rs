// size command — show disk usage of installed plugins, broken down by format.

use anyhow::Result;
use colored::Colorize;
use serde::Serialize;

use apm_core::config::Config;
use apm_core::state::InstallState;

// ── JSON types ──────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct SizeJson {
    plugins: Vec<PluginSizeJson>,
    total_bytes: u64,
}

#[derive(Serialize)]
struct PluginSizeJson {
    name: String,
    total_bytes: u64,
    formats: Vec<FormatSizeJson>,
}

#[derive(Serialize)]
struct FormatSizeJson {
    format: String,
    bytes: u64,
    path: String,
}

// ── Internal types ──────────────────────────────────────────────────────────

struct FormatSize {
    format: String,
    bytes: u64,
    path: String,
}

struct PluginSize {
    name: String,
    total_bytes: u64,
    formats: Vec<FormatSize>,
}

// ── Entry point ─────────────────────────────────────────────────────────────

pub async fn run(config: &Config, json: bool) -> Result<()> {
    let state = InstallState::load(config)?;

    if state.plugins.is_empty() {
        if json {
            println!(
                "{}",
                serde_json::to_string(&SizeJson {
                    plugins: vec![],
                    total_bytes: 0,
                })?
            );
        } else {
            println!("No plugins installed.");
        }
        return Ok(());
    }

    // Compute sizes for each plugin.
    let mut plugin_sizes: Vec<PluginSize> = state
        .plugins
        .iter()
        .map(|plugin| {
            let formats: Vec<FormatSize> = plugin
                .formats
                .iter()
                .map(|f| {
                    let bytes = dir_size(&f.path);
                    FormatSize {
                        format: f.format.to_string().to_lowercase(),
                        bytes,
                        path: f.path.to_string_lossy().into_owned(),
                    }
                })
                .collect();

            let total_bytes: u64 = formats.iter().map(|f| f.bytes).sum();

            PluginSize {
                name: plugin.name.clone(),
                total_bytes,
                formats,
            }
        })
        .collect();

    // Sort by size descending (largest first).
    plugin_sizes.sort_by(|a, b| b.total_bytes.cmp(&a.total_bytes));

    let grand_total: u64 = plugin_sizes.iter().map(|p| p.total_bytes).sum();

    // ── JSON output ─────────────────────────────────────────────────────────
    if json {
        let out = SizeJson {
            plugins: plugin_sizes
                .iter()
                .map(|p| PluginSizeJson {
                    name: p.name.clone(),
                    total_bytes: p.total_bytes,
                    formats: p
                        .formats
                        .iter()
                        .map(|f| FormatSizeJson {
                            format: f.format.clone(),
                            bytes: f.bytes,
                            path: f.path.clone(),
                        })
                        .collect(),
                })
                .collect(),
            total_bytes: grand_total,
        };
        println!("{}", serde_json::to_string_pretty(&out)?);
        return Ok(());
    }

    // ── Human output ────────────────────────────────────────────────────────
    println!("\n  {}\n", "Plugin disk usage:".bold());

    // Determine the widest plugin name for alignment.
    let max_name_len = plugin_sizes
        .iter()
        .map(|p| p.name.len())
        .max()
        .unwrap_or(0);

    for p in &plugin_sizes {
        let name_padded = format!("{:<width$}", p.name, width = max_name_len);
        let total_str = format_bytes(p.total_bytes);

        let breakdown: String = p
            .formats
            .iter()
            .map(|f| format!("{}: {}", f.format.to_uppercase(), format_bytes(f.bytes)))
            .collect::<Vec<_>>()
            .join(", ");

        println!(
            "    {name_padded}  {:>10}  ({breakdown})",
            total_str
        );
    }

    let plugin_suffix = if plugin_sizes.len() == 1 {
        "plugin"
    } else {
        "plugins"
    };
    println!(
        "\n    {} across {} {plugin_suffix}\n",
        format!("Total: {}", format_bytes(grand_total)).bold(),
        plugin_sizes.len(),
    );

    Ok(())
}

// ── Helpers ─────────────────────────────────────────────────────────────────

/// Recursively sum the size of all files under `path`.
/// Returns 0 if the path does not exist.
fn dir_size(path: &std::path::Path) -> u64 {
    if !path.exists() {
        return 0;
    }

    walkdir::WalkDir::new(path)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .map(|e| e.metadata().map(|m| m.len()).unwrap_or(0))
        .sum()
}

/// Format a byte count into a human-readable string with one decimal place.
fn format_bytes(bytes: u64) -> String {
    if bytes == 0 {
        return "0 B".to_string();
    }

    const KB: f64 = 1024.0;
    const MB: f64 = 1024.0 * 1024.0;
    const GB: f64 = 1024.0 * 1024.0 * 1024.0;

    let b = bytes as f64;
    if b >= GB {
        format!("{:.1} GB", b / GB)
    } else if b >= MB {
        format!("{:.1} MB", b / MB)
    } else if b >= KB {
        format!("{:.1} KB", b / KB)
    } else {
        format!("{} B", bytes)
    }
}
