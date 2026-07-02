use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use super::install_archive::{ArchiveFormat, ReadyArchiveFormat};
use super::{ensure_not_cancelled, EngineEvent, EventSink};
use crate::config::Config;
use crate::download::{self, DownloadObserver, DownloadOptions, DownloadProgress};
use crate::error::ApmError;
use crate::install;
use crate::registry::{FormatSource, InstallType, PluginDefinition, PluginFormat};

const DOWNLOAD_PROGRESS_STEP_BYTES: u64 = 5 * 1024 * 1024;

pub(super) fn download_ready_archive_formats(
    config: &Config,
    package: &PluginDefinition,
    formats: &[ReadyArchiveFormat],
    events: &mut impl EventSink,
) -> Result<Vec<ArchiveFormat>> {
    let mut downloaded = Vec::new();
    for format in formats {
        let archive_path =
            download_archive(config, package, format.format, &format.source, events)?;
        ensure_not_cancelled(events)?;
        downloaded.push(ArchiveFormat {
            format: format.format,
            source: format.source.clone(),
            archive_path,
        });
    }
    Ok(downloaded)
}

fn download_archive(
    config: &Config,
    package: &PluginDefinition,
    format: PluginFormat,
    source: &FormatSource,
    events: &mut impl EventSink,
) -> Result<PathBuf> {
    ensure_not_cancelled(events)?;
    let url = source.url.trim();
    if url.is_empty() {
        return Err(ApmError::Install {
            plugin: package.slug.clone(),
            reason: format!("No download URL is listed for {format}."),
            hint: "Update the registry entry with a direct archive URL or use a manual handoff."
                .to_string(),
        }
        .into());
    }

    let archive_path = config
        .downloads_cache_dir()
        .join(archive_filename(package, format, source));
    if cached_archive_is_valid(&archive_path, &source.sha256) {
        let bytes = archive_path
            .metadata()
            .map(|metadata| metadata.len())
            .unwrap_or(0);
        events.emit(EngineEvent::InstallDownloadFinished {
            slug: package.slug.clone(),
            format,
            path: archive_path.clone(),
            bytes,
        });
        return Ok(archive_path);
    }

    remove_download_artifacts(&archive_path);

    events.emit(EngineEvent::InstallDownloadStarted {
        slug: package.slug.clone(),
        format,
        url: url.to_string(),
    });
    ensure_not_cancelled(events)?;

    if let Err(error) = download_to_path(url, &archive_path, &package.slug, format, events)
        .and_then(|()| verify_downloaded_archive(&archive_path, &source.sha256))
    {
        remove_download_artifacts(&archive_path);
        return Err(error);
    }

    let bytes = archive_path
        .metadata()
        .with_context(|| format!("Cannot stat downloaded archive: {}", archive_path.display()))?
        .len();
    events.emit(EngineEvent::InstallDownloadFinished {
        slug: package.slug.clone(),
        format,
        path: archive_path.clone(),
        bytes,
    });
    Ok(archive_path)
}

fn verify_downloaded_archive(path: &Path, expected_sha256: &str) -> Result<()> {
    if install::is_placeholder_sha256(expected_sha256) {
        return Ok(());
    }
    install::verify_file_sha256(path, expected_sha256).map(|_| ())
}

fn cached_archive_is_valid(path: &Path, expected_sha256: &str) -> bool {
    path.exists()
        && (install::is_placeholder_sha256(expected_sha256)
            || install::verify_file_sha256(path, expected_sha256).is_ok())
}

fn remove_download_artifacts(archive_path: &Path) {
    let _ = std::fs::remove_file(archive_path);
    let _ = std::fs::remove_file(part_path(archive_path));
}

fn download_to_path(
    url: &str,
    destination: &Path,
    slug: &str,
    format: PluginFormat,
    events: &mut impl EventSink,
) -> Result<()> {
    download::download_to_path(
        url,
        destination,
        DownloadOptions {
            progress_step_bytes: DOWNLOAD_PROGRESS_STEP_BYTES,
        },
        InstallDownloadObserver {
            slug,
            format,
            events,
        },
    )
    .map(|_| ())
}

struct InstallDownloadObserver<'a, T> {
    slug: &'a str,
    format: PluginFormat,
    events: &'a mut T,
}

impl<T: EventSink> DownloadObserver for InstallDownloadObserver<'_, T> {
    fn checkpoint(&mut self) -> Result<()> {
        ensure_not_cancelled(self.events)
    }

    fn progress(&mut self, DownloadProgress { bytes, total_bytes }: DownloadProgress) {
        self.events.emit(EngineEvent::InstallDownloadProgress {
            slug: self.slug.to_string(),
            format: self.format,
            bytes,
            total_bytes,
        });
    }
}

fn archive_filename(
    package: &PluginDefinition,
    format: PluginFormat,
    source: &FormatSource,
) -> String {
    let url_path = Path::new(source.url.split('?').next().unwrap_or(&source.url));
    let extension = url_path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or(match source.install_type {
            InstallType::Dmg => "dmg",
            InstallType::Pkg => "pkg",
            InstallType::Zip => "zip",
            InstallType::Mas => "app",
        });

    format!(
        "{}-{}-{}.{}",
        package.slug,
        package.version,
        format.to_string().to_lowercase(),
        extension
    )
}

pub(super) fn part_path(destination: &Path) -> PathBuf {
    download::part_path(destination)
}
