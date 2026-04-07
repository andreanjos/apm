use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug)]
pub struct StripeConfig {
    pub secret_key: String,
    pub webhook_secret: String,
    pub success_url: String,
    pub cancel_url: String,
}

impl StripeConfig {
    pub fn from_env() -> Self {
        Self {
            secret_key: std::env::var("STRIPE_SECRET_KEY")
                .unwrap_or_else(|_| "sk_test_mock".to_string()),
            webhook_secret: std::env::var("STRIPE_WEBHOOK_SECRET")
                .unwrap_or_else(|_| "whsec_mock".to_string()),
            success_url: std::env::var("APM_STRIPE_SUCCESS_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:3000/commerce/success".to_string()),
            cancel_url: std::env::var("APM_STRIPE_CANCEL_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:3000/commerce/cancel".to_string()),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CheckoutSession {
    pub session_id: String,
    pub checkout_url: String,
    pub tax_enabled: bool,
    pub idempotency_key: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StripeWebhookEvent {
    pub id: String,
    pub event_type: String,
    pub checkout_session_id: Option<String>,
    pub refund_id: Option<String>,
}

pub async fn create_checkout_session(
    _config: &StripeConfig,
    plugin_slug: &str,
    amount_cents: i64,
    currency: &str,
    idempotency_key: &str,
) -> Result<CheckoutSession> {
    let session_id = format!("cs_test_{}", uuid::Uuid::new_v4());
    let checkout_url = format!(
        "https://checkout.stripe.com/pay/{session_id}?plugin={plugin_slug}&amount={amount_cents}&currency={currency}"
    );
    Ok(CheckoutSession {
        session_id,
        checkout_url,
        tax_enabled: true,
        idempotency_key: idempotency_key.to_string(),
    })
}

pub fn verify_webhook(
    _config: &StripeConfig,
    payload: &str,
    _signature: &str,
) -> Result<StripeWebhookEvent> {
    let event: StripeWebhookEvent = serde_json::from_str(payload)?;
    Ok(event)
}
