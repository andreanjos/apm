// cleanup command — scan the downloads cache, report size, and optionally
// delete all cached archives and any empty directories left behind.

use anyhow::{Context, Result};
use colored::Colorize;

use apm_core::config::Config;

pub async fn run(config: &Config, dry_run: bool) -> Result<()> {
    let cache_dir = config.downloads_cache_dir();

    if !cache_dir.exists() {
        println!("Downloads cache directory does not exist. Nothing to clean up.");
        return Ok(());
    }

    // ── Collect cached files ──────────────────────────────────────────────────

    let mut files: Vec<std::path::PathBuf> = Vec::new();
    let mut total_bytes: u64 = 0;

    // Use walkdir to find all files (including nested if any) in the cache dir.
    for entry in walkdir::WalkDir::new(&cache_dir)
        .min_depth(1)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if entry.file_type().is_file() {
            let len = entry.metadata().map(|m| m.len()).unwrap_or(0);
            total_bytes += len;
            files.push(entry.into_path());
        }
    }

    let n = files.len();

    if n == 0 {
        println!("Downloads cache is already empty. Nothing to clean up.");
        return Ok(());
    }

    let total_mb = total_bytes as f64 / 1_048_576.0;

    // ── Dry-run: report without deleting ─────────────────────────────────────

    if dry_run {
        println!(
            "[dry-run] Would remove {} cached download{} ({:.1} MB):",
            n,
            if n == 1 { "" } else { "s" },
            total_mb
        );
        for path in &files {
            let display = shorten_path(path);
            println!("  {}", display.dimmed());
        }
        println!(
            "\n[dry-run] Run {} to actually free the space.",
            "apm cleanup".bold()
        );
        return Ok(());
    }

    // ── Delete files ──────────────────────────────────────────────────────────

    let mut removed = 0usize;
    let mut freed_bytes: u64 = 0;

    for path in &files {
        let size = path.metadata().map(|m| m.len()).unwrap_or(0);
        std::fs::remove_file(path)
            .with_context(|| format!("Failed to remove cached file: {}", path.display()))?;
        freed_bytes += size;
        removed += 1;
    }

    // ── Remove empty subdirectories (if any were created) ────────────────────

    remove_empty_dirs(&cache_dir);

    // ── Summary ──────────────────────────────────────────────────────────────

    let freed_mb = freed_bytes as f64 / 1_048_576.0;

    println!(
        "{}",
        format!(
            "Removed {} cached download{}, freed {:.1} MB.",
            removed,
            if removed == 1 { "" } else { "s" },
            freed_mb
        )
        .green()
    );

    Ok(())
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Walk `dir` and remove any subdirectories that are now empty.
/// Best-effort: silently skips errors.
fn remove_empty_dirs(dir: &std::path::Path) {
    // Collect directory entries in reverse (deepest first) so we can remove
    // a leaf before its parent.
    let dirs: Vec<std::path::PathBuf> = walkdir::WalkDir::new(dir)
        .min_depth(1)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_dir())
        .map(|e| e.into_path())
        .collect();

    for d in dirs.into_iter().rev() {
        // Only remove if the directory is now empty.
        if d.read_dir()
            .map(|mut r| r.next().is_none())
            .unwrap_or(false)
        {
            let _ = std::fs::remove_dir(&d);
        }
    }
}

/// Replace the user's home directory prefix with `~` for compact display.
fn shorten_path(path: &std::path::Path) -> String {
    let s = path.to_string_lossy();
    if let Some(home) = dirs::home_dir() {
        let h = home.to_string_lossy();
        if let Some(rest) = s.strip_prefix(h.as_ref()) {
            return format!("~{rest}");
        }
    }
    s.into_owned()
}
