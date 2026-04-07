use anyhow::{anyhow, Context, Result};
use apm_core::license::{LicensePayload, SignedLicense};
use ed25519_dalek::{Signer, SigningKey, VerifyingKey};

const DEFAULT_SIGNING_KEY_HEX: &str =
    "1f1e1d1c1b1a19181716151413121110ffeeddccbbaa99887766554433221100";

#[derive(Clone)]
pub struct LicenseConfig {
    key_id: String,
    signing_key: SigningKey,
    public_key_hex: String,
}

impl LicenseConfig {
    pub fn from_env() -> Result<Self> {
        let private_key_hex = std::env::var("APM_LICENSE_SIGNING_KEY")
            .unwrap_or_else(|_| DEFAULT_SIGNING_KEY_HEX.to_string());
        let key_id =
            std::env::var("APM_LICENSE_KEY_ID").unwrap_or_else(|_| "dev-ed25519-1".to_string());

        let signing_key_bytes = hex::decode(&private_key_hex)
            .with_context(|| "failed to decode APM_LICENSE_SIGNING_KEY as hex")?;
        let signing_key_array: [u8; 32] = signing_key_bytes
            .try_into()
            .map_err(|_| anyhow!("APM_LICENSE_SIGNING_KEY must decode to 32 bytes"))?;
        let signing_key = SigningKey::from_bytes(&signing_key_array);
        let verifying_key: VerifyingKey = signing_key.verifying_key();
        let public_key_hex = hex::encode(verifying_key.to_bytes());

        Ok(Self {
            key_id,
            signing_key,
            public_key_hex,
        })
    }

    pub fn key_id(&self) -> &str {
        &self.key_id
    }

    pub fn public_key_hex(&self) -> &str {
        &self.public_key_hex
    }

    pub fn sign_license(&self, payload: LicensePayload) -> Result<SignedLicense> {
        let signing_bytes =
            serde_json::to_vec(&payload).context("failed to serialize license payload")?;
        let signature = self.signing_key.sign(&signing_bytes);

        Ok(SignedLicense {
            payload,
            signature: hex::encode(signature.to_bytes()),
            key_id: self.key_id.clone(),
        })
    }
}
