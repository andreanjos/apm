use anyhow::Result;
use colored::Colorize;
use serde::Serialize;

use apm_core::{config::Config, license::verify_signed_license, Registry};

use crate::license_cache::{CachedLicense, LicenseCache};

#[derive(Serialize)]
struct LicenseJson {
    plugin_slug: String,
    plugin_name: String,
    order_id: i64,
    status: String,
    issued_at: chrono::DateTime<chrono::Utc>,
    revoked_at: Option<chrono::DateTime<chrono::Utc>>,
    key_id: String,
    verified: bool,
    last_synced_at: chrono::DateTime<chrono::Utc>,
}

pub async fn run(config: &Config, json: bool) -> Result<()> {
    let cache_path = config.license_cache_db_path();
    if !cache_path.exists() {
        if json {
            println!("[]");
        } else {
            println!(
                "No local license cache found.\nRun `apm buy <plugin>` or `apm restore` after authentication to populate it."
            );
        }
        return Ok(());
    }

    let cache = LicenseCache::open(config)?;
    let licenses = cache.list_licenses()?;
    if licenses.is_empty() {
        if json {
            println!("[]");
        } else {
            println!(
                "Your local license cache is empty.\nRun `apm buy <plugin>` or `apm restore` to sync ownership data."
            );
        }
        return Ok(());
    }

    let registry = Registry::load_all_sources(config).ok();
    let results: Vec<LicenseJson> = licenses
        .iter()
        .map(|license| license_to_json(license, registry.as_ref()))
        .collect();

    if json {
        println!("{}", serde_json::to_string_pretty(&results)?);
        return Ok(());
    }

    const HDR_PLUGIN: &str = "Plugin";
    const HDR_STATUS: &str = "Status";
    const HDR_ISSUED: &str = "Issued";
    const HDR_ORDER: &str = "Order";
    const HDR_VERIFIED: &str = "Verified";

    let w_plugin = results
        .iter()
        .map(|entry| entry.plugin_name.len())
        .max()
        .unwrap_or(0)
        .max(HDR_PLUGIN.len());
    let w_status = results
        .iter()
        .map(|entry| entry.status.len())
        .max()
        .unwrap_or(0)
        .max(HDR_STATUS.len());
    let w_issued = results
        .iter()
        .map(|entry| timestamp_label(entry.issued_at).len())
        .max()
        .unwrap_or(0)
        .max(HDR_ISSUED.len());
    let w_order = results
        .iter()
        .map(|entry| entry.order_id.to_string().len())
        .max()
        .unwrap_or(0)
        .max(HDR_ORDER.len());
    let w_verified = results
        .iter()
        .map(|entry| verified_label(entry.verified).len())
        .max()
        .unwrap_or(0)
        .max(HDR_VERIFIED.len());

    println!(
        "{}",
        format!(
            "{:<w_plugin$}  {:<w_status$}  {:<w_issued$}  {:<w_order$}  {}",
            HDR_PLUGIN, HDR_STATUS, HDR_ISSUED, HDR_ORDER, HDR_VERIFIED
        )
        .bold()
    );
    println!(
        "{}",
        "\u{2500}"
            .repeat(w_plugin + 2 + w_status + 2 + w_issued + 2 + w_order + 2 + w_verified)
            .dimmed()
    );

    for entry in &results {
        println!(
            "{:<w_plugin$}  {:<w_status$}  {:<w_issued$}  {:<w_order$}  {}",
            entry.plugin_name.bold().to_string(),
            status_label(&entry.status),
            timestamp_label(entry.issued_at),
            entry.order_id.to_string(),
            verified_label(entry.verified),
        );
        println!(
            "  {} {}",
            entry.plugin_slug.dimmed(),
            format!("key {}", entry.key_id).dimmed()
        );
    }

    println!();
    println!(
        "{}",
        format!(
            "{} cached license{}.",
            results.len(),
            if results.len() == 1 { "" } else { "s" }
        )
        .dimmed()
    );

    Ok(())
}

fn license_to_json(license: &CachedLicense, registry: Option<&Registry>) -> LicenseJson {
    let plugin_name = registry
        .and_then(|loaded| loaded.find(&license.plugin_slug))
        .map(|plugin| plugin.name.clone())
        .unwrap_or_else(|| license.plugin_slug.clone());

    LicenseJson {
        plugin_slug: license.plugin_slug.clone(),
        plugin_name,
        order_id: license.order_id,
        status: license.status.clone(),
        issued_at: license.license.payload.issued_at,
        revoked_at: license.license.payload.revoked_at,
        key_id: license.license.key_id.clone(),
        verified: verify_signed_license(&license.public_key_hex, &license.license).is_ok(),
        last_synced_at: license.last_synced_at,
    }
}

fn status_label(status: &str) -> String {
    match status {
        "active" => "active".green().to_string(),
        "refunded" => "refunded".yellow().to_string(),
        "revoked" => "revoked".red().to_string(),
        other => other.to_string(),
    }
}

fn verified_label(verified: bool) -> String {
    if verified {
        "yes".green().to_string()
    } else {
        "no".red().to_string()
    }
}

fn timestamp_label(timestamp: chrono::DateTime<chrono::Utc>) -> String {
    timestamp.format("%Y-%m-%d").to_string()
}
