pub mod api_keys;
pub mod device_flow;
pub mod jwt;
pub mod password;

use anyhow::{Context, Result};

#[derive(Clone, Debug)]
pub struct AuthConfig {
    pub public_base_url: String,
    pub jwt_secret: String,
    pub access_token_ttl_seconds: i64,
    pub refresh_token_ttl_seconds: i64,
    pub device_code_ttl_seconds: i64,
}

impl AuthConfig {
    pub fn from_env() -> Result<Self> {
        Ok(Self {
            public_base_url: std::env::var("APM_SERVER_PUBLIC_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:3000".to_string()),
            jwt_secret: std::env::var("APM_SERVER_JWT_SECRET")
                .unwrap_or_else(|_| "apm-dev-jwt-secret-change-me".to_string()),
            access_token_ttl_seconds: parse_env_i64("APM_ACCESS_TOKEN_TTL_SECONDS", 900)?,
            refresh_token_ttl_seconds: parse_env_i64(
                "APM_REFRESH_TOKEN_TTL_SECONDS",
                60 * 60 * 24 * 30,
            )?,
            device_code_ttl_seconds: parse_env_i64("APM_DEVICE_CODE_TTL_SECONDS", 600)?,
        })
    }
}

fn parse_env_i64(key: &str, default: i64) -> Result<i64> {
    match std::env::var(key) {
        Ok(raw) => raw.parse::<i64>().with_context(|| format!("invalid {key}")),
        Err(_) => Ok(default),
    }
}
