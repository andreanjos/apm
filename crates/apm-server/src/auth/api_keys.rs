use sha2::{Digest, Sha256};

use crate::auth::device_flow::generate_secret_token;

pub fn generate_api_key() -> String {
    let prefix = &generate_secret_token()[..10];
    let secret = generate_secret_token();
    format!("apm_live_{prefix}_{secret}")
}

pub fn hash_secret(secret: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(secret.as_bytes());
    hex::encode(hasher.finalize())
}

pub fn key_prefix(secret: &str) -> String {
    secret.split('_').nth(2).unwrap_or("unknown").to_string()
}
