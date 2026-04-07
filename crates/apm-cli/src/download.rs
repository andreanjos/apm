// Download manager — streams an HTTP file to disk, computes SHA256 incrementally,
// and verifies the checksum before committing the result. Uses indicatif for a
// live progress bar (bytes, speed, ETA). One retry on transient failure.
// Supports download caching (skip re-download if cached) and resumable downloads
// via HTTP Range requests.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use futures_util::StreamExt;
use indicatif::{ProgressBar, ProgressStyle};
use sha2::{Digest, Sha256};
use tracing::{debug, info};

use apm_core::config::Config;
use apm_core::error::ApmError;

// ── Placeholder SHA256 detection ──────────────────────────────────────────────

/// Returns `true` when the sha256 value is an empty/placeholder that should
/// not be used for caching or verification.
fn is_placeholder_sha256(sha256: &str) -> bool {
    let s = sha256.trim();
    s.is_empty() || s.eq_ignore_ascii_case("manual") || s.chars().all(|c| c == '0')
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Download `url` to `dest`, verifying the SHA256 checksum on completion.
///
/// Creates its own progress bar. For multi-format installs where a
/// [`MultiProgress`](indicatif::MultiProgress) is managing display, use
/// [`download_file_with_progress`] instead.
///
/// - Streams the response, computing the hash incrementally so no second pass
///   over the file is needed.
/// - Writes to a sibling `.part` file first; renames to `dest` only on success.
/// - Resumes partial downloads via HTTP Range header if a `.part` file exists.
/// - On checksum mismatch: deletes the temp file and returns `ApmError::Checksum`.
/// - On network error: retries once before surfacing the error.
#[allow(dead_code)]
pub async fn download_file(url: &str, dest: &Path, expected_sha256: &str) -> Result<()> {
    let pb = build_standalone_progress_bar(None);
    download_file_with_progress(url, dest, expected_sha256, pb).await
}

/// Download `url` to `dest` using the supplied `ProgressBar`.
///
/// The progress bar is updated as bytes arrive and finished on completion.
/// This variant is intended for use from the install orchestrator, which
/// manages a [`MultiProgress`](indicatif::MultiProgress) container so that
/// per-format bars render correctly alongside each other.
///
/// When `config` is provided, the download cache is consulted first. A cached
/// copy matching the expected SHA256 will be used directly without a network
/// request. On a fresh download the verified file is saved to the cache.
pub async fn download_file_with_progress(
    url: &str,
    dest: &Path,
    expected_sha256: &str,
    pb: ProgressBar,
) -> Result<()> {
    download_file_with_progress_and_config(url, dest, expected_sha256, pb, None).await
}

/// Like [`download_file_with_progress`] but also accepts a [`Config`] reference
/// so the download cache can be used.
pub async fn download_file_with_progress_cached(
    url: &str,
    dest: &Path,
    expected_sha256: &str,
    pb: ProgressBar,
    config: &Config,
) -> Result<()> {
    download_file_with_progress_and_config(url, dest, expected_sha256, pb, Some(config)).await
}

async fn download_file_with_progress_and_config(
    url: &str,
    dest: &Path,
    expected_sha256: &str,
    pb: ProgressBar,
    config: Option<&Config>,
) -> Result<()> {
    debug!("Downloading {} → {}", url, dest.display());

    // ── Check download cache ──────────────────────────────────────────────────

    if let Some(cfg) = config {
        if !is_placeholder_sha256(expected_sha256) {
            let cache_path = cache_file_path(cfg, expected_sha256);
            if cache_path.exists() {
                // Validate the cached file's hash before trusting it.
                if file_sha256_matches(&cache_path, expected_sha256) {
                    pb.finish_and_clear();
                    // Derive plugin name from dest filename for the message.
                    let plugin_name = dest
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("plugin");
                    println!("  Using cached download for {plugin_name}");
                    if let Some(parent) = dest.parent() {
                        apm_core::config::ensure_dir(parent).with_context(|| {
                            format!("Cannot create download directory: {}", parent.display())
                        })?;
                    }
                    std::fs::copy(&cache_path, dest).with_context(|| {
                        format!(
                            "Cannot copy cached file {} → {}",
                            cache_path.display(),
                            dest.display()
                        )
                    })?;
                    info!("Used cache for {}", dest.display());
                    return Ok(());
                } else {
                    // Cache entry is corrupt — remove it and re-download.
                    debug!(
                        "Cached file {} has wrong hash; removing and re-downloading.",
                        cache_path.display()
                    );
                    let _ = std::fs::remove_file(&cache_path);
                }
            }
        }
    }

    // ── Download (with one retry on transient error) ──────────────────────────

    match attempt_download(url, dest, expected_sha256, &pb).await {
        Ok(()) => {
            // Save to cache after a successful verified download.
            if let Some(cfg) = config {
                if !is_placeholder_sha256(expected_sha256) {
                    save_to_cache(cfg, dest, expected_sha256);
                }
            }
            Ok(())
        }
        Err(first_err) => {
            // Only retry on network / transient errors, not checksum mismatches.
            if first_err
                .downcast_ref::<ApmError>()
                .map(|e| matches!(e, ApmError::Checksum { .. }))
                .unwrap_or(false)
            {
                return Err(first_err);
            }

            tracing::warn!("Download failed ({}); retrying once…", first_err);

            // Reset the bar for the retry.
            pb.set_position(0);

            let result = attempt_download(url, dest, expected_sha256, &pb)
                .await
                .with_context(|| format!("Download failed after retry: {url}"));

            if result.is_ok() {
                if let Some(cfg) = config {
                    if !is_placeholder_sha256(expected_sha256) {
                        save_to_cache(cfg, dest, expected_sha256);
                    }
                }
            }

            result
        }
    }
}

// ── Core download logic ───────────────────────────────────────────────────────

async fn attempt_download(
    url: &str,
    dest: &Path,
    expected_sha256: &str,
    pb: &ProgressBar,
) -> Result<()> {
    // Ensure destination parent exists.
    if let Some(parent) = dest.parent() {
        apm_core::config::ensure_dir(parent)
            .with_context(|| format!("Cannot create download directory: {}", parent.display()))?;
    }

    let part_path = part_path(dest);

    // ── Check for an existing .part file (resume support) ─────────────────────

    let resume_offset: u64 = if part_path.exists() {
        part_path.metadata().map(|m| m.len()).unwrap_or(0)
    } else {
        0
    };

    // Build client with a generous timeout for large plugin archives.
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(600))
        .build()
        .context("Failed to build HTTP client")?;

    let mut request = client.get(url);
    if resume_offset > 0 {
        debug!("Resuming download from byte {resume_offset}");
        request = request.header("Range", format!("bytes={resume_offset}-"));
    }

    let response = request
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

    let status = response.status();

    if !status.is_success() {
        let hint = match status.as_u16() {
            404 => "The download URL for this plugin is no longer valid. The registry entry may be outdated.".to_owned(),
            429 => "Download rate limited by vendor server. Try again in a few minutes.".to_owned(),
            s if s >= 500 => "Vendor server error. Try again later.".to_owned(),
            _ => "Check your network connection and try again.".to_owned(),
        };
        anyhow::bail!("HTTP {} from {}\n  Hint: {}", status, url, hint);
    }

    // Determine whether the server accepted our Range request.
    let is_partial = status == reqwest::StatusCode::PARTIAL_CONTENT;

    // If we asked for a range but the server returned 200 (no range support),
    // discard the .part file and start fresh.
    let (mut hasher, mut bytes_written, file) = if is_partial && resume_offset > 0 {
        // Resuming: seed the hasher with the already-downloaded bytes.
        let existing_hash = hash_file_sync(&part_path)?;
        let file = tokio::fs::OpenOptions::new()
            .append(true)
            .open(&part_path)
            .await
            .with_context(|| {
                format!(
                    "Cannot open .part file for appending: {}",
                    part_path.display()
                )
            })?;

        // Update progress bar to show resumed state.
        let total = response.content_length().map(|cl| cl + resume_offset);
        if let Some(total) = total {
            pb.set_length(total);
        }
        pb.set_position(resume_offset);

        (existing_hash, resume_offset, file)
    } else {
        // Fresh start — remove stale .part if it exists.
        if part_path.exists() {
            let _ = std::fs::remove_file(&part_path);
        }

        if let Some(len) = response.content_length() {
            pb.set_length(len);
        }

        let file = tokio::fs::File::create(&part_path)
            .await
            .with_context(|| format!("Cannot create .part file: {}", part_path.display()))?;

        (Sha256::new(), 0u64, file)
    };

    // Stream body, hash incrementally, write to .part file.
    let mut stream = response.bytes_stream();
    let mut file = file;

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
                .with_context(|| format!("Write error on .part file: {}", part_path.display()))?;
        }

        bytes_written += chunk.len() as u64;
        pb.set_position(bytes_written);
    }

    {
        use tokio::io::AsyncWriteExt;
        file.flush()
            .await
            .with_context(|| format!("Flush error on .part file: {}", part_path.display()))?;
    }
    drop(file);

    pb.finish_and_clear();

    // Verify checksum.
    let actual_hex = hex::encode(hasher.finalize());
    let expected_lower = expected_sha256.to_lowercase();
    let actual_lower = actual_hex.to_lowercase();

    if !expected_lower.is_empty() && expected_lower != actual_lower {
        // Delete the corrupt .part file before surfacing the error.
        let _ = std::fs::remove_file(&part_path);
        return Err(ApmError::Checksum {
            expected: expected_sha256.to_owned(),
            actual: actual_hex,
        }
        .into());
    }

    debug!("SHA256 OK: {actual_hex}");

    // Atomically move the verified file into its final location.
    std::fs::rename(&part_path, dest).with_context(|| {
        format!(
            "Cannot rename .part file {} → {}",
            part_path.display(),
            dest.display()
        )
    })?;

    info!("Downloaded {} ({} bytes)", dest.display(), bytes_written);
    Ok(())
}

// ── Cache helpers ─────────────────────────────────────────────────────────────

/// Returns the cache file path for a given SHA256 hash.
fn cache_file_path(config: &Config, sha256: &str) -> PathBuf {
    config
        .downloads_cache_dir()
        .join(format!("{sha256}.archive"))
}

/// Copy a freshly-downloaded (and verified) file into the download cache.
/// Best-effort: silently ignores errors (cache miss is never fatal).
fn save_to_cache(config: &Config, src: &Path, sha256: &str) {
    let cache_dir = config.downloads_cache_dir();
    if let Err(e) = apm_core::config::ensure_dir(&cache_dir) {
        debug!("Could not create cache dir: {e}");
        return;
    }
    let dest = cache_file_path(config, sha256);
    if let Err(e) = std::fs::copy(src, &dest) {
        debug!("Could not save to cache {}: {e}", dest.display());
    } else {
        debug!("Saved to cache: {}", dest.display());
    }
}

/// Returns `true` when the file at `path` has the expected SHA256 hex digest.
fn file_sha256_matches(path: &Path, expected: &str) -> bool {
    match hash_file_sync(path) {
        Ok(hasher) => {
            let actual = hex::encode(hasher.finalize());
            actual.to_lowercase() == expected.to_lowercase()
        }
        Err(_) => false,
    }
}

/// Read a file synchronously and return a finalised `Sha256` hasher.
fn hash_file_sync(path: &Path) -> Result<Sha256> {
    use std::io::Read;

    let mut file = std::fs::File::open(path)
        .with_context(|| format!("Cannot open file for hashing: {}", path.display()))?;

    let mut hasher = Sha256::new();
    let mut buf = [0u8; 65536];

    loop {
        let n = file
            .read(&mut buf)
            .with_context(|| format!("Read error while hashing: {}", path.display()))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }

    Ok(hasher)
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Returns a sibling `.part` path for resumable in-progress writes.
fn part_path(dest: &Path) -> PathBuf {
    let mut p = dest.to_path_buf();
    let stem = dest
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("download");
    p.set_file_name(format!("{stem}.part"));
    p
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
