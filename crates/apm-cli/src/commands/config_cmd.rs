// config command — view apm configuration and paths.

use anyhow::Result;

use apm_core::config::{self, Config};

use crate::utils::display_path;

/// Show the current effective configuration values.
pub fn run_show(config: &Config, json: bool) -> Result<()> {
    let cfg_path = config::config_dir().join("config.toml");
    let data_dir = config.resolved_data_dir();
    let cache_dir = config.resolved_cache_dir();
    let sources = config.sources();

    let scope_str = match config.install_scope {
        apm_core::config::InstallScope::User => "user",
        apm_core::config::InstallScope::System => "system",
    };

    if json {
        let obj = serde_json::json!({
            "registry_url": config.default_registry_url,
            "install_scope": scope_str,
            "config_file": cfg_path.to_string_lossy(),
            "data_directory": data_dir.to_string_lossy(),
            "cache_directory": cache_dir.to_string_lossy(),
            "sources": sources.len(),
        });
        println!("{}", serde_json::to_string_pretty(&obj)?);
        return Ok(());
    }

    let label_width = 17;

    println!(
        "{:<label_width$}{}",
        "Registry URL:",
        config.default_registry_url
    );
    println!("{:<label_width$}{}", "Install scope:", scope_str);
    println!(
        "{:<label_width$}{}",
        "Config file:",
        display_path(&cfg_path)
    );
    println!(
        "{:<label_width$}{}",
        "Data directory:",
        display_path(&data_dir)
    );
    println!(
        "{:<label_width$}{}",
        "Cache directory:",
        display_path(&cache_dir)
    );

    // Sources summary: count with names when few.
    let source_names: Vec<&str> = sources.iter().map(|s| s.name.as_str()).collect();
    let sources_label = if source_names.len() <= 3 {
        format!("{} ({})", sources.len(), source_names.join(", "))
    } else {
        format!("{}", sources.len())
    };
    println!("{:<label_width$}{}", "Sources:", sources_label);

    Ok(())
}

/// Print just the config file path (useful for `$EDITOR $(apm config path)`).
pub fn run_path(json: bool) -> Result<()> {
    let cfg_path = config::config_dir().join("config.toml");

    if json {
        let obj = serde_json::json!({
            "config_file": cfg_path.to_string_lossy(),
        });
        println!("{}", serde_json::to_string_pretty(&obj)?);
        return Ok(());
    }

    println!("{}", cfg_path.display());
    Ok(())
}
