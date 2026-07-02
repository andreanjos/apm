use anyhow::Result;
use semver::Version;
use serde::{Deserialize, Serialize};

#[cfg(feature = "reqwest")]
use super::{
    ensure_not_cancelled, EventSink, InstallPackageRequest, InstallPlanRequest, InstallPlanResult,
};
use super::{ApmEngine, InstallPackageResult, InstalledPackageSummary};
use crate::config::InstallScope;
use crate::registry::PluginFormat;
use crate::registry::Registry;
use crate::state::{InstallOrigin, InstallState};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AvailableUpdatesRequest;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum AvailableUpdatesResult {
    CatalogEmpty,
    Ready {
        installed_count: usize,
        updates: Vec<PackageUpdateSummary>,
        up_to_date_count: usize,
        pinned_count: usize,
        external_count: usize,
        missing_count: usize,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageUpdateSummary {
    pub slug: String,
    pub vendor: String,
    pub installed_version: String,
    pub available_version: String,
    pub pinned: bool,
    pub origin: InstallOrigin,
    pub action: PackageUpdateAction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PackageUpdateAction {
    Installable,
    Pinned,
    External,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdatePackageRequest {
    pub slug: String,
    #[serde(default)]
    pub format: Option<PluginFormat>,
    #[serde(default)]
    pub scope: Option<InstallScope>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum UpdatePackageResult {
    CatalogEmpty,
    NotInstalled {
        slug: String,
    },
    NotFound {
        slug: String,
    },
    UpToDate {
        slug: String,
        version: String,
    },
    Pinned {
        update: PackageUpdateSummary,
    },
    External {
        update: PackageUpdateSummary,
    },
    InstallUnavailable {
        update: PackageUpdateSummary,
        result: InstallPackageResult,
    },
    Updated {
        update: PackageUpdateSummary,
        package: InstalledPackageSummary,
    },
}

impl ApmEngine {
    pub fn available_updates(
        &self,
        _request: AvailableUpdatesRequest,
    ) -> Result<AvailableUpdatesResult> {
        let state = InstallState::load(&self.config)?;
        let installed_count = state.plugins.len();
        if installed_count == 0 {
            return Ok(AvailableUpdatesResult::Ready {
                installed_count,
                updates: Vec::new(),
                up_to_date_count: 0,
                pinned_count: 0,
                external_count: 0,
                missing_count: 0,
            });
        }

        let registry = Registry::load_all_sources(&self.config)?;
        if registry.is_empty() {
            return Ok(AvailableUpdatesResult::CatalogEmpty);
        }

        let mut updates = Vec::new();
        let mut up_to_date_count = 0;
        let mut missing_count = 0;

        for installed in &state.plugins {
            let Some(registry_package) = registry.find(&installed.name) else {
                missing_count += 1;
                continue;
            };

            let latest_release = registry_package.latest_release();
            if !is_version_newer(&installed.version, &latest_release.version) {
                up_to_date_count += 1;
                continue;
            }

            updates.push(PackageUpdateSummary {
                slug: installed.name.clone(),
                vendor: installed.vendor.clone(),
                installed_version: installed.version.clone(),
                available_version: latest_release.version,
                pinned: installed.pinned,
                origin: installed.origin,
                action: update_action(installed.pinned, installed.origin),
            });
        }

        updates.sort_by_key(|update| update.slug.to_lowercase());
        let pinned_count = updates
            .iter()
            .filter(|update| update.action == PackageUpdateAction::Pinned)
            .count();
        let external_count = updates
            .iter()
            .filter(|update| update.action == PackageUpdateAction::External)
            .count();

        Ok(AvailableUpdatesResult::Ready {
            installed_count,
            updates,
            up_to_date_count,
            pinned_count,
            external_count,
            missing_count,
        })
    }

    #[cfg(feature = "reqwest")]
    pub fn update_package(
        &self,
        request: UpdatePackageRequest,
        events: &mut impl EventSink,
    ) -> Result<UpdatePackageResult> {
        self.update_package_with_dest_resolver(request, events, crate::install::plugin_dest_dir)
    }

    #[cfg(feature = "reqwest")]
    fn update_package_with_dest_resolver(
        &self,
        request: UpdatePackageRequest,
        events: &mut impl EventSink,
        destination_dir: impl Fn(PluginFormat, InstallScope) -> std::path::PathBuf,
    ) -> Result<UpdatePackageResult> {
        ensure_not_cancelled(events)?;
        let state = InstallState::load(&self.config)?;
        let Some(installed) = state.find(&request.slug).cloned() else {
            return Ok(UpdatePackageResult::NotInstalled { slug: request.slug });
        };

        let registry = Registry::load_all_sources(&self.config)?;
        if registry.is_empty() {
            return Ok(UpdatePackageResult::CatalogEmpty);
        }

        let Some(registry_package) = registry.find(&installed.name) else {
            return Ok(UpdatePackageResult::NotFound {
                slug: installed.name,
            });
        };

        let latest_release = registry_package.latest_release();
        if !is_version_newer(&installed.version, &latest_release.version) {
            return Ok(UpdatePackageResult::UpToDate {
                slug: installed.name,
                version: installed.version,
            });
        }

        let update = PackageUpdateSummary {
            slug: installed.name.clone(),
            vendor: installed.vendor.clone(),
            installed_version: installed.version.clone(),
            available_version: latest_release.version.clone(),
            pinned: installed.pinned,
            origin: installed.origin,
            action: update_action(installed.pinned, installed.origin),
        };

        match update.action {
            PackageUpdateAction::Pinned => return Ok(UpdatePackageResult::Pinned { update }),
            PackageUpdateAction::External => return Ok(UpdatePackageResult::External { update }),
            PackageUpdateAction::Installable => {}
        }

        let tracked_formats = installed_formats(&installed);
        if tracked_formats.len() > 1 && request.format.is_some() {
            let result = self.partial_update_unavailable(&update, &installed, request.scope)?;
            return Ok(UpdatePackageResult::InstallUnavailable { update, result });
        }

        let install_request = InstallPackageRequest {
            slug: update.slug.clone(),
            version: Some(update.available_version.clone()),
            format: request
                .format
                .or_else(|| single_installed_format(&installed)),
            scope: request.scope,
            ..InstallPackageRequest::default()
        };

        let result = if tracked_formats.len() > 1 {
            self.install_package_formats_from_url_with_dest_resolver(
                InstallPackageRequest {
                    format: None,
                    ..install_request
                },
                &tracked_formats,
                events,
                destination_dir,
            )?
        } else {
            self.install_package_from_url_with_dest_resolver(
                install_request,
                events,
                destination_dir,
            )?
        };

        match result {
            InstallPackageResult::Installed { package } => {
                Ok(UpdatePackageResult::Updated { update, package })
            }
            result => Ok(UpdatePackageResult::InstallUnavailable { update, result }),
        }
    }

    #[cfg(feature = "reqwest")]
    fn partial_update_unavailable(
        &self,
        update: &PackageUpdateSummary,
        installed: &crate::state::InstalledPlugin,
        scope: Option<InstallScope>,
    ) -> Result<InstallPackageResult> {
        let plan_result = self.plan_install(InstallPlanRequest {
            slug: update.slug.clone(),
            version: Some(update.available_version.clone()),
            scope,
            ..InstallPlanRequest::default()
        })?;

        let InstallPlanResult::Plan { plan } = plan_result else {
            return Ok(InstallPackageResult::PlanUnavailable { plan: plan_result });
        };

        Ok(InstallPackageResult::FormatRequired {
            available_formats: installed_formats(installed),
            reason: "Partial update would leave tracked formats on different versions; update all tracked formats together.".to_string(),
            plan,
        })
    }
}

#[cfg(feature = "reqwest")]
fn installed_formats(installed: &crate::state::InstalledPlugin) -> Vec<PluginFormat> {
    let mut formats: Vec<PluginFormat> = installed
        .formats
        .iter()
        .map(|format| format.format)
        .collect();
    formats.sort_by_key(|format| format.to_string());
    formats.dedup();
    formats
}

#[cfg(feature = "reqwest")]
fn single_installed_format(installed: &crate::state::InstalledPlugin) -> Option<PluginFormat> {
    match installed_formats(installed).as_slice() {
        [format] => Some(*format),
        _ => None,
    }
}

fn update_action(pinned: bool, origin: InstallOrigin) -> PackageUpdateAction {
    if pinned {
        PackageUpdateAction::Pinned
    } else if origin == InstallOrigin::External {
        PackageUpdateAction::External
    } else {
        PackageUpdateAction::Installable
    }
}

pub(super) fn is_version_newer(installed: &str, candidate: &str) -> bool {
    match (Version::parse(installed), Version::parse(candidate)) {
        (Ok(installed), Ok(candidate)) => candidate > installed,
        _ => candidate != installed,
    }
}

#[cfg(test)]
#[path = "updates_tests.rs"]
mod tests;
