use anyhow::Result;

use crate::config::Config;
use crate::error::ApmError;

pub async fn run(_config: &Config) -> Result<()> {
    Err(ApmError::not_implemented("sync", "Phase 3").into())
}
