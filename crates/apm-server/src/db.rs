use std::time::Duration;

use anyhow::{anyhow, Result};
use sqlx::{postgres::PgPoolOptions, PgPool};

pub async fn create_pool() -> Result<PgPool> {
    let database_url = std::env::var("DATABASE_URL")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            anyhow!("DATABASE_URL must be set. Copy .env.example to .env and configure it.")
        })?;

    let pool = PgPoolOptions::new()
        .max_connections(5)
        .acquire_timeout(Duration::from_secs(3))
        .connect(&database_url)
        .await?;

    Ok(pool)
}
