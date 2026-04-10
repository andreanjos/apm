use anyhow::{bail, Result};
use colored::Colorize;
use serde::Serialize;

use apm_core::{
    config::Config,
    state::{InstallState, InstalledPlugin},
};

use crate::utils::display_path;

#[derive(Serialize)]
struct FormatJson {
    format: String,
    path: String,
}

#[derive(Serialize)]
struct InstalledPluginJson {
    slug: String,
    version: String,
    vendor: String,
    formats: Vec<FormatJson>,
    installed_at: String,
    source: String,
    pinned: bool,
    origin: String,
}

pub async fn run(config: &Config, json: bool, format: Option<&str>, sort: &str) -> Result<()> {
    // Validate the sort parameter up front.
    match sort {
        "name" | "version" | "date" => {}
        other => bail!(
            "Unknown sort key '{other}'. Valid values are: name, version, date.\n\
             Hint: Use `--sort name`, `--sort version`, or `--sort date`."
        ),
    }

    // Validate the format parameter up front.
    if let Some(f) = format {
        match f {
            "au" | "vst3" | "app" => {}
            other => bail!(
                "Unknown format '{other}'. Valid values are: au, vst3, app.\n\
                 Hint: Use `--format au`, `--format vst3`, or omit the flag to show all."
            ),
        }
    }

    let state = InstallState::load(config)?;

    // Filter by format if requested.
    let mut plugins: Vec<&InstalledPlugin> = state
        .plugins
        .iter()
        .filter(|plugin| match format {
            Some("au") => plugin
                .formats
                .iter()
                .any(|f| f.format.to_string().eq_ignore_ascii_case("au")),
            Some("vst3") => plugin
                .formats
                .iter()
                .any(|f| f.format.to_string().eq_ignore_ascii_case("vst3")),
            Some("app") => plugin
                .formats
                .iter()
                .any(|f| f.format.to_string().eq_ignore_ascii_case("app")),
            _ => true,
        })
        .collect();

    // Sort.
    match sort {
        "name" => plugins.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase())),
        "version" => plugins.sort_by(|a, b| a.version.cmp(&b.version)),
        "date" => plugins.sort_by(|a, b| b.installed_at.cmp(&a.installed_at)),
        _ => unreachable!(),
    }

    if plugins.is_empty() {
        if json {
            println!("[]");
        } else if format.is_some() {
            println!("No plugins installed matching the specified format.");
        } else {
            println!("No plugins installed via apm. Use 'apm install <plugin>' to get started.");
        }
        return Ok(());
    }

    if json {
        let results: Vec<InstalledPluginJson> = plugins
            .iter()
            .map(|plugin| InstalledPluginJson {
                slug: plugin.name.clone(),
                version: plugin.version.clone(),
                vendor: plugin.vendor.clone(),
                formats: plugin
                    .formats
                    .iter()
                    .map(|f| FormatJson {
                        format: f.format.to_string().to_lowercase(),
                        path: f.path.to_string_lossy().into_owned(),
                    })
                    .collect(),
                installed_at: plugin.installed_at.to_rfc3339(),
                source: plugin.source.clone(),
                pinned: plugin.pinned,
                origin: plugin.origin.to_string(),
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&results)?);
        return Ok(());
    }

    const HDR_NAME: &str = "Name";
    const HDR_VER: &str = "Version";
    const HDR_FMT: &str = "Format";
    const HDR_ORIGIN: &str = "Origin";
    const HDR_PATH: &str = "Path";

    let rows: Vec<_> = plugins
        .iter()
        .map(|plugin| (*plugin, format_label(plugin)))
        .collect();

    let w_name = rows
        .iter()
        .map(|(plugin, _)| plugin.name.len())
        .max()
        .unwrap_or(0)
        .max(HDR_NAME.len());
    let w_ver = rows
        .iter()
        .map(|(plugin, _)| plugin.version.len())
        .max()
        .unwrap_or(0)
        .max(HDR_VER.len());
    let w_fmt = rows
        .iter()
        .map(|(_, format)| format.len())
        .max()
        .unwrap_or(0)
        .max(HDR_FMT.len());
    let w_origin = rows
        .iter()
        .map(|(plugin, _)| plugin.origin.to_string().len())
        .max()
        .unwrap_or(0)
        .max(HDR_ORIGIN.len());

    println!(
        "{}",
        format!(
            "{:<w_name$}  {:<w_ver$}  {:<w_fmt$}  {:<w_origin$}  {}",
            HDR_NAME, HDR_VER, HDR_FMT, HDR_ORIGIN, HDR_PATH,
        )
        .bold()
    );

    let rule_len = w_name + 2 + w_ver + 2 + w_fmt + 2 + w_origin + 2 + HDR_PATH.len();
    println!("{}", "\u{2500}".repeat(rule_len).dimmed());

    for (plugin, fmt_label) in &rows {
        let path_str = plugin
            .formats
            .first()
            .and_then(|f| f.path.parent())
            .map(display_path)
            .unwrap_or_default();

        println!(
            "{:<w_name$}  {:<w_ver$}  {:<w_fmt$}  {:<w_origin$}  {}",
            plugin.name.bold().to_string(),
            plugin.version.cyan().to_string(),
            fmt_label,
            plugin.origin.to_string(),
            path_str.dimmed(),
        );
    }

    println!();
    println!(
        "{}",
        format!(
            "{} plugin{} tracked by apm.",
            plugins.len(),
            if plugins.len() == 1 { "" } else { "s" }
        )
        .dimmed()
    );

    Ok(())
}

fn format_label(plugin: &InstalledPlugin) -> String {
    let mut parts: Vec<String> = plugin
        .formats
        .iter()
        .map(|f| f.format.to_string())
        .collect();
    parts.sort();
    parts.dedup();
    parts.join("+")
}
