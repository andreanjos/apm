use anyhow::Result;
use serde::Serialize;

use apm_core::config::Config;

use crate::{api::commerce::CommerceHttpClient, commands::buy::load_order_record};

#[derive(Debug, Serialize)]
struct RefundOutput {
    order_id: i64,
    refunded: bool,
    status: String,
}

pub async fn run(config: &Config, target: &str, json: bool) -> Result<()> {
    let order_id = resolve_order_id(config, target)?;
    let client = CommerceHttpClient::from_env();
    let refunded = client.create_refund(order_id).await?;

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&RefundOutput {
                order_id: refunded.order_id,
                refunded: refunded.refunded,
                status: refunded.status,
            })?
        );
        return Ok(());
    }

    println!(
        "Refund request recorded for order {}. Current status: {}.",
        refunded.order_id, refunded.status
    );
    Ok(())
}

fn resolve_order_id(config: &Config, target: &str) -> Result<i64> {
    if let Ok(order_id) = target.parse::<i64>() {
        return Ok(order_id);
    }

    if let Some((order_id, _status)) = load_order_record(config, target)? {
        return Ok(order_id);
    }

    anyhow::bail!(
        "No local order record exists for '{}'.\nHint: Pass an order id or buy the plugin first from this machine.",
        target
    )
}
