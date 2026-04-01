use anyhow::Result;

use crate::config::{Config, InstallScope};
use crate::error::ApmError;
use crate::registry::PluginFormat;

pub async fn run(
    _config: &Config,
    _name: &str,
    _format: Option<PluginFormat>,
    _scope: Option<InstallScope>,
) -> Result<()> {
    Err(ApmError::not_implemented("install", "Phase 4").into())
}
