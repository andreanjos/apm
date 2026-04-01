use anyhow::Result;

use crate::config::Config;
use crate::error::ApmError;

pub async fn run_add(_config: &Config, _url: &str, _name: Option<&str>) -> Result<()> {
    Err(ApmError::not_implemented("sources add", "Phase 3").into())
}

pub async fn run_remove(_config: &Config, _name: &str) -> Result<()> {
    Err(ApmError::not_implemented("sources remove", "Phase 3").into())
}

pub async fn run_list(_config: &Config) -> Result<()> {
    Err(ApmError::not_implemented("sources list", "Phase 3").into())
}
