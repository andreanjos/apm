use anyhow::Result;
use serde::Serialize;

use crate::auth::credential::CredentialStore;

#[derive(Debug, Serialize)]
struct LogoutResponse {
    logged_out: bool,
}

pub async fn run(json: bool) -> Result<()> {
    let store = CredentialStore::from_env();
    store.clear_session()?;
    store.clear_api_keys()?;

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&LogoutResponse { logged_out: true })?
        );
    } else {
        println!("Logged out. Stored session tokens and API keys were removed.");
    }

    Ok(())
}
