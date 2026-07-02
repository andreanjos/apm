use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use super::{installed_package_summary, ApmEngine, InstalledPackageSummary};
use crate::state::InstallState;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PinnedPackagesRequest;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SetPackagePinRequest {
    pub slug: String,
    pub pinned: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum SetPackagePinResult {
    NotInstalled {
        slug: String,
    },
    Changed {
        package: InstalledPackageSummary,
        pinned: bool,
    },
    Unchanged {
        package: InstalledPackageSummary,
        pinned: bool,
    },
}

impl ApmEngine {
    pub fn pinned_packages(
        &self,
        _request: PinnedPackagesRequest,
    ) -> Result<Vec<InstalledPackageSummary>> {
        let state = InstallState::load(&self.config)?;
        let mut pinned: Vec<InstalledPackageSummary> = state
            .plugins
            .iter()
            .filter(|package| package.pinned)
            .map(installed_package_summary)
            .collect();
        pinned.sort_by_key(|package| package.slug.to_lowercase());
        Ok(pinned)
    }

    pub fn set_package_pin(&self, request: SetPackagePinRequest) -> Result<SetPackagePinResult> {
        let mut state = InstallState::load(&self.config)?;
        let summary = {
            let Some(package) = state.find_mut(&request.slug) else {
                return Ok(SetPackagePinResult::NotInstalled { slug: request.slug });
            };

            if package.pinned == request.pinned {
                return Ok(SetPackagePinResult::Unchanged {
                    package: installed_package_summary(package),
                    pinned: request.pinned,
                });
            }
            package.pinned = request.pinned;
            installed_package_summary(package)
        };

        state
            .save(&self.config)
            .with_context(|| format!("Failed to save pin state for '{}'", request.slug))?;

        Ok(SetPackagePinResult::Changed {
            package: summary,
            pinned: request.pinned,
        })
    }
}

#[cfg(test)]
#[path = "pin_tests.rs"]
mod tests;
