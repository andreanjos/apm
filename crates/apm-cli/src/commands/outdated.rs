// outdated command — thin CLI wrapper over the shared update-read engine.

use anyhow::Result;
use colored::Colorize;
use serde::Serialize;

use apm_core::config::Config;
use apm_core::engine::{
    ApmEngine, AvailableUpdatesRequest, AvailableUpdatesResult, PackageUpdateAction,
    PackageUpdateSummary,
};

#[derive(Serialize)]
struct OutdatedResultJson {
    outdated: Vec<OutdatedPluginJson>,
    up_to_date_count: usize,
    pinned_count: usize,
}

#[derive(Serialize)]
struct OutdatedPluginJson {
    name: String,
    installed: String,
    available: String,
    pinned: bool,
}

pub async fn run(config: &Config, json: bool) -> Result<()> {
    let engine = ApmEngine::new(config.clone());
    let AvailableUpdatesResult::Ready {
        installed_count,
        updates,
        up_to_date_count,
        pinned_count,
        ..
    } = engine.available_updates(AvailableUpdatesRequest)?
    else {
        anyhow::bail!(
            "Registry cache is empty.\n\
             Hint: Run `apm sync` to populate the local registry cache."
        );
    };

    if installed_count == 0 {
        if json {
            println!(
                "{}",
                serde_json::to_string_pretty(&OutdatedResultJson {
                    outdated: Vec::new(),
                    up_to_date_count: 0,
                    pinned_count: 0,
                })?
            );
        } else {
            println!("No plugins installed via apm.");
        }
        return Ok(());
    }

    if json {
        print_json(&updates, up_to_date_count, pinned_count)?;
    } else {
        print_text(&updates, up_to_date_count, pinned_count);
    }
    Ok(())
}

fn print_json(
    updates: &[PackageUpdateSummary],
    up_to_date_count: usize,
    pinned_count: usize,
) -> Result<()> {
    let result = OutdatedResultJson {
        outdated: updates
            .iter()
            .map(|update| OutdatedPluginJson {
                name: update.slug.clone(),
                installed: update.installed_version.clone(),
                available: update.available_version.clone(),
                pinned: update.pinned,
            })
            .collect(),
        up_to_date_count,
        pinned_count,
    };
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}

fn print_text(updates: &[PackageUpdateSummary], up_to_date_count: usize, pinned_count: usize) {
    if updates.is_empty() {
        println!("All {} plugins are up to date.", up_to_date_count);
        return;
    }

    let col_name = updates
        .iter()
        .map(|update| update.slug.len())
        .max()
        .unwrap_or(6)
        .max(6);
    let col_inst = updates
        .iter()
        .map(|update| update.installed_version.len())
        .max()
        .unwrap_or(9)
        .max(9);
    let col_avail = updates
        .iter()
        .map(|update| update.available_version.len())
        .max()
        .unwrap_or(9)
        .max(9);

    println!(
        "{}",
        format!(
            "{:<col_name$}  {:<col_inst$}  {:<col_avail$}  Status",
            "Plugin",
            "Installed",
            "Available",
            col_name = col_name,
            col_inst = col_inst,
            col_avail = col_avail,
        )
        .bold()
    );

    let total_width = col_name + 2 + col_inst + 2 + col_avail + 2 + 8;
    println!("{}", "\u{2500}".repeat(total_width).dimmed());

    for update in updates {
        println!(
            "{:<col_name$}  {:<col_inst$}  {:<col_avail$}  {}",
            update.slug.bold().to_string(),
            update.installed_version.cyan().to_string(),
            update.available_version.green().to_string(),
            status_label(update),
            col_name = col_name,
            col_inst = col_inst,
            col_avail = col_avail,
        );
    }

    let upgradeable = updates
        .iter()
        .filter(|update| update.action == PackageUpdateAction::Installable)
        .count();
    let mut summary_parts = vec![format!(
        "{} outdated, {} up to date",
        updates.len(),
        up_to_date_count
    )];
    if pinned_count > 0 {
        summary_parts.push(format!("{} pinned", pinned_count));
    }
    println!("\n{}", summary_parts.join(", ").dimmed());

    if upgradeable > 0 {
        println!(
            "{}",
            format!("{upgradeable} plugin(s) can be upgraded. Run 'apm upgrade' to upgrade all.")
                .yellow()
        );
    } else {
        println!(
            "{}",
            "No outdated apm-managed plugins can be upgraded.".yellow()
        );
    }
}

fn status_label(update: &PackageUpdateSummary) -> String {
    match update.action {
        PackageUpdateAction::Installable => String::new(),
        PackageUpdateAction::Pinned => "pinned".yellow().to_string(),
        PackageUpdateAction::External => "external".yellow().to_string(),
    }
}
