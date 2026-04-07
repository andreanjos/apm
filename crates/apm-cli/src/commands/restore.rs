use anyhow::Result;
use serde::Serialize;

use apm_core::{config::Config, LicenseStatus};

use crate::commands::install::InstallAuthorization;
use crate::{api::commerce::CommerceHttpClient, license_cache::LicenseCache};

#[derive(Debug, Serialize)]
struct RestoreItemOutput {
    plugin_slug: String,
    order_id: i64,
    status: String,
}

#[derive(Debug, Serialize)]
struct SkippedRestoreItem {
    plugin_slug: String,
    reason: String,
}

#[derive(Debug, Serialize)]
struct RestoreOutput {
    restored_plugins: Vec<RestoreItemOutput>,
    skipped_plugins: Vec<SkippedRestoreItem>,
}

pub async fn run(config: &Config, json: bool) -> Result<()> {
    let client = CommerceHttpClient::from_env();
    let manifest = client.restore_manifest().await?;
    let cache = LicenseCache::open(config)?;
    let mut restored_plugins = Vec::new();
    let mut skipped_plugins = Vec::new();

    for item in manifest.restorable_plugins {
        cache.upsert_license(
            license_status_name(&item.license.payload.status),
            &manifest.public_key_hex,
            &item.license,
        )?;

        let plugin_slug = item.plugin_slug.clone();
        let already_installed =
            apm_core::state::InstallState::load(config)?.is_installed(&plugin_slug);
        if already_installed {
            skipped_plugins.push(SkippedRestoreItem {
                plugin_slug,
                reason: "already installed".to_string(),
            });
            continue;
        }

        let install_result = if json {
            run_install_subprocess(&item.plugin_slug)
        } else {
            crate::commands::install::run_with_authorization(
                config,
                std::slice::from_ref(&item.plugin_slug),
                None,
                None,
                None,
                None,
                false,
                None,
                InstallAuthorization::Restore,
            )
            .await
        };

        match install_result {
            Ok(()) => restored_plugins.push(RestoreItemOutput {
                plugin_slug: item.plugin_slug,
                order_id: item.order_id,
                status: license_status_name(&item.license.payload.status).to_string(),
            }),
            Err(error) => skipped_plugins.push(SkippedRestoreItem {
                plugin_slug: item.plugin_slug,
                reason: error.to_string(),
            }),
        }
    }

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&RestoreOutput {
                restored_plugins,
                skipped_plugins,
            })?
        );
        return Ok(());
    }

    if restored_plugins.is_empty() && skipped_plugins.is_empty() {
        println!("No restorable paid plugins were returned for this account.");
        return Ok(());
    }

    for plugin in &restored_plugins {
        println!(
            "Restored '{}' from order {}.",
            plugin.plugin_slug, plugin.order_id
        );
    }
    for plugin in &skipped_plugins {
        println!("Skipped '{}': {}", plugin.plugin_slug, plugin.reason);
    }

    Ok(())
}

fn license_status_name(status: &LicenseStatus) -> &'static str {
    match status {
        LicenseStatus::Active => "active",
        LicenseStatus::Refunded => "refunded",
        LicenseStatus::Revoked => "revoked",
    }
}

fn run_install_subprocess(plugin_slug: &str) -> Result<()> {
    let current_exe = std::env::current_exe()?;
    let output = std::process::Command::new(current_exe)
        .arg("install")
        .arg(plugin_slug)
        .arg("--internal-restore")
        .output()?;

    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let reason = if stderr.is_empty() { stdout } else { stderr };
    anyhow::bail!("install failed for '{}': {}", plugin_slug, reason);
}
