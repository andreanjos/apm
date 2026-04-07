// export command — serialize the installed plugin list to TOML or JSON.

use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use apm_core::config::Config;
use apm_core::state::InstallState;

// ── Export record ─────────────────────────────────────────────────────────────

/// One entry in the exported plugin list.
#[derive(Debug, Serialize, Deserialize)]
pub struct ExportedPlugin {
    pub name: String,
    pub version: String,
    pub formats: Vec<String>,
    pub source: String,
}

/// Top-level export document.
#[derive(Debug, Serialize, Deserialize)]
pub struct ExportDocument {
    pub plugins: Vec<ExportedPlugin>,
}

// ── Entry point ───────────────────────────────────────────────────────────────

pub async fn run(config: &Config, output: Option<&PathBuf>, format: &str) -> Result<()> {
    let state = InstallState::load(config)?;

    let entries: Vec<ExportedPlugin> = state
        .plugins
        .iter()
        .map(|p| ExportedPlugin {
            name: p.name.clone(),
            version: p.version.clone(),
            formats: p
                .formats
                .iter()
                .map(|f| f.format.to_string().to_lowercase())
                .collect(),
            source: p.source.clone(),
        })
        .collect();

    let doc = ExportDocument { plugins: entries };

    let content = match format {
        "json" => {
            serde_json::to_string_pretty(&doc).context("Failed to serialize plugin list as JSON")?
        }
        _ => {
            // TOML with header comment.
            let mut out = String::from("# apm plugin export\n");
            for plugin in &doc.plugins {
                out.push_str("\n[[plugins]]\n");
                out.push_str(&format!("name = {:?}\n", plugin.name));
                out.push_str(&format!("version = {:?}\n", plugin.version));
                // Render formats as a TOML array.
                let fmt_array: Vec<String> =
                    plugin.formats.iter().map(|f| format!("{f:?}")).collect();
                out.push_str(&format!("formats = [{}]\n", fmt_array.join(", ")));
                out.push_str(&format!("source = {:?}\n", plugin.source));
            }
            out
        }
    };

    match output {
        Some(path) => {
            std::fs::write(path, &content)
                .with_context(|| format!("Failed to write export to {}", path.display()))?;
            println!(
                "Exported {} plugin(s) to {}",
                doc.plugins.len(),
                path.display()
            );
        }
        None => {
            print!("{content}");
        }
    }

    Ok(())
}
