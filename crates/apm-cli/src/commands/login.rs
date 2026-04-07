use anyhow::Result;
use serde::Serialize;

use apm_core::config::Config;

use crate::{api::auth::AuthHttpClient, auth::device_flow::run_login_flow};

#[derive(Debug, Serialize)]
struct LoginResponse {
    authenticated: bool,
    email: String,
}

pub async fn run(
    _config: &Config,
    email: &str,
    password: &str,
    create_account: bool,
    json: bool,
) -> Result<()> {
    let client = AuthHttpClient::from_env();
    run_login_flow(&client, email, password, create_account).await?;
    let status = client.auth_status().await?;

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&LoginResponse {
                authenticated: true,
                email: status.email,
            })?
        );
        return Ok(());
    }

    println!("Authenticated as {}.", status.email);
    Ok(())
}
