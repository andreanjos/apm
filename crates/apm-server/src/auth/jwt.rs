use anyhow::{Context, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{DateTime, Utc};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessTokenClaims {
    pub sub: i64,
    pub email: String,
    pub exp: usize,
    pub iat: usize,
    pub typ: String,
}

pub fn issue_access_token(
    user_id: i64,
    email: &str,
    jwt_secret: &str,
    ttl_seconds: i64,
) -> Result<(String, DateTime<Utc>)> {
    let issued_at = Utc::now();
    let expires_at = issued_at + chrono::Duration::seconds(ttl_seconds);
    let claims = AccessTokenClaims {
        sub: user_id,
        email: email.to_string(),
        exp: expires_at.timestamp() as usize,
        iat: issued_at.timestamp() as usize,
        typ: "access".to_string(),
    };

    let header = serde_json::json!({
        "alg": "HS256",
        "typ": "JWT",
    });
    let header = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&header)?);
    let claims = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims)?);
    let payload = format!("{header}.{claims}");

    let mut mac = HmacSha256::new_from_slice(jwt_secret.as_bytes())
        .context("failed to initialize token signer")?;
    mac.update(payload.as_bytes());
    let signature = URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes());
    let token = format!("{payload}.{signature}");

    Ok((token, expires_at))
}

pub fn verify_access_token(token: &str, jwt_secret: &str) -> Result<AccessTokenClaims> {
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        anyhow::bail!("invalid access token");
    }

    let payload = format!("{}.{}", parts[0], parts[1]);
    let mut mac = HmacSha256::new_from_slice(jwt_secret.as_bytes())
        .context("failed to initialize token verifier")?;
    mac.update(payload.as_bytes());
    let expected = URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes());
    if expected != parts[2] {
        anyhow::bail!("invalid access token");
    }

    let claims: AccessTokenClaims = serde_json::from_slice(
        &URL_SAFE_NO_PAD
            .decode(parts[1])
            .context("failed to decode access token payload")?,
    )
    .context("failed to parse access token payload")?;

    if claims.typ != "access" {
        anyhow::bail!("invalid token type");
    }
    if claims.exp <= Utc::now().timestamp() as usize {
        anyhow::bail!("access token expired");
    }

    Ok(claims)
}
