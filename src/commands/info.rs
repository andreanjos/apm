use anyhow::Result;

use crate::config::Config;
use crate::error::ApmError;

pub async fn run(_config: &Config, _name: &str) -> Result<()> {
    Err(ApmError::not_implemented("info", "Phase 3").into())
}
