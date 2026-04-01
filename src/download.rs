// Download manager — streams an HTTP file to disk, computes SHA256 incrementally,
// and verifies the checksum before committing the result. Uses indicatif for a
// live progress bar (bytes, speed, ETA). One retry on transient failure.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use futures_util::StreamExt;
use indicatif::{ProgressBar, ProgressStyle};
use sha2::{Digest, Sha256};
use tracing::{debug, info};

use crate::error::ApmError;

/// Download `url` to `dest`, verifying the SHA256 checksum on completion.
///
/// Creates its own progress bar. For multi-format installs where a
/// [`MultiProgress`](indicatif::MultiProgress) is managing display, use
/// [`download_file_with_progress`] instead.
///
/// - Streams the response, computing the hash incrementally so no second pass
///   over the file is needed.
/// - Writes to a sibling `.tmp` file first; renames to `dest` only on success.
/// - On checksum mismatch: deletes the temp file and returns `ApmError::Checksum`.
/// - On network error: retries once before surfacing the error.
#[allow(dead_code)]
pub async fn download_file(
    url: &str,
    dest: &Path,
    expected_sha256: &str,
) -> Result<()> {
    let pb = build_standalone_progress_bar(None);
    download_file_with_progress(url, dest, expected_sha256, pb).await
}

/// Download `url` to `dest` using the supplied `ProgressBar`.
///
/// The progress bar is updated as bytes arrive and finished on completion.
/// This variant is intended for use from the install orchestrator, which
/// manages a [`MultiProgress`](indicatif::MultiProgress) container so that
/// per-format bars render correctly alongside each other.
pub async fn download_file_with_progress(
    url: &str,
    dest: &Path,
    expected_sha256: &str,
    pb: ProgressBar,
) -> Result<()> {
    debug!("Downloading {} → {}", url, dest.display());

    // Attempt with one retry.
    match attempt_download(url, dest, expected_sha256, &pb).await {
        Ok(()) => Ok(()),
        Err(first_err) => {
            // Only retry on network / transient errors, not checksum mismatches.
            if first_err
                .downcast_ref::<ApmError>()
                .map(|e| matches!(e, ApmError::Checksum { .. }))
                .unwrap_or(false)
            {
                return Err(first_err);
            }

            tracing::warn!(
                "Download failed ({}); retrying once…",
                first_err
            );

            // Reset the bar for the retry.
            pb.set_position(0);

            attempt_download(url, dest, expected_sha256, &pb)
                .await
                .with_context(|| format!("Download failed after retry: {url}"))
        }
    }
}

async fn attempt_download(
    url: &str,
    dest: &Path,
    expected_sha256: &str,
    pb: &ProgressBar,
) -> Result<()> {
    // Ensure destination parent exists.
    if let Some(parent) = dest.parent() {
        crate::config::ensure_dir(parent)
            .with_context(|| format!("Cannot create download directory: {}", parent.display()))?;
    }

    let tmp_path = temp_path(dest);

    // Build client with a generous timeout for large plugin archives.
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(600))
        .build()
        .context("Failed to build HTTP client")?;

    let response = client
        .get(url)
        .send()
        .await
        .map_err(|e| {
            let reason = e.to_string();
            let u = url.to_owned();
            if e.is_connect() || e.is_timeout() {
                ApmError::Network { reason }
            } else {
                ApmError::Download { url: u, reason }
            }
        })
        .with_context(|| format!("Failed to start download from {url}"))?;

    if !response.status().is_success() {
        let status = response.status();
        let hint = match status.as_u16() {
            404 => "The download URL for this plugin is no longer valid. The registry entry may be outdated.".to_owned(),
            429 => "Download rate limited by vendor server. Try again in a few minutes.".to_owned(),
            s if s >= 500 => "Vendor server error. Try again later.".to_owned(),
            _ => "Check your network connection and try again.".to_owned(),
        };
        anyhow::bail!(
            "HTTP {} from {}\n  Hint: {}",
            status,
            url,
            hint
        );
    }

    // Update progress bar length if we now know the content length.
    if let Some(len) = response.content_length() {
        pb.set_length(len);
    }

    // Stream body, hash incrementally, write to temp file.
    let mut hasher = Sha256::new();
    let mut file = tokio::fs::File::create(&tmp_path)
        .await
        .with_context(|| format!("Cannot create temp file: {}", tmp_path.display()))?;

    let mut stream = response.bytes_stream();
    let mut bytes_written: u64 = 0;

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| ApmError::Download {
            url: url.to_owned(),
            reason: e.to_string(),
        })?;

        hasher.update(&chunk);

        {
            use tokio::io::AsyncWriteExt;
            file.write_all(&chunk)
                .await
                .with_context(|| format!("Write error on temp file: {}", tmp_path.display()))?;
        }

        bytes_written += chunk.len() as u64;
        pb.set_position(bytes_written);
    }

    {
        use tokio::io::AsyncWriteExt;
        file.flush()
            .await
            .with_context(|| format!("Flush error on temp file: {}", tmp_path.display()))?;
    }
    drop(file);

    pb.finish_and_clear();

    // Verify checksum.
    let actual_hex = hex::encode(hasher.finalize());
    let expected_lower = expected_sha256.to_lowercase();
    let actual_lower = actual_hex.to_lowercase();

    if expected_lower != actual_lower {
        // Delete the corrupt temp file before surfacing the error.
        let _ = std::fs::remove_file(&tmp_path);
        return Err(ApmError::Checksum {
            expected: expected_sha256.to_owned(),
            actual: actual_hex,
        }
        .into());
    }

    debug!("SHA256 OK: {actual_hex}");

    // Atomically move the verified file into its final location.
    std::fs::rename(&tmp_path, dest).with_context(|| {
        format!(
            "Cannot rename temp file {} → {}",
            tmp_path.display(),
            dest.display()
        )
    })?;

    info!("Downloaded {} ({} bytes)", dest.display(), bytes_written);
    Ok(())
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Returns a sibling `.tmp` path for atomic writes.
fn temp_path(dest: &Path) -> PathBuf {
    let mut tmp = dest.to_path_buf();
    let stem = dest
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("download");
    tmp.set_file_name(format!("{stem}.tmp"));
    tmp
}

/// Build a standalone progress bar (not attached to a MultiProgress).
fn build_standalone_progress_bar(total_bytes: Option<u64>) -> ProgressBar {
    let pb = if let Some(total) = total_bytes {
        ProgressBar::new(total)
    } else {
        ProgressBar::new_spinner()
    };

    let style = ProgressStyle::with_template(
        "  {msg:>12} [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({bytes_per_sec}, ETA {eta})",
    )
    .unwrap_or_else(|_| ProgressStyle::default_bar())
    .progress_chars("=>-");

    pb.set_style(style);
    pb.set_message("downloading");
    pb
}
