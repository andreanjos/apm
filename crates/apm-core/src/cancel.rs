use anyhow::Result;

use crate::error::ApmError;

pub trait CancellationToken {
    fn cancel_requested(&self) -> bool {
        false
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct NoopCancellationToken;

impl CancellationToken for NoopCancellationToken {}

pub fn ensure_not_cancelled(cancellation: &(impl CancellationToken + ?Sized)) -> Result<()> {
    if cancellation.cancel_requested() {
        return Err(ApmError::OperationCanceled.into());
    }
    Ok(())
}
