use std::path::PathBuf;

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};

use super::ApmEngine;
use crate::config::InstallScope;
use crate::registry::{
    DownloadType, FormatSource, InstallType, InstallerDefinition, PluginDefinition, PluginFormat,
    ProductType, Registry,
};
use crate::state::InstallState;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstallPlanRequest {
    pub slug: String,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub format: Option<PluginFormat>,
    #[serde(default)]
    pub scope: Option<InstallScope>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum InstallPlanResult {
    CatalogEmpty,
    NotFound {
        query: String,
        suggestions: Vec<String>,
    },
    NotInstallable {
        slug: String,
        name: String,
        product_type: ProductType,
    },
    VersionNotFound {
        slug: String,
        requested_version: String,
        available_versions: Vec<String>,
    },
    FormatUnavailable {
        slug: String,
        requested_format: Option<PluginFormat>,
        available_formats: Vec<PluginFormat>,
    },
    Plan {
        plan: Box<PackageInstallPlan>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstallPlanStatus {
    Ready,
    AlreadyInstalled,
    ManualRequired,
    PrivilegedInstallerRequired,
    AppStoreRequired,
    VendorInstallerAvailable,
    VendorInstallerRequired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageInstallPlan {
    pub slug: String,
    pub name: String,
    pub vendor: String,
    pub version: String,
    pub status: InstallPlanStatus,
    pub destination: Option<String>,
    pub scope: InstallScope,
    pub installed_version: Option<String>,
    pub formats: Vec<InstallPlanFormat>,
    pub installer: Option<VendorInstallerPlan>,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstallPlanFormat {
    pub format: PluginFormat,
    pub install_type: String,
    pub download_type: DownloadType,
    pub source: String,
    pub bundle_path: Option<String>,
    pub has_checksum: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VendorInstallerPlan {
    pub key: String,
    pub name: String,
    pub download_url: String,
    pub homepage: String,
    pub installed_app_path: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum InstallHandoffResult {
    Open {
        plan: Box<PackageInstallPlan>,
        handoff: InstallHandoff,
    },
    NoHandoff {
        plan: Box<PackageInstallPlan>,
        reason: String,
    },
    PlanUnavailable {
        plan: InstallPlanResult,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstallHandoff {
    pub kind: InstallHandoffKind,
    pub label: String,
    pub target: InstallHandoffTarget,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstallHandoffKind {
    ManualDownload,
    PrivilegedInstaller,
    AppStore,
    VendorApp,
    VendorDownload,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum InstallHandoffTarget {
    App { path: PathBuf },
    Url { url: String },
}

impl ApmEngine {
    pub fn plan_install(&self, request: InstallPlanRequest) -> Result<InstallPlanResult> {
        let registry = Registry::load_all_sources(&self.config)?;
        if registry.is_empty() {
            return Ok(InstallPlanResult::CatalogEmpty);
        }

        let Some(package) = registry.find(&request.slug) else {
            return Ok(InstallPlanResult::NotFound {
                query: request.slug.clone(),
                suggestions: install_suggestions(&registry, &request.slug),
            });
        };

        if !package.is_installable_product() {
            return Ok(InstallPlanResult::NotInstallable {
                slug: package.slug.clone(),
                name: package.name.clone(),
                product_type: package.product_type.clone(),
            });
        }

        let Some(selected_release) = package.resolve_release(request.version.as_deref()) else {
            return Ok(InstallPlanResult::VersionNotFound {
                slug: package.slug.clone(),
                requested_version: request.version.unwrap_or_default(),
                available_versions: package.available_versions(),
            });
        };

        let mut selected_package = package.clone();
        selected_package.version = selected_release.version;
        selected_package.formats = selected_release.formats;

        let formats = selected_formats(&selected_package, request.format);
        if formats.is_empty() {
            return Ok(InstallPlanResult::FormatUnavailable {
                slug: package.slug.clone(),
                requested_format: request.format,
                available_formats: available_formats(&selected_package),
            });
        }

        let state = InstallState::load(&self.config)?;
        let installed = state.find(&package.slug);
        let installed_version = installed.map(|entry| entry.version.clone());
        let effective_scope = request.scope.unwrap_or(self.config.install_scope);

        let already_installed = installed.is_some_and(|entry| {
            let already_has_format = match request.format {
                Some(format) => entry.formats.iter().any(|entry| entry.format == format),
                None => !entry.formats.is_empty(),
            };
            already_has_format && entry.version == selected_package.version
        });

        let has_managed_format = formats
            .iter()
            .any(|(_, source)| source.download_type == DownloadType::Managed);

        let external_policy_status = external_policy_status(&formats);

        let plan = if already_installed {
            build_plan(
                &selected_package,
                InstallPlanStatus::AlreadyInstalled,
                None,
                effective_scope,
                installed_version,
                &formats,
                None,
                format!(
                    "{} is already installed at version {}.",
                    package.name, selected_package.version
                ),
            )
        } else if has_managed_format {
            let installer = vendor_installer_plan(package, &registry)?;
            let status = if installer.installed_app_path.is_some() {
                InstallPlanStatus::VendorInstallerAvailable
            } else {
                InstallPlanStatus::VendorInstallerRequired
            };
            let message = if installer.installed_app_path.is_some() {
                format!(
                    "Use {} to download and activate {}. Then run apm scan.",
                    installer.name, package.name
                )
            } else {
                format!(
                    "{} is required for {}. Install it first, then run apm scan after the vendor install.",
                    installer.name, package.name
                )
            };
            build_plan(
                &selected_package,
                status,
                None,
                effective_scope,
                installed_version,
                &formats,
                Some(installer),
                message,
            )
        } else if let Some(status) = external_policy_status {
            build_external_policy_plan(
                &selected_package,
                status,
                effective_scope,
                installed_version,
                &formats,
                &package.name,
            )
        } else {
            let destination = install_destination_label(
                &formats
                    .iter()
                    .map(|(format, _)| *format)
                    .collect::<Vec<_>>(),
                effective_scope,
            );
            build_plan(
                &selected_package,
                InstallPlanStatus::Ready,
                Some(destination.clone()),
                effective_scope,
                installed_version,
                &formats,
                None,
                format!(
                    "Ready to install {} v{} to {}.",
                    package.name, selected_package.version, destination
                ),
            )
        };

        Ok(InstallPlanResult::Plan {
            plan: Box::new(plan),
        })
    }

    pub fn install_handoff(&self, request: InstallPlanRequest) -> Result<InstallHandoffResult> {
        let plan = self.plan_install(request)?;
        let InstallPlanResult::Plan { plan } = plan else {
            return Ok(InstallHandoffResult::PlanUnavailable { plan });
        };

        match handoff_for_plan(&plan) {
            Some(handoff) => Ok(InstallHandoffResult::Open { plan, handoff }),
            None => Ok(InstallHandoffResult::NoHandoff {
                reason: no_handoff_reason(&plan),
                plan,
            }),
        }
    }
}

pub(super) fn selected_formats(
    package: &PluginDefinition,
    requested_format: Option<PluginFormat>,
) -> Vec<(PluginFormat, &FormatSource)> {
    let mut formats: Vec<(PluginFormat, &FormatSource)> = match requested_format {
        Some(format) => package
            .formats
            .get(&format)
            .map(|source| vec![(format, source)])
            .unwrap_or_default(),
        None => package
            .formats
            .iter()
            .map(|(format, source)| (*format, source))
            .collect(),
    };

    formats.sort_by_key(|(format, _)| format.to_string());
    formats
}

fn external_policy_status(formats: &[(PluginFormat, &FormatSource)]) -> Option<InstallPlanStatus> {
    if formats
        .iter()
        .any(|(_, source)| source.install_type == InstallType::Mas)
    {
        return Some(InstallPlanStatus::AppStoreRequired);
    }
    if formats
        .iter()
        .any(|(_, source)| source.install_type == InstallType::Pkg)
    {
        return Some(InstallPlanStatus::PrivilegedInstallerRequired);
    }
    if formats
        .iter()
        .any(|(_, source)| source.download_type == DownloadType::Manual)
    {
        return Some(InstallPlanStatus::ManualRequired);
    }
    None
}

fn build_external_policy_plan(
    package: &PluginDefinition,
    status: InstallPlanStatus,
    scope: InstallScope,
    installed_version: Option<String>,
    formats: &[(PluginFormat, &FormatSource)],
    package_name: &str,
) -> PackageInstallPlan {
    let message = match status {
        InstallPlanStatus::AppStoreRequired => format!(
            "{package_name} is distributed through the Mac App Store. Open the App Store listing, install it with the App Store app, then run apm scan."
        ),
        InstallPlanStatus::PrivilegedInstallerRequired => format!(
            "{package_name} requires a PKG installer. PKG installers can run privileged scripts, so the shared desktop/service path will not run it until apm has a privileged helper or escalation design."
        ),
        InstallPlanStatus::ManualRequired => format!(
            "{package_name} requires manual installation. Install it externally, then run apm scan."
        ),
        _ => unreachable!("external policy plan only covers non-managed external statuses"),
    };

    build_plan(
        package,
        status,
        None,
        scope,
        installed_version,
        formats,
        None,
        message,
    )
}

fn build_plan(
    package: &PluginDefinition,
    status: InstallPlanStatus,
    destination: Option<String>,
    scope: InstallScope,
    installed_version: Option<String>,
    formats: &[(PluginFormat, &FormatSource)],
    installer: Option<VendorInstallerPlan>,
    message: String,
) -> PackageInstallPlan {
    PackageInstallPlan {
        slug: package.slug.clone(),
        name: package.name.clone(),
        vendor: package.vendor.clone(),
        version: package.version.clone(),
        status,
        destination,
        scope,
        installed_version,
        formats: formats
            .iter()
            .map(|(format, source)| InstallPlanFormat {
                format: *format,
                install_type: source.install_type.to_string(),
                download_type: source.download_type.clone(),
                source: format_source_url(package, source),
                bundle_path: source.bundle_path.clone(),
                has_checksum: has_real_checksum(&source.sha256),
            })
            .collect(),
        installer,
        message,
    }
}

fn vendor_installer_plan(
    package: &PluginDefinition,
    registry: &Registry,
) -> Result<VendorInstallerPlan> {
    let installer_key = package.installer.as_deref().ok_or_else(|| {
        anyhow!(
            "Plugin '{}' is marked as installer-managed but has no installer key.",
            package.slug
        )
    })?;

    let installer = registry.find_installer(installer_key).ok_or_else(|| {
        anyhow!(
            "Installer '{}' for plugin '{}' was not found in the registry.",
            installer_key,
            package.slug
        )
    })?;

    Ok(VendorInstallerPlan {
        key: installer.key.clone(),
        name: installer.name.clone(),
        download_url: installer.download_url.clone(),
        homepage: installer.homepage.clone(),
        installed_app_path: installed_app_path(installer),
    })
}

fn installed_app_path(installer: &InstallerDefinition) -> Option<PathBuf> {
    installer
        .app_paths
        .iter()
        .find(|path| path.exists())
        .cloned()
}

fn handoff_for_plan(plan: &PackageInstallPlan) -> Option<InstallHandoff> {
    match plan.status {
        InstallPlanStatus::ManualRequired => manual_handoff(plan),
        InstallPlanStatus::PrivilegedInstallerRequired => privileged_installer_handoff(plan),
        InstallPlanStatus::AppStoreRequired => app_store_handoff(plan),
        InstallPlanStatus::VendorInstallerAvailable => {
            let installer = plan.installer.as_ref()?;
            let path = installer.installed_app_path.clone()?;
            Some(InstallHandoff {
                kind: InstallHandoffKind::VendorApp,
                label: format!("Open {}", installer.name),
                target: InstallHandoffTarget::App { path },
                message: format!(
                    "Opening {}. Install {}, then run apm scan.",
                    installer.name, plan.name
                ),
            })
        }
        InstallPlanStatus::VendorInstallerRequired => {
            let installer = plan.installer.as_ref()?;
            non_empty_url(&installer.download_url).map(|url| InstallHandoff {
                kind: InstallHandoffKind::VendorDownload,
                label: format!("Get {}", installer.name),
                target: InstallHandoffTarget::Url { url },
                message: format!(
                    "Opening the {} download page. Install {}, then run apm scan.",
                    installer.name, plan.name
                ),
            })
        }
        InstallPlanStatus::Ready | InstallPlanStatus::AlreadyInstalled => None,
    }
}

fn manual_handoff(plan: &PackageInstallPlan) -> Option<InstallHandoff> {
    first_handoff_url(plan).map(|url| InstallHandoff {
        kind: InstallHandoffKind::ManualDownload,
        label: "Open download page".to_string(),
        target: InstallHandoffTarget::Url { url },
        message: format!(
            "Opening the {} download page. Install it manually, then run apm scan.",
            plan.name
        ),
    })
}

fn privileged_installer_handoff(plan: &PackageInstallPlan) -> Option<InstallHandoff> {
    first_handoff_url(plan).map(|url| InstallHandoff {
        kind: InstallHandoffKind::PrivilegedInstaller,
        label: "Open PKG download".to_string(),
        target: InstallHandoffTarget::Url { url },
        message: format!(
            "Opening the {} PKG download. Review the vendor installer prompt manually, then run apm scan after installation.",
            plan.name
        ),
    })
}

fn app_store_handoff(plan: &PackageInstallPlan) -> Option<InstallHandoff> {
    first_handoff_url(plan).map(|url| InstallHandoff {
        kind: InstallHandoffKind::AppStore,
        label: "Open App Store".to_string(),
        target: InstallHandoffTarget::Url { url },
        message: format!(
            "Opening the {} App Store listing. Install it with the App Store app, then run apm scan.",
            plan.name
        ),
    })
}

fn first_handoff_url(plan: &PackageInstallPlan) -> Option<String> {
    plan.formats
        .iter()
        .find_map(|format| non_empty_url(&format.source))
}

fn non_empty_url(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn no_handoff_reason(plan: &PackageInstallPlan) -> String {
    match plan.status {
        InstallPlanStatus::Ready => {
            "This package is ready for direct apm installation; no external handoff is needed."
                .to_string()
        }
        InstallPlanStatus::AlreadyInstalled => {
            "This package is already installed; no external handoff is needed.".to_string()
        }
        InstallPlanStatus::ManualRequired => {
            "This manual package does not list a download page.".to_string()
        }
        InstallPlanStatus::PrivilegedInstallerRequired => {
            "This PKG installer package does not list a download page.".to_string()
        }
        InstallPlanStatus::AppStoreRequired => {
            "This Mac App Store package does not list an App Store URL.".to_string()
        }
        InstallPlanStatus::VendorInstallerAvailable
        | InstallPlanStatus::VendorInstallerRequired => {
            "This vendor-managed package does not list an app path or download page.".to_string()
        }
    }
}

fn format_source_url(package: &PluginDefinition, source: &FormatSource) -> String {
    match source.download_type {
        DownloadType::Manual => (!source.url.trim().is_empty() && source.url != "manual")
            .then_some(source.url.as_str())
            .or_else(|| {
                package
                    .homepage
                    .as_deref()
                    .filter(|homepage| !homepage.trim().is_empty())
            })
            .unwrap_or("")
            .to_string(),
        _ => source.url.clone(),
    }
}

fn install_destination_label(formats: &[PluginFormat], scope: InstallScope) -> String {
    let has_app = formats.contains(&PluginFormat::App);
    let has_plugin = formats
        .iter()
        .any(|format| matches!(format, PluginFormat::Au | PluginFormat::Vst3));

    match (has_app, has_plugin, scope) {
        (true, false, InstallScope::User) => "~/Applications/",
        (true, false, InstallScope::System) => "/Applications/",
        (false, true, InstallScope::User) => "~/Library/Audio/Plug-Ins/",
        (false, true, InstallScope::System) => "/Library/Audio/Plug-Ins/",
        _ => "format-specific destinations",
    }
    .to_string()
}

fn available_formats(package: &PluginDefinition) -> Vec<PluginFormat> {
    let mut formats: Vec<PluginFormat> = package.formats.keys().copied().collect();
    formats.sort_by_key(|format| format.to_string());
    formats
}

fn has_real_checksum(sha256: &str) -> bool {
    !crate::install::is_placeholder_sha256(sha256)
}

fn install_suggestions(registry: &Registry, query: &str) -> Vec<String> {
    let query_lower = query.to_lowercase();
    let prefix = if query_lower.len() >= 3 {
        &query_lower[..3]
    } else {
        &query_lower
    };

    let mut suggestions: Vec<String> = registry
        .plugins
        .keys()
        .filter(|slug| {
            let slug_lower = slug.to_lowercase();
            slug_lower.starts_with(prefix) || slug_lower.contains(&query_lower)
        })
        .cloned()
        .collect();
    suggestions.sort();
    suggestions.truncate(3);
    suggestions
}

#[cfg(test)]
#[path = "install_plan_tests.rs"]
mod tests;
