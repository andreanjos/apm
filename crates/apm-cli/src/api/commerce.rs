use anyhow::{Context, Result};
use apm_core::SignedLicense;
use reqwest::{Method, Response};
use serde::{Deserialize, Serialize};

use crate::{api::auth::AuthHttpClient, auth::credential::ResolvedCredential};

#[derive(Debug, Clone)]
pub struct CommerceHttpClient {
    client: reqwest::Client,
    base_url: String,
    auth: AuthHttpClient,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct CheckoutResponse {
    pub order_id: i64,
    pub checkout_session_id: String,
    pub checkout_url: String,
    pub idempotency_key: String,
    pub tax_enabled: bool,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct OrderStatusResponse {
    pub order_id: i64,
    pub plugin_slug: String,
    pub status: String,
    pub license_token: Option<String>,
    pub license: Option<SignedLicense>,
    pub download_token: Option<String>,
    pub checkout_session_id: String,
    pub refunded_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct RefundResponse {
    pub order_id: i64,
    pub refunded: bool,
    pub status: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct LicenseSyncResponse {
    pub public_key_hex: String,
    pub licenses: Vec<LicenseSyncItem>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct LicenseSyncItem {
    pub plugin_slug: String,
    pub order_id: i64,
    pub status: String,
    pub license: SignedLicense,
    pub activated_at: Option<chrono::DateTime<chrono::Utc>>,
    pub revoked_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct RestoreResponse {
    pub public_key_hex: String,
    pub restorable_plugins: Vec<RestoreItem>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct RestoreItem {
    pub plugin_slug: String,
    pub order_id: i64,
    pub download_token: Option<String>,
    pub license: SignedLicense,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct AgentPurchaseResponse {
    pub transaction_id: String,
    pub order_id: i64,
    pub plugin_slug: String,
    pub status: String,
    pub fulfilled: bool,
    pub install_ready: bool,
    pub cost_cents: i64,
    pub currency: String,
    pub license_token: Option<String>,
    pub license: Option<SignedLicense>,
    pub download_token: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct AgentPurchaseDenial {
    pub error: String,
    pub code: String,
    pub message: String,
    pub details: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
struct CheckoutRequest<'a> {
    plugin_slug: &'a str,
    idempotency_key: &'a str,
}

impl CommerceHttpClient {
    pub fn from_env() -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url: std::env::var("APM_SERVER_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:3000".to_string()),
            auth: AuthHttpClient::from_env(),
        }
    }

    pub async fn create_checkout(
        &self,
        plugin_slug: &str,
        idempotency_key: &str,
    ) -> Result<CheckoutResponse> {
        let response = self
            .send_authenticated(
                Method::POST,
                "/commerce/checkout",
                Some(serde_json::to_value(CheckoutRequest {
                    plugin_slug,
                    idempotency_key,
                })?),
            )
            .await?;

        Ok(response
            .error_for_status()
            .context("checkout request failed")?
            .json()
            .await?)
    }

    pub async fn order_status(&self, order_id: i64) -> Result<OrderStatusResponse> {
        let response = self
            .send_authenticated(Method::GET, &format!("/commerce/orders/{order_id}"), None)
            .await?;

        Ok(response
            .error_for_status()
            .context("order status request failed")?
            .json()
            .await?)
    }

    pub async fn create_refund(&self, order_id: i64) -> Result<RefundResponse> {
        let response = self
            .send_authenticated(
                Method::POST,
                "/commerce/refunds",
                Some(serde_json::json!({ "order_id": order_id })),
            )
            .await?;

        Ok(response
            .error_for_status()
            .context("refund request failed")?
            .json()
            .await?)
    }

    pub async fn sync_licenses(&self) -> Result<LicenseSyncResponse> {
        let response = self
            .send_authenticated(Method::GET, "/commerce/licenses", None)
            .await?;

        Ok(response
            .error_for_status()
            .context("license sync request failed")?
            .json()
            .await?)
    }

    pub async fn restore_manifest(&self) -> Result<RestoreResponse> {
        let response = self
            .send_authenticated(Method::POST, "/commerce/restore", None)
            .await?;

        Ok(response
            .error_for_status()
            .context("restore manifest request failed")?
            .json()
            .await?)
    }

    pub async fn create_agent_purchase(
        &self,
        plugin_slug: &str,
        idempotency_key: &str,
    ) -> Result<Result<AgentPurchaseResponse, AgentPurchaseDenial>> {
        let response = self
            .send_authenticated(
                Method::POST,
                "/commerce/agent/purchase",
                Some(serde_json::json!({
                    "plugin_slug": plugin_slug,
                    "idempotency_key": idempotency_key,
                })),
            )
            .await?;

        if response.status().is_success() {
            return Ok(Ok(response.json().await?));
        }

        let denial = response
            .json::<AgentPurchaseDenial>()
            .await
            .context("agent purchase denial response was not valid JSON")?;
        Ok(Err(denial))
    }

    async fn send_authenticated(
        &self,
        method: Method,
        path: &str,
        body: Option<serde_json::Value>,
    ) -> Result<Response> {
        match self.auth.store().resolve_credential()? {
            Some(ResolvedCredential::EnvApiKey(api_key)) => {
                self.send_with_api_key(method, path, &api_key, body).await
            }
            Some(ResolvedCredential::StoredApiKey(api_key)) => {
                self.send_with_api_key(method, path, &api_key.key, body)
                    .await
            }
            Some(ResolvedCredential::Session(mut session)) => {
                if session.is_expired() {
                    session = self.auth.refresh_session(&session).await?;
                }

                let mut response = self
                    .send_with_bearer(method.clone(), path, &session.access_token, body.clone())
                    .await?;

                if response.status() == reqwest::StatusCode::UNAUTHORIZED {
                    session = self.auth.refresh_session(&session).await?;
                    response = self
                        .send_with_bearer(method, path, &session.access_token, body)
                        .await?;
                }

                Ok(response)
            }
            None => anyhow::bail!("no stored or environment credentials are available"),
        }
    }

    async fn send_with_api_key(
        &self,
        method: Method,
        path: &str,
        api_key: &str,
        body: Option<serde_json::Value>,
    ) -> Result<Response> {
        let mut request = self
            .client
            .request(method, format!("{}{}", self.base_url, path))
            .header("x-apm-api-key", api_key);

        if let Some(body) = body {
            request = request.json(&body);
        }

        Ok(request.send().await?)
    }

    async fn send_with_bearer(
        &self,
        method: Method,
        path: &str,
        access_token: &str,
        body: Option<serde_json::Value>,
    ) -> Result<Response> {
        let mut request = self
            .client
            .request(method, format!("{}{}", self.base_url, path))
            .bearer_auth(access_token);

        if let Some(body) = body {
            request = request.json(&body);
        }

        Ok(request.send().await?)
    }
}
