use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

use crate::cancel::{ensure_not_cancelled, CancellationToken};

use super::{ModelRunExecutionBlocker, ModelRunExecutionReadiness, ModelRunPlan};

pub trait ModelRunner {
    fn run(
        &self,
        plan: ModelRunPlan,
        cancellation: &dyn CancellationToken,
    ) -> Result<ModelRunResult>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct UnavailableModelRunner;

impl ModelRunner for UnavailableModelRunner {
    fn run(
        &self,
        plan: ModelRunPlan,
        cancellation: &dyn CancellationToken,
    ) -> Result<ModelRunResult> {
        ensure_not_cancelled(cancellation)?;
        ModelRunResult::blocked(plan)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelRunResult {
    package_id: String,
    status: ModelRunStatus,
    plan: ModelRunPlan,
    message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelRunStatus {
    Completed,
    Blocked,
}

impl ModelRunResult {
    pub fn package_id(&self) -> &str {
        &self.package_id
    }

    pub fn status(&self) -> ModelRunStatus {
        self.status
    }

    pub fn plan(&self) -> &ModelRunPlan {
        &self.plan
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn completed(plan: ModelRunPlan) -> Result<Self> {
        if !plan.execution.is_ready() {
            bail!(
                "model runner cannot complete {} while execution readiness is {}",
                plan.package_id,
                plan.execution.message()
            );
        }

        Ok(Self {
            package_id: plan.package_id.clone(),
            status: ModelRunStatus::Completed,
            message: format!(
                "{} completed; output written to {}.",
                plan.package_id, plan.output_path
            ),
            plan,
        })
    }

    pub fn blocked(plan: ModelRunPlan) -> Result<Self> {
        let message = match &plan.execution {
            ModelRunExecutionReadiness::Ready { message } => {
                bail!(
                    "model runner cannot block {} while execution readiness is {}",
                    plan.package_id,
                    message
                );
            }
            ModelRunExecutionReadiness::Blocked { message, .. } => message.clone(),
        };

        Ok(Self {
            package_id: plan.package_id.clone(),
            status: ModelRunStatus::Blocked,
            message,
            plan,
        })
    }

    pub fn terminal_error_message(&self) -> Option<&str> {
        match self.status() {
            ModelRunStatus::Completed => None,
            ModelRunStatus::Blocked => self
                .blocked_execution()
                .map(|(_, message)| message)
                .or(Some(self.message())),
        }
    }

    pub fn blocked_execution(&self) -> Option<(ModelRunExecutionBlocker, &str)> {
        match &self.plan().execution {
            ModelRunExecutionReadiness::Ready { .. } => None,
            ModelRunExecutionReadiness::Blocked { blocker, message } => {
                Some((*blocker, message.as_str()))
            }
        }
    }
}
