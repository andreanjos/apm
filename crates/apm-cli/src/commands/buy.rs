use std::{fs, path::PathBuf, process::Command, time::Duration};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use apm_core::{config::Config, LicenseStatus};

use crate::api::commerce::{AgentPurchaseDenial, CommerceHttpClient, OrderStatusResponse};
use crate::license_cache::LicenseCache;

const INTENT_TTL_HOURS: i64 = 24;

#[derive(Debug, Serialize)]
struct BuyOutput {
    plugin_slug: String,
    order_id: i64,
    status: String,
    checkout_session_id: String,
    checkout_url: String,
    idempotency_key: String,
    browser_opened: bool,
    reused_intent: bool,
    fulfilled: bool,
    install_ready: bool,
    license_token: Option<String>,
    download_token: Option<String>,
}

#[derive(Debug, Serialize)]
struct AgentBuyOutput {
    transaction_id: String,
    plugin_slug: String,
    order_id: i64,
    status: String,
    fulfilled: bool,
    install_ready: bool,
    cost_cents: i64,
    currency: String,
    license_token: Option<String>,
    download_token: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct StoredPurchaseIntent {
    plugin_slug: String,
    idempotency_key: String,
    created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize)]
struct StoredOrderRecord {
    plugin_slug: String,
    order_id: i64,
    status: String,
    updated_at: DateTime<Utc>,
}

pub async fn run(config: &Config, plugin: &str, confirm: bool, json: bool) -> Result<()> {
    let client = CommerceHttpClient::from_env();
    let existing_intent = load_intent(config, plugin)?;
    let reused_intent = existing_intent.is_some();
    let intent = existing_intent.unwrap_or_else(|| StoredPurchaseIntent {
        plugin_slug: plugin.to_string(),
        idempotency_key: format!("buy-{plugin}-{}", Utc::now().timestamp_millis()),
        created_at: Utc::now(),
    });

    save_intent(config, &intent)?;

    if confirm {
        return run_agent_buy(config, plugin, json, &client, &intent.idempotency_key).await;
    }

    let checkout = client
        .create_checkout(plugin, &intent.idempotency_key)
        .await?;
    let browser_opened = open_checkout_url(&checkout.checkout_url);
    let order = wait_for_fulfillment(&client, checkout.order_id).await?;
    let fulfilled = order.status == "fulfilled";
    let install_ready = fulfilled && order.license.is_some() && order.download_token.is_some();

    if install_ready {
        clear_intent(config, plugin)?;
    }
    save_order_record(
        config,
        &StoredOrderRecord {
            plugin_slug: order.plugin_slug.clone(),
            order_id: order.order_id,
            status: order.status.clone(),
            updated_at: Utc::now(),
        },
    )?;
    if let Some(license) = &order.license {
        persist_license(config, &client, license).await?;
    }

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&BuyOutput {
                plugin_slug: order.plugin_slug,
                order_id: order.order_id,
                status: order.status,
                checkout_session_id: order.checkout_session_id,
                checkout_url: checkout.checkout_url,
                idempotency_key: checkout.idempotency_key,
                browser_opened,
                reused_intent,
                fulfilled,
                install_ready,
                license_token: order.license_token,
                download_token: order.download_token,
            })?
        );
        return Ok(());
    }

    println!("Checkout created for '{plugin}'.");
    println!("Checkout URL: {}", checkout.checkout_url);
    if browser_opened {
        println!("Opened the hosted checkout page in your browser.");
    } else {
        println!("Browser launch skipped. Open the checkout URL manually if needed.");
    }

    match order.status.as_str() {
        "fulfilled" => {
            println!("Order {} is fulfilled.", order.order_id);
            if install_ready {
                println!(
                    "Install-ready entitlement received for '{}'.",
                    order.plugin_slug
                );
                println!("Starting plugin install...");
                crate::commands::install::run(
                    config,
                    std::slice::from_ref(&order.plugin_slug),
                    None,
                    None,
                    None,
                    None,
                    false,
                    None,
                )
                .await?;
            }
        }
        "pending" => {
            println!(
                "Order {} is still pending. Re-run `apm buy {}` to keep waiting on the same purchase intent.",
                order.order_id, plugin
            );
        }
        other => {
            println!("Order {} is currently '{}'.", order.order_id, other);
        }
    }

    Ok(())
}

async fn run_agent_buy(
    config: &Config,
    plugin: &str,
    json: bool,
    client: &CommerceHttpClient,
    idempotency_key: &str,
) -> Result<()> {
    match client
        .create_agent_purchase(plugin, idempotency_key)
        .await?
    {
        Ok(mut response) => {
            if response.status == "pending" {
                let order = wait_for_fulfillment(client, response.order_id).await?;
                response.status = order.status.clone();
                response.fulfilled = order.status == "fulfilled";
                response.install_ready =
                    response.fulfilled && order.license.is_some() && order.download_token.is_some();
                response.license_token = order.license_token;
                response.license = order.license;
                response.download_token = order.download_token;
            }

            if response.install_ready {
                clear_intent(config, plugin)?;
            }

            save_order_record(
                config,
                &StoredOrderRecord {
                    plugin_slug: response.plugin_slug.clone(),
                    order_id: response.order_id,
                    status: response.status.clone(),
                    updated_at: Utc::now(),
                },
            )?;
            if let Some(license) = &response.license {
                persist_license(config, client, license).await?;
            }

            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&AgentBuyOutput {
                        transaction_id: response.transaction_id,
                        plugin_slug: response.plugin_slug,
                        order_id: response.order_id,
                        status: response.status,
                        fulfilled: response.fulfilled,
                        install_ready: response.install_ready,
                        cost_cents: response.cost_cents,
                        currency: response.currency,
                        license_token: response.license_token,
                        download_token: response.download_token,
                    })?
                );
                return Ok(());
            }

            println!("Agent purchase confirmed for '{plugin}'.");
            println!("Transaction: {}", response.transaction_id);
            println!(
                "Status: {} (cost: {} {}.{:02})",
                response.status,
                response.currency.to_uppercase(),
                response.cost_cents / 100,
                response.cost_cents.abs() % 100
            );
            if response.install_ready {
                println!("Install-ready entitlement received.");
            }
            Ok(())
        }
        Err(denial) => render_agent_denial(denial, json),
    }
}

async fn wait_for_fulfillment(
    client: &CommerceHttpClient,
    order_id: i64,
) -> Result<OrderStatusResponse> {
    let max_polls = std::env::var("APM_BUY_MAX_POLLS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(10);
    let poll_interval_ms = std::env::var("APM_BUY_POLL_INTERVAL_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(1_000);

    let mut last = client.order_status(order_id).await?;
    for _ in 0..max_polls {
        if last.status != "pending" {
            return Ok(last);
        }
        tokio::time::sleep(Duration::from_millis(poll_interval_ms)).await;
        last = client.order_status(order_id).await?;
    }

    Ok(last)
}

fn open_checkout_url(url: &str) -> bool {
    if std::env::var("APM_TEST_SKIP_BROWSER").as_deref() == Ok("1") {
        return false;
    }

    Command::new("open")
        .arg(url)
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

async fn persist_license(
    config: &Config,
    client: &CommerceHttpClient,
    license: &apm_core::SignedLicense,
) -> Result<()> {
    let cache = LicenseCache::open(config)?;
    if let Ok(sync) = client.sync_licenses().await {
        for entry in sync.licenses {
            cache.upsert_license(&entry.status, &sync.public_key_hex, &entry.license)?;
        }
    } else {
        let public_key_hex = std::env::var("APM_LICENSE_PUBLIC_KEY")
            .context("APM_LICENSE_PUBLIC_KEY is required when license sync is unavailable")?;
        cache.upsert_license(
            license_status_name(&license.payload.status),
            &public_key_hex,
            license,
        )?;
    }
    Ok(())
}

fn render_agent_denial(denial: AgentPurchaseDenial, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(&denial)?);
    } else {
        println!("Agent purchase denied: {}", denial.message);
        if let Some(details) = denial.details {
            println!("{}", serde_json::to_string_pretty(&details)?);
        }
    }
    anyhow::bail!("agent purchase denied: {}", denial.code)
}

fn load_intent(config: &Config, plugin: &str) -> Result<Option<StoredPurchaseIntent>> {
    let path = intent_path(config, plugin);
    if !path.exists() {
        return Ok(None);
    }

    let raw = fs::read_to_string(&path)
        .with_context(|| format!("failed to read purchase intent {}", path.display()))?;
    let intent: StoredPurchaseIntent = serde_json::from_str(&raw)
        .with_context(|| format!("failed to parse purchase intent {}", path.display()))?;

    if intent.created_at + chrono::Duration::hours(INTENT_TTL_HOURS) <= Utc::now() {
        clear_intent(config, plugin)?;
        return Ok(None);
    }

    Ok(Some(intent))
}

fn save_intent(config: &Config, intent: &StoredPurchaseIntent) -> Result<()> {
    let path = intent_path(config, &intent.plugin_slug);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }

    fs::write(&path, serde_json::to_string_pretty(intent)?)
        .with_context(|| format!("failed to write purchase intent {}", path.display()))?;
    Ok(())
}

fn clear_intent(config: &Config, plugin: &str) -> Result<()> {
    let path = intent_path(config, plugin);
    if path.exists() {
        fs::remove_file(&path)
            .with_context(|| format!("failed to delete purchase intent {}", path.display()))?;
    }
    Ok(())
}

fn intent_path(config: &Config, plugin: &str) -> PathBuf {
    config
        .resolved_data_dir()
        .join("commerce")
        .join("intents")
        .join(format!("{plugin}.json"))
}

fn license_status_name(status: &LicenseStatus) -> &'static str {
    match status {
        LicenseStatus::Active => "active",
        LicenseStatus::Refunded => "refunded",
        LicenseStatus::Revoked => "revoked",
    }
}

pub(crate) fn load_order_record(config: &Config, plugin: &str) -> Result<Option<(i64, String)>> {
    let path = order_record_path(config, plugin);
    if !path.exists() {
        return Ok(None);
    }

    let raw = fs::read_to_string(&path)
        .with_context(|| format!("failed to read order record {}", path.display()))?;
    let record: StoredOrderRecord = serde_json::from_str(&raw)
        .with_context(|| format!("failed to parse order record {}", path.display()))?;
    Ok(Some((record.order_id, record.status)))
}

fn save_order_record(config: &Config, record: &StoredOrderRecord) -> Result<()> {
    let path = order_record_path(config, &record.plugin_slug);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }

    fs::write(&path, serde_json::to_string_pretty(record)?)
        .with_context(|| format!("failed to write order record {}", path.display()))?;
    Ok(())
}

fn order_record_path(config: &Config, plugin: &str) -> PathBuf {
    config
        .resolved_data_dir()
        .join("commerce")
        .join("orders")
        .join(format!("{plugin}.json"))
}
