use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};

use crate::config::ensure_dir;
use crate::error::ApmError;

const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(600);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DownloadOptions {
    pub progress_step_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DownloadProgress {
    pub bytes: u64,
    pub total_bytes: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DownloadComplete {
    pub bytes: u64,
}

pub(crate) trait DownloadObserver {
    fn checkpoint(&mut self) -> Result<()>;
    fn progress(&mut self, progress: DownloadProgress);
}

pub(crate) fn download_to_path(
    url: &str,
    destination: &Path,
    options: DownloadOptions,
    mut observer: impl DownloadObserver,
) -> Result<DownloadComplete> {
    use std::io::{Read, Write};

    if let Some(parent) = destination.parent() {
        ensure_dir(parent)
            .with_context(|| format!("Cannot create download directory: {}", parent.display()))?;
    }

    let part_path = part_path(destination);
    observer.checkpoint()?;
    let mut response = reqwest::blocking::Client::builder()
        .timeout(DOWNLOAD_TIMEOUT)
        .build()
        .context("Failed to build HTTP client")?
        .get(url)
        .send()
        .map_err(ApmError::from)
        .with_context(|| format!("Failed to start download from {url}"))?;

    if !response.status().is_success() {
        let status = response.status();
        let _ = std::fs::remove_file(&part_path);
        return Err(ApmError::Download {
            url: url.to_string(),
            reason: format!("HTTP {status}"),
        }
        .into());
    }

    let total_bytes = response.content_length();
    let mut file = std::fs::File::create(&part_path)
        .with_context(|| format!("Cannot create .part file: {}", part_path.display()))?;
    let mut buffer = [0_u8; 64 * 1024];
    let mut downloaded = 0_u64;
    let mut last_progress = 0_u64;
    loop {
        observer.checkpoint()?;
        let read = response
            .read(&mut buffer)
            .with_context(|| format!("Read error from {url}"))?;
        if read == 0 {
            break;
        }
        file.write_all(&buffer[..read])
            .with_context(|| format!("Write error on .part file: {}", part_path.display()))?;
        downloaded += read as u64;
        if should_emit_progress(downloaded, last_progress, total_bytes, options) {
            observer.progress(DownloadProgress {
                bytes: downloaded,
                total_bytes,
            });
            last_progress = downloaded;
        }
    }

    observer.checkpoint()?;
    if downloaded > 0 && last_progress != downloaded {
        observer.progress(DownloadProgress {
            bytes: downloaded,
            total_bytes,
        });
    }
    file.flush()
        .with_context(|| format!("Flush error on .part file: {}", part_path.display()))?;
    drop(file);

    if let Err(error) = std::fs::rename(&part_path, destination).with_context(|| {
        format!(
            "Cannot rename .part file {} -> {}",
            part_path.display(),
            destination.display()
        )
    }) {
        let _ = std::fs::remove_file(&part_path);
        return Err(error);
    }

    Ok(DownloadComplete { bytes: downloaded })
}

pub(crate) fn part_path(destination: &Path) -> PathBuf {
    let mut part_path = destination.to_path_buf();
    let file_name = destination
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("download");
    part_path.set_file_name(format!("{file_name}.part"));
    part_path
}

fn should_emit_progress(
    downloaded: u64,
    last_progress: u64,
    total_bytes: Option<u64>,
    options: DownloadOptions,
) -> bool {
    downloaded.saturating_sub(last_progress) >= options.progress_step_bytes
        || total_bytes.is_some_and(|total| downloaded >= total)
}
