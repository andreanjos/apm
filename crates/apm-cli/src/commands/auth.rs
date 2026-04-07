use anyhow::Result;
use serde::Serialize;

use crate::{
    api::auth::{stored_api_key, AuthHttpClient},
    auth::credential::{CredentialStore, ResolvedCredential},
};

#[derive(Debug, Serialize)]
struct AuthStatusOutput {
    active_source: String,
    user_id: i64,
    email: String,
    scopes: Vec<String>,
}

#[derive(Debug, Serialize)]
struct ApiKeysOutput {
    names: Vec<String>,
}

pub async fn run_set_api_key(name: &str, key: &str, scopes: &[String], json: bool) -> Result<()> {
    let store = CredentialStore::from_env();
    store.save_api_key(&stored_api_key(name, key, scopes.to_vec()))?;

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "stored": true,
                "name": name,
                "scopes": scopes,
            }))?
        );
    } else {
        println!("Stored API key '{name}'.");
    }

    Ok(())
}

pub async fn run_list_api_keys(json: bool) -> Result<()> {
    let store = CredentialStore::from_env();
    let names: Vec<String> = store
        .list_api_keys()?
        .into_iter()
        .map(|key| key.name)
        .collect();

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&ApiKeysOutput { names })?
        );
    } else if names.is_empty() {
        println!("No API keys are stored.");
    } else {
        for name in names {
            println!("{name}");
        }
    }

    Ok(())
}

pub async fn run_remove_api_key(name: &str, json: bool) -> Result<()> {
    let store = CredentialStore::from_env();
    store.remove_api_key(name)?;

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "removed": true,
                "name": name,
            }))?
        );
    } else {
        println!("Removed API key '{name}'.");
    }

    Ok(())
}

pub async fn run_status(json: bool) -> Result<()> {
    let client = AuthHttpClient::from_env();
    let status = client.auth_status().await?;

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&AuthStatusOutput {
                active_source: status.source,
                user_id: status.user_id,
                email: status.email,
                scopes: status.scopes,
            })?
        );
        return Ok(());
    }

    let source = match client.store().resolve_credential()? {
        Some(ResolvedCredential::EnvApiKey(_)) => "environment API key",
        Some(ResolvedCredential::StoredApiKey(_)) => "stored API key",
        Some(ResolvedCredential::Session(_)) => "session token",
        None => "unknown",
    };
    println!("Authenticated as {} via {}.", status.email, source);
    Ok(())
}
