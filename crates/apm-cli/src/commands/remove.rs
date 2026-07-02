// remove command — thin CLI wrapper over the shared lifecycle engine.

use anyhow::Result;
use colored::Colorize;
use serde_json::json;

use apm_core::config::Config;
use apm_core::engine::{
    ApmEngine, NoopEventSink, RemoveFormatSummary, RemovePackageRequest, RemovePackageResult,
};

pub async fn run(config: &Config, name: &str, json: bool, dry_run: bool) -> Result<()> {
    let engine = ApmEngine::new(config.clone());
    let result = engine.remove_package(
        RemovePackageRequest {
            slug: name.to_string(),
            dry_run,
        },
        &mut NoopEventSink,
    )?;

    if json {
        print_json(&result);
    } else {
        print_text(&result);
    }
    Ok(())
}

fn print_json(result: &RemovePackageResult) {
    match result {
        RemovePackageResult::NotInstalled { slug } => {
            println!(
                "{}",
                json!({ "removed": false, "plugin": slug, "reason": "not installed" })
            );
        }
        RemovePackageResult::ExternalInstallPresent { package, reason } => {
            println!(
                "{}",
                json!({ "removed": false, "plugin": package.slug, "reason": reason })
            );
        }
        RemovePackageResult::DryRun {
            package,
            formats,
            would_delete_files,
            reason,
        } => {
            println!(
                "{}",
                json!({
                    "dry_run": true,
                    "would_remove": true,
                    "would_delete_files": would_delete_files,
                    "plugin": package.slug,
                    "version": package.version,
                    "reason": reason,
                    "formats": json_format_entries(formats),
                })
            );
        }
        RemovePackageResult::Removed {
            package,
            removed_formats,
            state_only,
        } => {
            println!(
                "{}",
                json!({
                    "removed": true,
                    "plugin": package.slug,
                    "version": package.version,
                    "state_only": state_only,
                    "formats_removed": json_format_entries(removed_formats),
                })
            );
        }
    }
}

fn print_text(result: &RemovePackageResult) {
    match result {
        RemovePackageResult::NotInstalled { slug } => {
            println!(
                "Plugin '{}' is not installed via apm. Nothing to remove.",
                slug
            );
        }
        RemovePackageResult::ExternalInstallPresent { package, .. } => {
            println!(
                "{} was discovered by `apm scan`; apm will not delete externally installed files.",
                package.slug.bold()
            );
            println!(
                "Remove it with the vendor installer or Finder first, then run `apm remove {}` to clean apm state.",
                package.slug
            );
        }
        RemovePackageResult::DryRun {
            package,
            formats,
            would_delete_files,
            reason,
        } => {
            if *would_delete_files {
                println!(
                    "[dry-run] Would remove {} v{}",
                    package.slug.bold(),
                    package.version.cyan()
                );
                println!("          Formats: {}", text_format_entries(formats));
            } else {
                println!(
                    "[dry-run] Would remove stale external state entry for {}.",
                    package.slug.bold()
                );
                println!(
                    "          {}",
                    reason.as_deref().unwrap_or("No files would be deleted.")
                );
            }
        }
        RemovePackageResult::Removed {
            package,
            removed_formats,
            state_only,
        } => {
            if *state_only {
                println!(
                    "{}",
                    format!(
                        "Removed stale external state entry for {}. No plugin files were deleted.",
                        package.slug
                    )
                    .green()
                );
            } else {
                println!(
                    "{}",
                    format!(
                        "Removed {} v{} ({})",
                        package.slug,
                        package.version,
                        removed_format_names(removed_formats)
                    )
                    .green()
                );
            }
        }
    }
}

fn json_format_entries(formats: &[RemoveFormatSummary]) -> Vec<serde_json::Value> {
    formats
        .iter()
        .map(|format| {
            json!({
                "format": format.format.to_string(),
                "path": format.path.display().to_string(),
                "existed": format.existed,
            })
        })
        .collect()
}

fn text_format_entries(formats: &[RemoveFormatSummary]) -> String {
    formats
        .iter()
        .map(|format| format!("{} ({})", format.format, format.path.display()))
        .collect::<Vec<_>>()
        .join(", ")
}

fn removed_format_names(formats: &[RemoveFormatSummary]) -> String {
    let names: Vec<String> = formats
        .iter()
        .filter(|format| format.existed)
        .map(|format| format.format.to_string())
        .collect();
    if names.is_empty() {
        "state only".to_string()
    } else {
        names.join(", ")
    }
}
