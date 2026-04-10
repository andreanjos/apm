// why command — show why/how a plugin was installed by apm.

use anyhow::Result;
use colored::Colorize;
use serde::Serialize;

use apm_core::config::Config;
use apm_core::state::InstallState;

use crate::utils::display_path;

/// JSON-serializable output for `apm why`.
#[derive(Serialize)]
struct WhyJson {
    installed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    installed_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    origin: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pinned: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    formats: Option<Vec<FormatJson>>,
}

#[derive(Serialize)]
struct FormatJson {
    format: String,
    path: String,
}

pub async fn run(config: &Config, name: &str, json: bool) -> Result<()> {
    let state = InstallState::load(config)?;

    let plugin = match state.find(name) {
        Some(p) => p,
        None => {
            if json {
                let output = WhyJson {
                    installed: false,
                    name: Some(name.to_string()),
                    installed_at: None,
                    version: None,
                    source: None,
                    origin: None,
                    pinned: None,
                    formats: None,
                };
                println!("{}", serde_json::to_string_pretty(&output)?);
            } else {
                println!(
                    "Plugin '{}' is not installed by apm. Use 'apm scan' to check if it's \
                     installed manually.",
                    name
                );
            }
            return Ok(());
        }
    };

    if json {
        let formats: Vec<FormatJson> = plugin
            .formats
            .iter()
            .map(|f| FormatJson {
                format: f.format.to_string(),
                path: f.path.to_string_lossy().to_string(),
            })
            .collect();

        let output = WhyJson {
            installed: true,
            name: Some(plugin.name.clone()),
            installed_at: Some(plugin.installed_at.format("%Y-%m-%dT%H:%M:%SZ").to_string()),
            version: Some(plugin.version.clone()),
            source: Some(plugin.source.clone()),
            origin: Some(plugin.origin.to_string()),
            pinned: Some(plugin.pinned),
            formats: Some(formats),
        };
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        print_why(plugin);
    }

    Ok(())
}

fn print_why(plugin: &apm_core::state::InstalledPlugin) {
    println!("{}", plugin.name.bold());

    let timestamp = plugin.installed_at.format("%Y-%m-%d %H:%M UTC").to_string();
    println!("  {:<12}{}", "Installed:".dimmed(), timestamp);
    println!("  {:<12}{}", "Version:".dimmed(), plugin.version.cyan());
    println!("  {:<12}{}", "Source:".dimmed(), plugin.source);
    println!("  {:<12}{}", "Origin:".dimmed(), plugin.origin);
    println!(
        "  {:<12}{}",
        "Pinned:".dimmed(),
        if plugin.pinned { "yes" } else { "no" }
    );

    if !plugin.formats.is_empty() {
        println!("  {}:", "Formats".dimmed());
        let mut sorted_formats = plugin.formats.clone();
        sorted_formats.sort_by(|a, b| a.format.to_string().cmp(&b.format.to_string()));
        for f in &sorted_formats {
            println!(
                "    {:<6}{}",
                f.format.to_string().cyan(),
                display_path(&f.path)
            );
        }
    }
}
