use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};

use crate::auth::{
    api_keys::StoredApiKey,
    credential::{CredentialStore, ResolvedCredential},
    session::SessionRecord,
};

#[derive(Debug, Clone)]
pub struct AuthHttpClient {
    client: reqwest::Client,
    base_url: String,
    store: CredentialStore,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct DeviceStartResponse {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub verification_uri_complete: String,
    pub interval: u64,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: chrono::DateTime<chrono::Utc>,
    pub token_type: String,
    pub user_id: i64,
    pub email: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct AuthStatusResponse {
    pub authenticated: bool,
    pub user_id: i64,
    pub email: String,
    pub source: String,
    pub scopes: Vec<String>,
}

impl AuthHttpClient {
    pub fn from_env() -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url: std::env::var("APM_SERVER_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:3000".to_string()),
            store: CredentialStore::from_env(),
        }
    }

    pub fn store(&self) -> &CredentialStore {
        &self.store
    }

    pub async fn signup(&self, email: &str, password: &str) -> Result<()> {
        self.client
            .post(format!("{}/auth/signup", self.base_url))
            .json(&serde_json::json!({ "email": email, "password": password }))
            .send()
            .await?
            .error_for_status()
            .context("signup request failed")?;
        Ok(())
    }

    pub async fn start_device_flow(&self, email: &str) -> Result<DeviceStartResponse> {
        let response = self
            .client
            .post(format!("{}/auth/device/start", self.base_url))
            .json(&serde_json::json!({ "email": email }))
            .send()
            .await?
            .error_for_status()
            .context("device start request failed")?;
        Ok(response.json().await?)
    }

    pub async fn approve_device_flow(
        &self,
        email: &str,
        password: &str,
        user_code: &str,
    ) -> Result<()> {
        self.client
            .post(format!("{}/auth/device/approve", self.base_url))
            .json(&serde_json::json!({
                "email": email,
                "password": password,
                "user_code": user_code
            }))
            .send()
            .await?
            .error_for_status()
            .context("device approval request failed")?;
        Ok(())
    }

    pub async fn poll_device_flow(&self, device_code: &str) -> Result<TokenResponse> {
        let response = self
            .client
            .post(format!("{}/auth/token/poll", self.base_url))
            .json(&serde_json::json!({ "device_code": device_code }))
            .send()
            .await?;

        if response.status() == reqwest::StatusCode::PRECONDITION_REQUIRED {
            return Err(anyhow!("authorization_pending"));
        }

        Ok(response
            .error_for_status()
            .context("device token polling failed")?
            .json()
            .await?)
    }

    pub async fn refresh_session(&self, session: &SessionRecord) -> Result<SessionRecord> {
        let refreshed: TokenResponse = self
            .client
            .post(format!("{}/auth/token/refresh", self.base_url))
            .json(&serde_json::json!({ "refresh_token": session.refresh_token }))
            .send()
            .await?
            .error_for_status()
            .context("token refresh failed")?
            .json()
            .await?;

        let updated = SessionRecord {
            access_token: refreshed.access_token,
            refresh_token: refreshed.refresh_token,
            expires_at: refreshed.expires_at,
            user_id: refreshed.user_id,
            email: refreshed.email,
        };
        self.store.save_session(&updated)?;
        Ok(updated)
    }

    pub async fn auth_status(&self) -> Result<AuthStatusResponse> {
        match self.store.resolve_credential()? {
            Some(ResolvedCredential::EnvApiKey(api_key)) => {
                self.auth_status_with_api_key(&api_key).await
            }
            Some(ResolvedCredential::StoredApiKey(api_key)) => {
                self.auth_status_with_api_key(&api_key.key).await
            }
            Some(ResolvedCredential::Session(session)) => {
                self.auth_status_with_session(session).await
            }
            None => Err(anyhow!(
                "no stored or environment credentials are available"
            )),
        }
    }

    async fn auth_status_with_api_key(&self, api_key: &str) -> Result<AuthStatusResponse> {
        let response = self
            .client
            .get(format!("{}/auth/status", self.base_url))
            .header("x-apm-api-key", api_key)
            .send()
            .await?;
        Ok(response
            .error_for_status()
            .context("API key authentication failed")?
            .json()
            .await?)
    }

    async fn auth_status_with_session(
        &self,
        mut session: SessionRecord,
    ) -> Result<AuthStatusResponse> {
        if session.is_expired() {
            session = self.refresh_session(&session).await?;
        }

        let mut response = self
            .client
            .get(format!("{}/auth/status", self.base_url))
            .bearer_auth(&session.access_token)
            .send()
            .await?;

        if response.status() == reqwest::StatusCode::UNAUTHORIZED {
            session = self.refresh_session(&session).await?;
            response = self
                .client
                .get(format!("{}/auth/status", self.base_url))
                .bearer_auth(&session.access_token)
                .send()
                .await?;
        }

        Ok(response
            .error_for_status()
            .context("session authentication failed")?
            .json()
            .await?)
    }
}

pub fn token_to_session(token: TokenResponse) -> SessionRecord {
    SessionRecord {
        access_token: token.access_token,
        refresh_token: token.refresh_token,
        expires_at: token.expires_at,
        user_id: token.user_id,
        email: token.email,
    }
}

pub fn stored_api_key(name: &str, key: &str, scopes: Vec<String>) -> StoredApiKey {
    StoredApiKey {
        name: name.to_string(),
        key: key.to_string(),
        scopes,
        created_at: chrono::Utc::now(),
    }
}
