mod db;
mod routes;

use std::net::SocketAddr;

use anyhow::Context;
use axum::serve;
use tokio::net::TcpListener;
use tracing_subscriber::{prelude::*, EnvFilter};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _ = dotenvy::dotenv();

    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt().with_env_filter(env_filter).init();

    let pool = db::create_pool().await?;
    sqlx::migrate!("../../migrations").run(&pool).await?;

    let bind_addr = std::env::var("APM_SERVER_BIND_ADDR")
        .unwrap_or_else(|_| "127.0.0.1:3000".to_string())
        .parse::<SocketAddr>()
        .context("invalid APM_SERVER_BIND_ADDR")?;

    let listener = TcpListener::bind(bind_addr).await?;
    tracing::info!(%bind_addr, "server listening");

    serve(listener, routes::router(pool)).await?;
    Ok(())
}
