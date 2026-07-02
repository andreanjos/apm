use std::path::Path;

use anyhow::Result;
use colored::Colorize;
use serde::Serialize;

use apm_core::config::Config;
use apm_core::engine::{
    ApmEngine, NoopEventSink, ScanMatchMethod, ScanPackageFilter, ScanPackagesRequest,
    ScannedPackageSummary,
};
use apm_core::state::InstallOrigin;

use crate::utils::{display_path, truncate};

const MAX_NAME: usize = 35;
const MAX_VER: usize = 12;
const MAX_VENDOR: usize = 25;

#[derive(Serialize)]
struct ScannedPluginJson {
    name: String,
    version: String,
    vendor: String,
    format: String,
    path: String,
    managed_by_apm: bool,
    tracked_by_apm: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    origin: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    registry_slug: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    match_method: Option<String>,
}

pub async fn run(config: &Config, json: bool, managed: bool, unmanaged: bool) -> Result<()> {
    let result = ApmEngine::new(config.clone()).scan_packages(
        ScanPackagesRequest {
            filter: scan_filter(managed, unmanaged),
            learn_bundle_ids: !json,
            adopt_external: !json && !managed && !unmanaged,
        },
        &mut NoopEventSink,
    )?;

    if result.plugins.is_empty() {
        print_empty_scan(json, managed, unmanaged, result.scanned_count);
        return Ok(());
    }

    if json {
        let results = result
            .plugins
            .iter()
            .map(scanned_plugin_json)
            .collect::<Vec<_>>();
        println!("{}", serde_json::to_string_pretty(&results)?);
        return Ok(());
    }

    print_scan_table(&result.plugins);
    print_scan_summary(
        result.visible_count,
        result.au_count,
        result.vst3_count,
        result.adopted_count,
    );

    Ok(())
}

fn scan_filter(managed: bool, unmanaged: bool) -> ScanPackageFilter {
    if managed {
        ScanPackageFilter::Tracked
    } else if unmanaged {
        ScanPackageFilter::Untracked
    } else {
        ScanPackageFilter::All
    }
}

fn print_empty_scan(json: bool, managed: bool, unmanaged: bool, scanned_count: usize) {
    if json {
        println!("[]");
    } else if managed {
        println!("No apm-managed plugins found.");
    } else if unmanaged {
        println!("No unmanaged (third-party) plugins found.");
    } else if scanned_count == 0 {
        println!("No audio plugins found in configured directories.");
    }
}

fn scanned_plugin_json(plugin: &ScannedPackageSummary) -> ScannedPluginJson {
    ScannedPluginJson {
        name: plugin.name.clone(),
        version: plugin.version.clone(),
        vendor: plugin.vendor.clone(),
        format: plugin.format.to_string(),
        path: plugin.path.clone(),
        managed_by_apm: plugin.origin.is_some(),
        tracked_by_apm: plugin.tracked_by_apm,
        origin: plugin.origin.map(|origin| origin.to_string()),
        registry_slug: plugin.registry_slug.clone(),
        match_method: plugin.match_method.map(scan_match_method_json),
    }
}

fn scan_match_method_json(method: ScanMatchMethod) -> String {
    match method {
        ScanMatchMethod::BundleId => "bundle_id",
        ScanMatchMethod::NameVendor => "name_vendor",
        ScanMatchMethod::NameOnly => "name_only",
    }
    .to_string()
}

fn print_scan_table(plugins: &[ScannedPackageSummary]) {
    const HDR_NAME: &str = "Name";
    const HDR_VER: &str = "Version";
    const HDR_VENDOR: &str = "Vendor";
    const HDR_FMT: &str = "Format";
    const HDR_SRC: &str = "Source";
    const HDR_LOC: &str = "Location";

    let w_name = plugins
        .iter()
        .map(|p| p.name.len().min(MAX_NAME))
        .max()
        .unwrap_or(0)
        .max(HDR_NAME.len());
    let w_ver = plugins
        .iter()
        .map(|p| p.version.len().min(MAX_VER))
        .max()
        .unwrap_or(0)
        .max(HDR_VER.len());
    let w_vendor = plugins
        .iter()
        .map(|p| p.vendor.len().min(MAX_VENDOR))
        .max()
        .unwrap_or(0)
        .max(HDR_VENDOR.len());
    let w_fmt = HDR_FMT.len();
    let w_src = HDR_SRC.len();

    println!(
        "{}",
        format!(
            "{:<w_name$}  {:<w_ver$}  {:<w_vendor$}  {:<w_fmt$}  {:<w_src$}  {}",
            HDR_NAME, HDR_VER, HDR_VENDOR, HDR_FMT, HDR_SRC, HDR_LOC,
        )
        .bold()
    );

    let rule_len = w_name + 2 + w_ver + 2 + w_vendor + 2 + w_fmt + 2 + w_src + 2 + HDR_LOC.len();
    println!("{}", "\u{2500}".repeat(rule_len).dimmed());

    for plugin in plugins {
        println!(
            "{:<w_name$}  {:<w_ver$}  {:<w_vendor$}  {:<w_fmt$}  {:<w_src$}  {}",
            truncate(&plugin.name, MAX_NAME).bold().to_string(),
            truncate(&plugin.version, MAX_VER).cyan().to_string(),
            truncate(&plugin.vendor, MAX_VENDOR),
            plugin.format.to_string(),
            source_cell(plugin.origin),
            display_path(Path::new(&plugin.path)).dimmed(),
        );
    }
}

fn source_cell(origin: Option<InstallOrigin>) -> String {
    match origin {
        Some(InstallOrigin::Apm) => "apm".green().to_string(),
        Some(InstallOrigin::External) => "external".yellow().to_string(),
        None => "-".dimmed().to_string(),
    }
}

fn print_scan_summary(
    plugin_count: usize,
    au_count: usize,
    vst3_count: usize,
    adopted_count: usize,
) {
    println!();
    println!(
        "{}",
        format!(
            "Found {} plugin{} ({} AU, {} VST3)",
            plugin_count,
            if plugin_count == 1 { "" } else { "s" },
            au_count,
            vst3_count,
        )
        .dimmed()
    );

    if adopted_count > 0 {
        println!(
            "{}",
            format!(
                "Tracked {adopted_count} registry-matched external plugin{} in apm state.",
                if adopted_count == 1 { "" } else { "s" }
            )
            .green()
        );
    }
}
