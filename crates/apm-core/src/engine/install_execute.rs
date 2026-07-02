use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use tracing::debug;

use super::install_archive::{ArchiveFormat, ReadyArchiveFormat};
#[cfg(feature = "reqwest")]
use super::install_download::download_ready_archive_formats;
#[cfg(all(test, feature = "reqwest"))]
use super::install_download::part_path;
use super::install_plan;
use super::{
    ensure_not_cancelled, ApmEngine, EngineEvent, EventSink, InstallPlanRequest, InstallPlanResult,
    InstallPlanStatus, InstalledPackageSummary, PackageInstallPlan,
};
use crate::config::{Config, InstallScope};
use crate::error::ApmError;
use crate::install;
use crate::registry::{FormatSource, InstallType, PluginDefinition, PluginFormat, Registry};
use crate::state::{InstallOrigin, InstallState, InstalledFormat, InstalledPlugin};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstallPackageRequest {
    pub slug: String,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub format: Option<PluginFormat>,
    #[serde(default)]
    pub scope: Option<InstallScope>,
    #[serde(default)]
    pub archive_path: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum InstallPackageResult {
    PlanUnavailable {
        plan: InstallPlanResult,
    },
    AlreadyInstalled {
        plan: Box<PackageInstallPlan>,
    },
    ExternalHandoffRequired {
        plan: Box<PackageInstallPlan>,
        reason: String,
    },
    FormatRequired {
        plan: Box<PackageInstallPlan>,
        available_formats: Vec<PluginFormat>,
        reason: String,
    },
    ArchiveRequired {
        plan: Box<PackageInstallPlan>,
        reason: String,
    },
    UnsupportedInstallType {
        plan: Box<PackageInstallPlan>,
        format: PluginFormat,
        install_type: InstallType,
        reason: String,
    },
    Installed {
        package: InstalledPackageSummary,
    },
}

struct ReadyArchiveInstall {
    plan: Box<PackageInstallPlan>,
    package: PluginDefinition,
    formats: Vec<ReadyArchiveFormat>,
    scope: InstallScope,
}

impl ApmEngine {
    pub fn install_package_from_archive(
        &self,
        request: InstallPackageRequest,
        events: &mut impl EventSink,
    ) -> Result<InstallPackageResult> {
        self.install_package_from_archive_with_dest_resolver(
            request,
            events,
            install::plugin_dest_dir,
        )
    }

    fn install_package_from_archive_with_dest_resolver(
        &self,
        request: InstallPackageRequest,
        events: &mut impl EventSink,
        destination_dir: impl Fn(PluginFormat, InstallScope) -> PathBuf,
    ) -> Result<InstallPackageResult> {
        let ready = match self.prepare_ready_archive_install(&request)? {
            Ok(ready) => ready,
            Err(result) => return Ok(result),
        };
        ensure_not_cancelled(events)?;

        let Some(archive_path) = request.archive_path.as_deref() else {
            return Ok(InstallPackageResult::ArchiveRequired {
                reason: "The package is ready for direct installation, but no local archive path was supplied.".to_string(),
                plan: ready.plan,
            });
        };

        if !archive_path.is_file() {
            return Err(ApmError::Install {
                plugin: ready.package.slug.clone(),
                reason: format!("Archive not found: {}", archive_path.display()),
                hint: "Choose an existing downloaded archive and try again.".to_string(),
            }
            .into());
        }

        let formats = ready
            .formats
            .into_iter()
            .map(|format| ArchiveFormat {
                format: format.format,
                source: format.source,
                archive_path: archive_path.to_path_buf(),
            })
            .collect::<Vec<_>>();

        install_selected_archive_formats(
            &self.config,
            &ready.package,
            &formats,
            ready.scope,
            events,
            destination_dir,
        )
    }

    #[cfg(feature = "reqwest")]
    pub fn install_package_from_url(
        &self,
        request: InstallPackageRequest,
        events: &mut impl EventSink,
    ) -> Result<InstallPackageResult> {
        self.install_package_from_url_with_dest_resolver(request, events, install::plugin_dest_dir)
    }

    #[cfg(feature = "reqwest")]
    pub(super) fn install_package_from_url_with_dest_resolver(
        &self,
        request: InstallPackageRequest,
        events: &mut impl EventSink,
        destination_dir: impl Fn(PluginFormat, InstallScope) -> PathBuf,
    ) -> Result<InstallPackageResult> {
        let ready = match self.prepare_ready_archive_install(&request)? {
            Ok(ready) => ready,
            Err(result) => return Ok(result),
        };
        ensure_not_cancelled(events)?;

        self.install_ready_archive_formats_from_url_with_dest_resolver(
            ready,
            events,
            destination_dir,
        )
    }

    #[cfg(feature = "reqwest")]
    pub(super) fn install_package_formats_from_url_with_dest_resolver(
        &self,
        request: InstallPackageRequest,
        formats: &[PluginFormat],
        events: &mut impl EventSink,
        destination_dir: impl Fn(PluginFormat, InstallScope) -> PathBuf,
    ) -> Result<InstallPackageResult> {
        let ready = match self.prepare_ready_archive_install_for_formats(&request, formats)? {
            Ok(ready) => ready,
            Err(result) => return Ok(result),
        };
        ensure_not_cancelled(events)?;
        self.install_ready_archive_formats_from_url_with_dest_resolver(
            ready,
            events,
            destination_dir,
        )
    }

    #[cfg(feature = "reqwest")]
    fn install_ready_archive_formats_from_url_with_dest_resolver(
        &self,
        ready: ReadyArchiveInstall,
        events: &mut impl EventSink,
        destination_dir: impl Fn(PluginFormat, InstallScope) -> PathBuf,
    ) -> Result<InstallPackageResult> {
        let formats = match download_ready_archive_formats(
            &self.config,
            &ready.package,
            &ready.formats,
            events,
        ) {
            Ok(formats) => formats,
            Err(error) => {
                events.emit(EngineEvent::InstallFailed {
                    slug: ready.package.slug.clone(),
                    error: error.to_string(),
                });
                return Err(error);
            }
        };
        ensure_not_cancelled(events)?;
        install_selected_archive_formats(
            &self.config,
            &ready.package,
            &formats,
            ready.scope,
            events,
            destination_dir,
        )
    }

    fn prepare_ready_archive_install(
        &self,
        request: &InstallPackageRequest,
    ) -> Result<std::result::Result<ReadyArchiveInstall, InstallPackageResult>> {
        let plan_result = self.plan_install(plan_request(request))?;
        let InstallPlanResult::Plan { plan } = plan_result else {
            return Ok(Err(InstallPackageResult::PlanUnavailable {
                plan: plan_result,
            }));
        };

        match plan.status {
            InstallPlanStatus::AlreadyInstalled => {
                return Ok(Err(InstallPackageResult::AlreadyInstalled { plan }));
            }
            InstallPlanStatus::ManualRequired
            | InstallPlanStatus::PrivilegedInstallerRequired
            | InstallPlanStatus::AppStoreRequired
            | InstallPlanStatus::VendorInstallerAvailable
            | InstallPlanStatus::VendorInstallerRequired => {
                return Ok(Err(InstallPackageResult::ExternalHandoffRequired {
                    reason: external_handoff_reason(&plan),
                    plan,
                }));
            }
            InstallPlanStatus::Ready => {}
        }

        if request.format.is_none() && plan.formats.len() > 1 {
            let available_formats = plan.formats.iter().map(|format| format.format).collect();
            return Ok(Err(InstallPackageResult::FormatRequired {
                reason: "Direct archive installation needs one explicit format so apm can verify and place the correct archive.".to_string(),
                available_formats,
                plan,
            }));
        }

        let package = resolve_selected_package(&self.config, request)?;
        let formats: Vec<ReadyArchiveFormat> =
            install_plan::selected_formats(&package, request.format)
                .into_iter()
                .map(|(format, source)| ReadyArchiveFormat {
                    format,
                    source: source.clone(),
                })
                .collect();
        let Some(first_format) = formats.first() else {
            return Err(anyhow!(
                "Install plan for '{}' was ready but no installable formats were selected.",
                request.slug
            ));
        };

        if matches!(
            first_format.source.install_type,
            InstallType::Pkg | InstallType::Mas
        ) {
            return Ok(Err(unsupported_install_type_result(
                plan,
                first_format.format,
                first_format.source.install_type,
            )));
        }

        Ok(Ok(ReadyArchiveInstall {
            plan,
            package,
            formats,
            scope: request.scope.unwrap_or(self.config.install_scope),
        }))
    }

    #[cfg(feature = "reqwest")]
    fn prepare_ready_archive_install_for_formats(
        &self,
        request: &InstallPackageRequest,
        formats: &[PluginFormat],
    ) -> Result<std::result::Result<ReadyArchiveInstall, InstallPackageResult>> {
        let mut selected_formats = formats.to_vec();
        selected_formats.sort_by_key(|format| format.to_string());
        selected_formats.dedup();

        if selected_formats.is_empty() {
            let plan_result = self.plan_install(plan_request(request))?;
            let plan = match plan_result {
                InstallPlanResult::Plan { plan } => plan,
                plan => return Ok(Err(InstallPackageResult::PlanUnavailable { plan })),
            };
            return Ok(Err(InstallPackageResult::FormatRequired {
                reason: "Update execution needs at least one tracked format.".to_string(),
                available_formats: Vec::new(),
                plan,
            }));
        }

        let mut combined_formats = Vec::new();
        let mut combined_plan = None;
        let mut combined_package = None;
        let mut combined_scope = request.scope.unwrap_or(self.config.install_scope);

        for format in selected_formats {
            let format_request = InstallPackageRequest {
                format: Some(format),
                ..request.clone()
            };
            let ready = match self.prepare_ready_archive_install(&format_request)? {
                Ok(ready) => ready,
                Err(result) => return Ok(Err(result)),
            };

            let ReadyArchiveInstall {
                plan,
                package,
                formats,
                scope,
            } = ready;

            if combined_plan.is_none() {
                combined_plan = Some(plan);
                combined_package = Some(package);
                combined_scope = scope;
            }
            combined_formats.extend(formats);
        }

        let Some(plan) = combined_plan else {
            return Err(anyhow!(
                "Install plan for '{}' was ready but no selected formats were prepared.",
                request.slug
            ));
        };
        let Some(package) = combined_package else {
            return Err(anyhow!(
                "Install plan for '{}' was ready but no package was resolved.",
                request.slug
            ));
        };

        Ok(Ok(ReadyArchiveInstall {
            plan,
            package,
            formats: combined_formats,
            scope: combined_scope,
        }))
    }
}

fn plan_request(request: &InstallPackageRequest) -> InstallPlanRequest {
    InstallPlanRequest {
        slug: request.slug.clone(),
        version: request.version.clone(),
        format: request.format,
        scope: request.scope,
    }
}

fn unsupported_install_type_result(
    plan: Box<PackageInstallPlan>,
    format: PluginFormat,
    install_type: InstallType,
) -> InstallPackageResult {
    InstallPackageResult::UnsupportedInstallType {
        reason: unsupported_install_type_reason(install_type),
        format,
        install_type,
        plan,
    }
}

fn unsupported_install_type_reason(install_type: InstallType) -> String {
    match install_type {
        InstallType::Pkg => {
            "PKG installers can run privileged scripts; shared engine execution requires a privileged helper or escalation design before running them.".to_string()
        }
        InstallType::Mas => {
            "Mac App Store packages cannot be installed from an archive; open the App Store product page instead.".to_string()
        }
        InstallType::Dmg | InstallType::Zip => {
            unreachable!("ZIP and DMG are supported archive install types")
        }
    }
}

fn resolve_selected_package(
    config: &Config,
    request: &InstallPackageRequest,
) -> Result<PluginDefinition> {
    let registry = Registry::load_all_sources(config)?;
    let package = registry.find(&request.slug).ok_or_else(|| {
        anyhow!(
            "Install plan for '{}' was ready but the package was not found when resolving execution.",
            request.slug
        )
    })?;

    let release = package.resolve_release(request.version.as_deref()).ok_or_else(|| {
        anyhow!(
            "Install plan for '{}' was ready but version {:?} was not found when resolving execution.",
            request.slug,
            request.version
        )
    })?;

    let mut selected_package = package.clone();
    selected_package.version = release.version;
    selected_package.formats = release.formats;
    Ok(selected_package)
}

fn install_selected_archive_formats(
    config: &Config,
    package: &PluginDefinition,
    formats: &[ArchiveFormat],
    scope: InstallScope,
    events: &mut impl EventSink,
    destination_dir: impl Fn(PluginFormat, InstallScope) -> PathBuf,
) -> Result<InstallPackageResult> {
    let mut state = InstallState::load(config)?;

    events.emit(EngineEvent::InstallStarted {
        slug: package.slug.clone(),
        version: package.version.clone(),
        format_count: formats.len(),
    });
    ensure_not_cancelled(events)?;

    let mut installed_paths = Vec::new();
    for format in formats {
        match install_one_archive_format(
            package,
            format.format,
            &format.source,
            &format.archive_path,
            scope,
            events,
            &destination_dir,
        ) {
            Ok(bundle_path) => {
                installed_paths.push((format.format, bundle_path));
                if let Err(error) = ensure_not_cancelled(events) {
                    return rollback_and_return_error(
                        &package.slug,
                        &installed_paths,
                        events,
                        error,
                    );
                }
            }
            Err(error) => {
                return rollback_and_return_error(&package.slug, &installed_paths, events, error);
            }
        }
    }
    if let Err(error) = ensure_not_cancelled(events) {
        return rollback_and_return_error(&package.slug, &installed_paths, events, error);
    }

    events.emit(EngineEvent::InstallStateRecordingStarted {
        slug: package.slug.clone(),
    });

    let installed_formats: Vec<InstalledFormat> = installed_paths
        .iter()
        .map(|(format, path)| InstalledFormat {
            format: *format,
            path: path.clone(),
            sha256: String::new(),
        })
        .collect();

    let record = InstalledPlugin {
        name: package.slug.clone(),
        version: package.version.clone(),
        vendor: package.vendor.clone(),
        formats: installed_formats,
        installed_at: Utc::now(),
        source: package
            .source_name
            .clone()
            .unwrap_or_else(|| "official".to_string()),
        pinned: false,
        origin: InstallOrigin::Apm,
    };
    let package_summary = super::installed_package_summary(&record);

    state.record_install(record);
    if let Err(error) = state.save(config).with_context(|| {
        format!(
            "Failed to save install state after installing '{}'",
            package.slug
        )
    }) {
        return rollback_and_return_error(&package.slug, &installed_paths, events, error);
    }

    events.emit(EngineEvent::InstallStateRecorded {
        slug: package.slug.clone(),
    });
    events.emit(EngineEvent::InstallFinished {
        slug: package.slug.clone(),
        installed_format_count: installed_paths.len(),
    });

    Ok(InstallPackageResult::Installed {
        package: package_summary,
    })
}

fn install_one_archive_format(
    package: &PluginDefinition,
    format: PluginFormat,
    source: &FormatSource,
    archive_path: &Path,
    scope: InstallScope,
    events: &mut impl EventSink,
    destination_dir: &impl Fn(PluginFormat, InstallScope) -> PathBuf,
) -> Result<PathBuf> {
    events.emit(EngineEvent::InstallFormatStarted {
        slug: package.slug.clone(),
        format,
    });
    ensure_not_cancelled(events)?;

    if !install::is_placeholder_sha256(&source.sha256) {
        let sha256 =
            install::verify_file_sha256(archive_path, &source.sha256).with_context(|| {
                format!(
                    "Checksum verification failed for local file '{}'",
                    archive_path.display()
                )
            })?;
        events.emit(EngineEvent::InstallArchiveVerified {
            slug: package.slug.clone(),
            format,
            path: archive_path.to_path_buf(),
            sha256,
        });
        ensure_not_cancelled(events)?;
    }

    let dest_dir = destination_dir(format, scope);
    events.emit(EngineEvent::InstallArchiveInstallStarted {
        slug: package.slug.clone(),
        format,
        install_type: source.install_type,
        path: archive_path.to_path_buf(),
    });
    ensure_not_cancelled(events)?;

    let bundle_path = match source.install_type {
        InstallType::Zip => install::zip::install_from_zip(
            archive_path,
            &dest_dir,
            format,
            source.bundle_path.as_deref(),
        )
        .with_context(|| format!("ZIP install failed for '{}' ({})", package.slug, format))?,
        InstallType::Dmg => install::dmg::install_from_dmg(
            archive_path,
            &dest_dir,
            format,
            source.bundle_path.as_deref(),
        )
        .with_context(|| format!("DMG install failed for '{}' ({})", package.slug, format))?,
        InstallType::Pkg | InstallType::Mas => {
            return Err(ApmError::Install {
                plugin: package.slug.clone(),
                reason: unsupported_install_type_reason(source.install_type),
                hint: "Use a manual or vendor handoff for this package until the shared engine supports this installer type.".to_string(),
            }
            .into());
        }
    };

    if let Err(error) = finish_placed_archive_format(package, format, &bundle_path, events) {
        rollback_installed(&package.slug, &[(format, bundle_path)], events);
        return Err(error);
    }

    Ok(bundle_path)
}

fn finish_placed_archive_format(
    package: &PluginDefinition,
    format: PluginFormat,
    bundle_path: &Path,
    events: &mut impl EventSink,
) -> Result<()> {
    events.emit(EngineEvent::InstallQuarantineRemovalStarted {
        slug: package.slug.clone(),
        format,
        path: bundle_path.to_path_buf(),
    });
    ensure_not_cancelled(events)?;

    install::quarantine::remove_quarantine(bundle_path).with_context(|| {
        format!(
            "Quarantine removal failed for '{}' ({})",
            bundle_path.display(),
            format
        )
    })?;
    ensure_not_cancelled(events)?;

    events.emit(EngineEvent::InstallFormatPlaced {
        slug: package.slug.clone(),
        format,
        path: bundle_path.to_path_buf(),
    });
    Ok(())
}

fn rollback_and_return_error<T>(
    slug: &str,
    installed: &[(PluginFormat, PathBuf)],
    events: &mut impl EventSink,
    error: anyhow::Error,
) -> Result<T> {
    rollback_installed(slug, installed, events);
    events.emit(EngineEvent::InstallFailed {
        slug: slug.to_string(),
        error: error.to_string(),
    });
    Err(error)
}

fn rollback_installed(
    slug: &str,
    installed: &[(PluginFormat, PathBuf)],
    events: &mut impl EventSink,
) {
    for (format, path) in installed.iter().rev() {
        if !path.exists() {
            continue;
        }

        if let Err(error) = std::fs::remove_dir_all(path) {
            debug!(
                "Could not roll back partial {} bundle for {} at {}: {error}",
                format,
                slug,
                path.display()
            );
            continue;
        }

        events.emit(EngineEvent::InstallRolledBack {
            slug: slug.to_string(),
            format: *format,
            path: path.clone(),
        });
    }
}

fn external_handoff_reason(plan: &PackageInstallPlan) -> String {
    match plan.status {
        InstallPlanStatus::ManualRequired => {
            "This package requires a manual download or account-gated installer.".to_string()
        }
        InstallPlanStatus::PrivilegedInstallerRequired => {
            "This package requires a PKG installer, which can run privileged scripts. Use the external handoff until apm has a privileged helper or escalation design.".to_string()
        }
        InstallPlanStatus::AppStoreRequired => {
            "This package is distributed through the Mac App Store. Use the App Store handoff instead of a direct archive install.".to_string()
        }
        InstallPlanStatus::VendorInstallerAvailable => {
            "This package is managed by an installed vendor app.".to_string()
        }
        InstallPlanStatus::VendorInstallerRequired => {
            "This package requires a vendor app before apm can reconcile the install.".to_string()
        }
        InstallPlanStatus::Ready | InstallPlanStatus::AlreadyInstalled => {
            "This package does not require an external handoff.".to_string()
        }
    }
}

#[cfg(test)]
#[path = "install_execute_tests.rs"]
mod tests;
