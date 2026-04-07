use anyhow::Result;
use colored::Colorize;
use serde::Serialize;

use apm_core::{
    config::Config,
    state::{InstallState, InstalledPlugin},
    Registry,
};

use crate::license_cache::LicenseCache;

#[derive(Serialize)]
struct InstalledPluginJson {
    name: String,
    version: String,
    formats: Vec<String>,
    paths: Vec<String>,
    license_status: String,
}

pub async fn run(config: &Config, json: bool) -> Result<()> {
    let state = InstallState::load(config)?;

    if state.plugins.is_empty() {
        if json {
            println!("[]");
        } else {
            println!("No plugins installed via apm. Use 'apm install <plugin>' to get started.");
        }
        return Ok(());
    }

    let registry = Registry::load_all_sources(config).ok();
    let cache = if config.license_cache_db_path().exists() {
        Some(LicenseCache::open(config)?)
    } else {
        None
    };

    if json {
        let results: Vec<InstalledPluginJson> = state
            .plugins
            .iter()
            .map(|plugin| {
                Ok(InstalledPluginJson {
                    name: plugin.name.clone(),
                    version: plugin.version.clone(),
                    formats: plugin
                        .formats
                        .iter()
                        .map(|f| f.format.to_string())
                        .collect(),
                    paths: plugin
                        .formats
                        .iter()
                        .map(|f| f.path.to_string_lossy().into_owned())
                        .collect(),
                    license_status: license_annotation(plugin, registry.as_ref(), cache.as_ref())?,
                })
            })
            .collect::<Result<_>>()?;
        println!("{}", serde_json::to_string_pretty(&results)?);
        return Ok(());
    }

    const HDR_NAME: &str = "Name";
    const HDR_VER: &str = "Version";
    const HDR_FMT: &str = "Format";
    const HDR_LICENSE: &str = "License";
    const HDR_PATH: &str = "Path";

    let rows: Vec<_> = state
        .plugins
        .iter()
        .map(|plugin| {
            Ok((
                plugin,
                format_label(plugin),
                license_annotation(plugin, registry.as_ref(), cache.as_ref())?,
            ))
        })
        .collect::<Result<_>>()?;

    let w_name = rows
        .iter()
        .map(|(plugin, _, _)| plugin.name.len())
        .max()
        .unwrap_or(0)
        .max(HDR_NAME.len());
    let w_ver = rows
        .iter()
        .map(|(plugin, _, _)| plugin.version.len())
        .max()
        .unwrap_or(0)
        .max(HDR_VER.len());
    let w_fmt = rows
        .iter()
        .map(|(_, format, _)| format.len())
        .max()
        .unwrap_or(0)
        .max(HDR_FMT.len());
    let w_license = rows
        .iter()
        .map(|(_, _, license)| license.len())
        .max()
        .unwrap_or(0)
        .max(HDR_LICENSE.len());

    println!(
        "{}",
        format!(
            "{:<w_name$}  {:<w_ver$}  {:<w_fmt$}  {:<w_license$}  {}",
            HDR_NAME, HDR_VER, HDR_FMT, HDR_LICENSE, HDR_PATH,
        )
        .bold()
    );

    let rule_len = w_name + 2 + w_ver + 2 + w_fmt + 2 + w_license + 2 + HDR_PATH.len();
    println!("{}", "\u{2500}".repeat(rule_len).dimmed());

    for (plugin, fmt_label, license) in &rows {
        let path_str = plugin
            .formats
            .first()
            .and_then(|f| f.path.parent())
            .map(display_path)
            .unwrap_or_default();

        println!(
            "{:<w_name$}  {:<w_ver$}  {:<w_fmt$}  {:<w_license$}  {}",
            plugin.name.bold().to_string(),
            plugin.version.cyan().to_string(),
            fmt_label,
            human_license_label(license),
            path_str.dimmed(),
        );
    }

    println!();
    println!(
        "{}",
        format!(
            "{} plugin{} managed by apm.",
            state.plugins.len(),
            if state.plugins.len() == 1 { "" } else { "s" }
        )
        .dimmed()
    );

    Ok(())
}

fn license_annotation(
    plugin: &InstalledPlugin,
    registry: Option<&Registry>,
    cache: Option<&LicenseCache>,
) -> Result<String> {
    let cached_status = match cache {
        Some(cache) => cache
            .load_license(&plugin.name)?
            .map(|license| license.status),
        None => None,
    };

    let is_paid = registry
        .and_then(|registry| registry.find(&plugin.name))
        .map(|plugin| plugin.is_paid)
        .unwrap_or(cached_status.is_some());

    if !is_paid {
        return Ok("free".to_string());
    }

    Ok(match cached_status.as_deref() {
        Some("active") => "licensed".to_string(),
        Some("refunded") => "refunded".to_string(),
        Some("revoked") => "revoked".to_string(),
        Some(other) => other.to_string(),
        None => "no_license".to_string(),
    })
}

fn human_license_label(status: &str) -> String {
    match status {
        "free" => "free".dimmed().to_string(),
        "licensed" => "licensed".green().to_string(),
        "refunded" => "refunded".yellow().to_string(),
        "revoked" => "revoked".red().to_string(),
        "no_license" => "no_license".red().to_string(),
        other => other.to_string(),
    }
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

fn display_path(path: &std::path::Path) -> String {
    let path_str = path.to_string_lossy();
    if let Some(home) = dirs::home_dir() {
        let home_str = home.to_string_lossy();
        if let Some(rest) = path_str.strip_prefix(home_str.as_ref()) {
            return format!("~{rest}");
        }
    }
    path_str.into_owned()
}
