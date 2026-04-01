use anyhow::Result;

use crate::config::Config;
use crate::error::ApmError;

pub async fn run(_config: &Config, _name: &str, _unpin: bool) -> Result<()> {
    Err(ApmError::not_implemented("pin", "Phase 5").into())
}
