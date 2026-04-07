// check command — verify the integrity of an installed plugin.
//
// For each installed format of the plugin, checks:
// 1. Bundle exists on disk at the recorded path.
// 2. Bundle is not quarantined (com.apple.quarantine xattr).
//
// Reports a per-format status table and an overall health verdict.

use anyhow::{bail, Result};
use colored::Colorize;
use serde::Serialize;

use apm_core::config::Config;
use apm_core::state::InstallState;

use crate::utils::display_path;

// ── JSON output types ────────────────────────────────────────────────────────

#[derive(Serialize)]
struct CheckReport {
    plugin: String,
    version: String,
    healthy: bool,
    formats: Vec<FormatReport>,
}

#[derive(Serialize)]
struct FormatReport {
    format: String,
    path: String,
    exists: bool,
    quarantined: bool,
}

// ── Public entry point ───────────────────────────────────────────────────────

pub async fn run(config: &Config, name: &str, json: bool) -> Result<()> {
    let state = InstallState::load(config)?;

    let plugin = match state.find(name) {
        Some(p) => p,
        None => {
            bail!(
                "Plugin '{name}' is not installed (not tracked in apm state).\n\
                 Hint: Run `apm scan` to discover plugins on disk, or \
                 `apm install {name}` to install it.",
            );
        }
    };

    let mut format_reports: Vec<FormatReport> = Vec::new();
    let mut missing_count = 0usize;
    let mut quarantined_count = 0usize;

    for fmt in &plugin.formats {
        let exists = fmt.path.exists();
        let quarantined = if exists {
            is_quarantined(&fmt.path)
        } else {
            false
        };

        if !exists {
            missing_count += 1;
        }
        if quarantined {
            quarantined_count += 1;
        }

        format_reports.push(FormatReport {
            format: fmt.format.to_string().to_lowercase(),
            path: display_path(&fmt.path),
            exists,
            quarantined,
        });
    }

    let healthy = missing_count == 0 && quarantined_count == 0;

    if json {
        let report = CheckReport {
            plugin: plugin.name.clone(),
            version: plugin.version.clone(),
            healthy,
            formats: format_reports,
        };
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    // ── Human-readable output ────────────────────────────────────────────────

    println!("{} v{}", plugin.name, plugin.version);

    for fr in &format_reports {
        let format_label = fr.format.to_uppercase();
        let status = if !fr.exists {
            format!("{} missing", "\u{2717}".red())
        } else if fr.quarantined {
            format!("{} exists, {} quarantined", "\u{2713}".green(), "!".yellow())
        } else {
            format!("{} exists", "\u{2713}".green())
        };

        println!("  {:<4} {}   {}", format_label, fr.path, status);
    }

    if healthy {
        println!("  Status: {}", "healthy".green());
    } else {
        let mut issues = Vec::new();
        if missing_count > 0 {
            issues.push(format!("{missing_count} missing"));
        }
        if quarantined_count > 0 {
            issues.push(format!("{quarantined_count} quarantined"));
        }
        println!(
            "  Status: {}",
            format!("issues found ({})", issues.join(", ")).yellow()
        );
    }

    Ok(())
}

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Check whether a bundle has the com.apple.quarantine extended attribute.
///
/// Uses the same `xattr -l` approach as `apm doctor`.
fn is_quarantined(path: &std::path::Path) -> bool {
    match std::process::Command::new("xattr")
        .arg("-l")
        .arg(path)
        .output()
    {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            stdout.contains("com.apple.quarantine")
        }
        Err(_) => false,
    }
}
