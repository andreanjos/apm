use anyhow::Result;

use crate::config::Config;
use crate::error::ApmError;

pub async fn run(_config: &Config, _name: Option<&str>) -> Result<()> {
    Err(ApmError::not_implemented("upgrade", "Phase 5").into())
}
