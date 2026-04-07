use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, Utc};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LicenseStatus {
    Active,
    Refunded,
    Revoked,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LicensePayload {
    pub schema_version: u32,
    pub license_id: String,
    pub user_id: i64,
    pub plugin_slug: String,
    pub plugin_version: Option<String>,
    pub order_id: i64,
    pub issued_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
    pub status: LicenseStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedLicense {
    pub payload: LicensePayload,
    pub signature: String,
    pub key_id: String,
}

impl SignedLicense {
    pub fn signing_bytes(&self) -> Result<Vec<u8>> {
        serde_json::to_vec(&self.payload).context("failed to serialize license payload")
    }
}

pub fn verify_signed_license(public_key_hex: &str, license: &SignedLicense) -> Result<()> {
    let public_key_bytes =
        hex::decode(public_key_hex).with_context(|| "failed to decode Ed25519 public key hex")?;
    let public_key_array: [u8; 32] = public_key_bytes
        .try_into()
        .map_err(|_| anyhow!("Ed25519 public key must be 32 bytes"))?;
    let verifying_key = VerifyingKey::from_bytes(&public_key_array)
        .map_err(|error| anyhow!("invalid Ed25519 public key: {error}"))?;

    let signature_bytes = hex::decode(&license.signature)
        .with_context(|| "failed to decode license signature hex")?;
    let signature = Signature::from_slice(&signature_bytes)
        .map_err(|error| anyhow!("invalid Ed25519 signature: {error}"))?;

    verifying_key
        .verify(&license.signing_bytes()?, &signature)
        .map_err(|error| anyhow!("license signature verification failed: {error}"))
}
