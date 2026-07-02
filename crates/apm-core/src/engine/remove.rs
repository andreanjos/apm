use std::path::{Path, PathBuf};

use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};

use super::{
    ensure_not_cancelled, installed_package_summary, ApmEngine, EngineEvent, EventSink,
    InstalledPackageSummary,
};
use crate::install;
use crate::registry::PluginFormat;
use crate::state::{InstallOrigin, InstallState, InstalledFormat, InstalledPlugin};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemovePackageRequest {
    pub slug: String,
    #[serde(default)]
    pub dry_run: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum RemovePackageResult {
    NotInstalled {
        slug: String,
    },
    ExternalInstallPresent {
        package: InstalledPackageSummary,
        reason: String,
    },
    DryRun {
        package: InstalledPackageSummary,
        formats: Vec<RemoveFormatSummary>,
        would_delete_files: bool,
        reason: Option<String>,
    },
    Removed {
        package: InstalledPackageSummary,
        removed_formats: Vec<RemoveFormatSummary>,
        state_only: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoveFormatSummary {
    pub format: PluginFormat,
    pub path: PathBuf,
    pub existed: bool,
}

impl ApmEngine {
    pub fn remove_package(
        &self,
        request: RemovePackageRequest,
        events: &mut impl EventSink,
    ) -> Result<RemovePackageResult> {
        let mut state = InstallState::load(&self.config)?;
        let Some(plugin) = state.find(&request.slug).cloned() else {
            return Ok(RemovePackageResult::NotInstalled { slug: request.slug });
        };

        let package = installed_package_summary(&plugin);
        if plugin.origin == InstallOrigin::External {
            return remove_external_package(
                &self.config,
                &mut state,
                plugin,
                package,
                request,
                events,
            );
        }

        if request.dry_run {
            return Ok(RemovePackageResult::DryRun {
                package,
                formats: remove_format_summaries(&plugin),
                would_delete_files: true,
                reason: None,
            });
        }

        remove_apm_package(&self.config, &mut state, plugin, package, events)
    }
}

fn remove_external_package(
    config: &crate::config::Config,
    state: &mut InstallState,
    plugin: InstalledPlugin,
    package: InstalledPackageSummary,
    request: RemovePackageRequest,
    events: &mut impl EventSink,
) -> Result<RemovePackageResult> {
    let formats = remove_format_summaries(&plugin);
    if formats.iter().any(|format| format.existed) {
        return Ok(RemovePackageResult::ExternalInstallPresent {
            package,
            reason: "This package was discovered by scan; apm will not delete externally installed files.".to_string(),
        });
    }

    if request.dry_run {
        return Ok(RemovePackageResult::DryRun {
            package,
            formats,
            would_delete_files: false,
            reason: Some("stale external state entry".to_string()),
        });
    }

    start_remove(&plugin, events)?;
    state.remove(&plugin.name);
    save_remove_state(config, state, &plugin.name, events)?;
    events.emit(EngineEvent::RemoveFinished {
        slug: plugin.name,
        removed_format_count: 0,
    });

    Ok(RemovePackageResult::Removed {
        package,
        removed_formats: Vec::new(),
        state_only: true,
    })
}

fn remove_apm_package(
    config: &crate::config::Config,
    state: &mut InstallState,
    plugin: InstalledPlugin,
    package: InstalledPackageSummary,
    events: &mut impl EventSink,
) -> Result<RemovePackageResult> {
    start_remove(&plugin, events)?;

    let mut removed_formats = Vec::new();
    for format in &plugin.formats {
        if let Err(error) = ensure_not_cancelled(events) {
            persist_partial_removal(config, state, &plugin.name, &removed_formats, events)?;
            return Err(error);
        }

        match remove_one_format(&plugin.name, format, events) {
            Ok(summary) => {
                removed_formats.push(summary);
                if let Err(error) = ensure_not_cancelled(events) {
                    persist_partial_removal(config, state, &plugin.name, &removed_formats, events)?;
                    return Err(error);
                }
            }
            Err(error) => {
                persist_partial_removal(config, state, &plugin.name, &removed_formats, events)?;
                emit_remove_failed(&plugin.name, &error, events);
                return Err(error);
            }
        }
    }

    state.remove(&plugin.name);
    save_remove_state(config, state, &plugin.name, events)?;
    let removed_format_count = removed_formats
        .iter()
        .filter(|format| format.existed)
        .count();
    events.emit(EngineEvent::RemoveFinished {
        slug: plugin.name,
        removed_format_count,
    });

    Ok(RemovePackageResult::Removed {
        package,
        removed_formats,
        state_only: false,
    })
}

fn persist_partial_removal(
    config: &crate::config::Config,
    state: &mut InstallState,
    slug: &str,
    removed_formats: &[RemoveFormatSummary],
    events: &mut impl EventSink,
) -> Result<()> {
    if removed_formats.is_empty() {
        return Ok(());
    }

    let should_remove_plugin = if let Some(plugin) = state.find_mut(slug) {
        plugin.formats.retain(|installed| {
            !removed_formats
                .iter()
                .any(|removed| removed.format == installed.format && removed.path == installed.path)
        });
        plugin.formats.is_empty()
    } else {
        false
    };

    if should_remove_plugin {
        state.remove(slug);
    }
    save_remove_state(config, state, slug, events)
}

fn remove_one_format(
    slug: &str,
    installed: &InstalledFormat,
    events: &mut impl EventSink,
) -> Result<RemoveFormatSummary> {
    validate_bundle_path(installed.format, &installed.path)?;

    if !installed.path.exists() {
        events.emit(EngineEvent::RemoveFormatMissing {
            slug: slug.to_string(),
            format: installed.format,
            path: installed.path.clone(),
        });
        return Ok(remove_format_summary(installed, false));
    }

    let metadata = std::fs::symlink_metadata(&installed.path)
        .with_context(|| format!("Cannot inspect bundle at {}", installed.path.display()))?;
    ensure_not_cancelled(events)?;
    if metadata.is_dir() {
        std::fs::remove_dir_all(&installed.path)
            .with_context(|| format!("Cannot remove bundle at {}", installed.path.display()))?;
    } else {
        std::fs::remove_file(&installed.path)
            .with_context(|| format!("Cannot remove bundle at {}", installed.path.display()))?;
    }

    events.emit(EngineEvent::RemoveFormatRemoved {
        slug: slug.to_string(),
        format: installed.format,
        path: installed.path.clone(),
    });
    Ok(remove_format_summary(installed, true))
}

fn save_remove_state(
    config: &crate::config::Config,
    state: &InstallState,
    slug: &str,
    events: &mut impl EventSink,
) -> Result<()> {
    if let Err(error) = state
        .save(config)
        .with_context(|| format!("Failed to save install state after removing '{slug}'"))
    {
        emit_remove_failed(slug, &error, events);
        return Err(error);
    }

    events.emit(EngineEvent::RemoveStateRecorded {
        slug: slug.to_string(),
    });
    Ok(())
}

fn start_remove(plugin: &InstalledPlugin, events: &mut impl EventSink) -> Result<()> {
    ensure_not_cancelled(events)?;
    events.emit(EngineEvent::RemoveStarted {
        slug: plugin.name.clone(),
        version: plugin.version.clone(),
        format_count: plugin.formats.len(),
    });
    ensure_not_cancelled(events)
}

fn emit_remove_failed(slug: &str, error: &anyhow::Error, events: &mut impl EventSink) {
    events.emit(EngineEvent::RemoveFailed {
        slug: slug.to_string(),
        error: error.to_string(),
    });
}

fn remove_format_summaries(plugin: &InstalledPlugin) -> Vec<RemoveFormatSummary> {
    plugin
        .formats
        .iter()
        .map(|format| remove_format_summary(format, format.path.exists()))
        .collect()
}

fn remove_format_summary(format: &InstalledFormat, existed: bool) -> RemoveFormatSummary {
    RemoveFormatSummary {
        format: format.format,
        path: format.path.clone(),
        existed,
    }
}

fn validate_bundle_path(format: PluginFormat, path: &Path) -> Result<()> {
    let expected = install::bundle_extension(format);
    let actual = path.extension().and_then(|extension| extension.to_str());
    ensure!(
        actual.is_some_and(|extension| extension.eq_ignore_ascii_case(expected)),
        "Refusing to remove {} path '{}' because it is not a .{} bundle.",
        format,
        path.display(),
        expected
    );
    Ok(())
}

#[cfg(test)]
#[path = "remove_tests.rs"]
mod tests;
