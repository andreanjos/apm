use anyhow::Result;

use crate::config::Config;
use crate::error::ApmError;

pub async fn run(_config: &Config, _query: &str, _category: Option<&str>) -> Result<()> {
    Err(ApmError::not_implemented("search", "Phase 3").into())
}
