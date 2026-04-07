use std::{process::Command, time::Duration};

use anyhow::{anyhow, Result};

use crate::api::auth::{token_to_session, AuthHttpClient};

pub async fn run_login_flow(
    client: &AuthHttpClient,
    email: &str,
    password: &str,
    create_account: bool,
) -> Result<()> {
    if create_account {
        client.signup(email, password).await?;
    }

    let device = client.start_device_flow(email).await?;

    if std::env::var("APM_TEST_SKIP_BROWSER").as_deref() != Ok("1") {
        let _ = Command::new("open")
            .arg(&device.verification_uri_complete)
            .status();
    }

    client
        .approve_device_flow(email, password, &device.user_code)
        .await?;

    for _ in 0..10 {
        match client.poll_device_flow(&device.device_code).await {
            Ok(token) => {
                client.store().save_session(&token_to_session(token))?;
                return Ok(());
            }
            Err(error) if error.to_string().contains("authorization_pending") => {
                tokio::time::sleep(Duration::from_secs(device.interval.max(1))).await;
            }
            Err(error) => return Err(error),
        }
    }

    Err(anyhow!("timed out waiting for device authorization"))
}
