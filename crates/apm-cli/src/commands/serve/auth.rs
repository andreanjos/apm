use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{Context, Result};
use axum::{
    extract::{Request, State},
    http::{header::AUTHORIZATION, HeaderMap},
    middleware::Next,
    response::Response,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::{Deserialize, Serialize};

use apm_core::service::LOOPBACK_TOKEN_HEADER;

use super::ServiceHttpError;

const TOKEN_FILE_SCHEMA_VERSION: u16 = 1;
const TOKEN_BYTE_LEN: usize = 32;

#[derive(Clone)]
pub(super) struct LoopbackAuth {
    token: Arc<str>,
}

#[derive(Debug, Serialize, Deserialize)]
struct LoopbackTokenFile {
    schema_version: u16,
    header: String,
    token: String,
}

impl LoopbackAuth {
    pub(super) fn load_or_create(token_file: PathBuf) -> Result<Self> {
        let token = if token_file.exists() {
            read_token_file(&token_file)?
        } else {
            let token = generate_token()?;
            write_token_file(&token_file, &token)?;
            token
        };

        Ok(Self {
            token: Arc::from(token),
        })
    }

    fn is_authorized(&self, headers: &HeaderMap) -> bool {
        header_token(headers)
            .is_some_and(|candidate| token_eq(candidate.as_bytes(), self.token.as_bytes()))
    }
}

pub(super) async fn require_loopback_token(
    State(auth): State<LoopbackAuth>,
    request: Request,
    next: Next,
) -> Result<Response, ServiceHttpError> {
    if auth.is_authorized(request.headers()) {
        Ok(next.run(request).await)
    } else {
        Err(ServiceHttpError::unauthorized(format!(
            "Missing or invalid {LOOPBACK_TOKEN_HEADER} header"
        )))
    }
}

fn read_token_file(token_file: &Path) -> Result<String> {
    let content = fs::read_to_string(token_file).with_context(|| {
        format!(
            "Failed to read loopback token file: {}",
            token_file.display()
        )
    })?;
    let parsed: LoopbackTokenFile = serde_json::from_str(&content).with_context(|| {
        format!(
            "Failed to parse loopback token file: {}",
            token_file.display()
        )
    })?;
    if parsed.schema_version != TOKEN_FILE_SCHEMA_VERSION {
        anyhow::bail!(
            "Unsupported loopback token schema version: {}",
            parsed.schema_version
        );
    }
    if parsed.header != LOOPBACK_TOKEN_HEADER {
        anyhow::bail!("Loopback token header mismatch: {}", parsed.header);
    }
    if parsed.token.trim().is_empty() {
        anyhow::bail!("Loopback token file contains an empty token");
    }
    restrict_token_file_permissions(token_file);
    Ok(parsed.token)
}

fn write_token_file(token_file: &Path, token: &str) -> Result<()> {
    if let Some(parent) = token_file.parent() {
        apm_core::config::ensure_dir(parent).with_context(|| {
            format!(
                "Failed to create loopback token directory: {}",
                parent.display()
            )
        })?;
    }

    let content = serde_json::to_vec_pretty(&LoopbackTokenFile {
        schema_version: TOKEN_FILE_SCHEMA_VERSION,
        header: LOOPBACK_TOKEN_HEADER.to_string(),
        token: token.to_string(),
    })?;
    fs::write(token_file, content).with_context(|| {
        format!(
            "Failed to write loopback token file: {}",
            token_file.display()
        )
    })?;
    restrict_token_file_permissions(token_file);
    Ok(())
}

fn generate_token() -> Result<String> {
    let mut bytes = [0_u8; TOKEN_BYTE_LEN];
    getrandom::getrandom(&mut bytes).map_err(|error| {
        anyhow::anyhow!("Failed to read OS randomness for loopback token: {error}")
    })?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

fn header_token(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(LOOPBACK_TOKEN_HEADER)
        .and_then(|value| value.to_str().ok())
        .or_else(|| bearer_token(headers))
}

fn bearer_token(headers: &HeaderMap) -> Option<&str> {
    let value = headers.get(AUTHORIZATION)?.to_str().ok()?;
    value.strip_prefix("Bearer ")
}

fn token_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right)
        .fold(0_u8, |diff, (left, right)| diff | (left ^ right))
        == 0
}

#[cfg(unix)]
fn restrict_token_file_permissions(token_file: &Path) {
    use std::os::unix::fs::PermissionsExt;

    let permissions = fs::Permissions::from_mode(0o600);
    let _ = fs::set_permissions(token_file, permissions);
}

#[cfg(not(unix))]
fn restrict_token_file_permissions(_token_file: &Path) {}
