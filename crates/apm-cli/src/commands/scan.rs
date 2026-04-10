use std::collections::HashMap;

use anyhow::Result;
use chrono::Utc;
use colored::Colorize;
use serde::Serialize;

use apm_core::bundle_id_store::BundleIdStore;
use apm_core::config::Config;
use apm_core::registry;
use apm_core::registry::matcher;
use apm_core::scanner::{self, PluginFormat};
use apm_core::state::{InstallOrigin, InstallState, InstalledFormat, InstalledPlugin};

use crate::utils::{display_path, truncate};

// Maximum column widths for the scan table.
const MAX_NAME: usize = 35;
const MAX_VER: usize = 12;
const MAX_VENDOR: usize = 25;

/// JSON-serializable view of a scanned plugin.
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

struct Matched {
    slug: Option<String>,
    method: Option<String>,
}

pub async fn run(config: &Config, json: bool, managed: bool, unmanaged: bool) -> Result<()> {
    let plugins = scanner::scan_plugins(config);

    if plugins.is_empty() {
        if json {
            println!("[]");
        } else {
            println!("No audio plugins found in configured directories.");
        }
        return Ok(());
    }

    // Load apm-managed install state for source annotation.
    // A missing or unreadable state file is treated as empty (no managed plugins).
    let mut state = InstallState::load(config).unwrap_or_default();

    // Load registry and bundle ID store for matching scanned plugins.
    let reg = registry::Registry::load_all_sources(config).ok();
    let mut bid_store = BundleIdStore::open(config).ok();

    // Apply --managed / --unmanaged filter.
    let plugins: Vec<_> = if managed {
        plugins
            .into_iter()
            .filter(|plugin| scanned_is_tracked(&state, plugin))
            .collect()
    } else if unmanaged {
        plugins
            .into_iter()
            .filter(|plugin| !scanned_is_tracked(&state, plugin))
            .collect()
    } else {
        plugins
    };

    if plugins.is_empty() {
        if json {
            println!("[]");
        } else if managed {
            println!("No apm-managed plugins found.");
        } else if unmanaged {
            println!("No unmanaged (third-party) plugins found.");
        }
        return Ok(());
    }

    // ── Match + auto-learn ──────────────────────────────────────────────────
    // Match all scanned plugins against the registry and auto-learn bundle IDs.
    let mut matches: Vec<Matched> = Vec::new();
    let mut learned = 0usize;

    for p in &plugins {
        let m = reg
            .as_ref()
            .and_then(|r| matcher::match_plugin(p, r, bid_store.as_ref()));

        if let Some(ref pm) = m {
            // Auto-learn for interactive scans. JSON mode stays read-only so
            // scripts can inspect plugin state without changing local cache data.
            if !json && pm.method != matcher::MatchMethod::BundleId {
                if let Some(ref mut store) = bid_store {
                    if matcher::auto_learn(p, &pm.registry_plugin.slug, store) {
                        learned += 1;
                    }
                }
            }
        }

        matches.push(Matched {
            slug: m.as_ref().map(|m| m.registry_plugin.slug.clone()),
            method: m.as_ref().map(|m| match m.method {
                matcher::MatchMethod::BundleId => "bundle_id".to_string(),
                matcher::MatchMethod::NameAndVendor => "name_vendor".to_string(),
                matcher::MatchMethod::NameOnly => "name_only".to_string(),
            }),
        });
    }

    // Persist any newly learned bundle IDs locally.
    if learned > 0 {
        if let Some(ref store) = bid_store {
            let _ = store.save();
        }
    }

    let adopted = if !json && !managed && !unmanaged {
        adopt_external_matches(config, &plugins, &matches, reg.as_ref(), &mut state)?
    } else {
        0
    };

    // ── JSON output ───────────────────────────────────────────────────────────
    if json {
        let results: Vec<ScannedPluginJson> = plugins
            .iter()
            .zip(matches.iter())
            .map(|(p, m)| ScannedPluginJson {
                name: p.name.clone(),
                version: p.version.clone(),
                vendor: p.vendor.clone(),
                format: p.format.to_string(),
                path: p.path.to_string_lossy().into_owned(),
                managed_by_apm: scanned_origin(&state, p).is_some(),
                tracked_by_apm: scanned_origin(&state, p).is_some(),
                origin: scanned_origin(&state, p).map(|origin| origin.to_string()),
                registry_slug: m.slug.clone(),
                match_method: m.method.clone(),
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&results)?);
        return Ok(());
    }

    // ── Column widths ─────────────────────────────────────────────────────────
    // Compute widths from data, capped at the defined maximums.

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

    // Format column is at most 4 chars ("VST3") — header wins.
    let w_fmt = HDR_FMT.len();
    // Source column: "apm" (3) or "-" (1) — header "Source" wins.
    let w_src = HDR_SRC.len();

    // ── Header ────────────────────────────────────────────────────────────────
    println!(
        "{}",
        format!(
            "{:<w_name$}  {:<w_ver$}  {:<w_vendor$}  {:<w_fmt$}  {:<w_src$}  {}",
            HDR_NAME, HDR_VER, HDR_VENDOR, HDR_FMT, HDR_SRC, HDR_LOC,
        )
        .bold()
    );

    let rule_len = w_name + 2 + w_ver + 2 + w_vendor + 2 + w_fmt + 2 + w_src + 2 + HDR_LOC.len();
    println!("{}", "\u{2500}".repeat(rule_len).dimmed()); // ─────

    // ── Rows ──────────────────────────────────────────────────────────────────
    for p in &plugins {
        // Display the path in a human-friendly way: abbreviate $HOME to ~
        let path_str = display_path(&p.path);

        let name_cell = truncate(&p.name, MAX_NAME);
        let ver_cell = truncate(&p.version, MAX_VER);
        let vendor_cell = truncate(&p.vendor, MAX_VENDOR);

        // Determine if this plugin was installed by apm: match by path (most
        // precise) or by name as a fallback.
        let source_cell = match scanned_origin(&state, p) {
            Some(InstallOrigin::Apm) => "apm".green().to_string(),
            Some(InstallOrigin::External) => "external".yellow().to_string(),
            None => "-".dimmed().to_string(),
        };

        println!(
            "{:<w_name$}  {:<w_ver$}  {:<w_vendor$}  {:<w_fmt$}  {:<w_src$}  {}",
            name_cell.bold().to_string(),
            ver_cell.cyan().to_string(),
            vendor_cell,
            p.format.to_string(),
            source_cell,
            path_str.dimmed(),
        );
    }

    // ── Summary ───────────────────────────────────────────────────────────────
    let n_au = plugins
        .iter()
        .filter(|p| p.format == PluginFormat::Au)
        .count();
    let n_vst3 = plugins
        .iter()
        .filter(|p| p.format == PluginFormat::Vst3)
        .count();

    println!();
    println!(
        "{}",
        format!(
            "Found {} plugin{} ({} AU, {} VST3)",
            plugins.len(),
            if plugins.len() == 1 { "" } else { "s" },
            n_au,
            n_vst3,
        )
        .dimmed()
    );

    if adopted > 0 {
        println!(
            "{}",
            format!(
                "Tracked {adopted} registry-matched external plugin{} in apm state.",
                if adopted == 1 { "" } else { "s" }
            )
            .green()
        );
    }

    Ok(())
}

fn scanned_is_tracked(state: &InstallState, plugin: &scanner::ScannedPlugin) -> bool {
    scanned_origin(state, plugin).is_some()
}

fn scanned_origin(state: &InstallState, plugin: &scanner::ScannedPlugin) -> Option<InstallOrigin> {
    state
        .plugins
        .iter()
        .find(|installed| {
            installed
                .formats
                .iter()
                .any(|format| format.path == plugin.path)
                || installed.name.eq_ignore_ascii_case(&plugin.name)
        })
        .map(|installed| installed.origin)
}

struct ExternalAdoption {
    slug: String,
    version: String,
    vendor: String,
    source: String,
    formats: Vec<InstalledFormat>,
}

fn adopt_external_matches(
    config: &Config,
    plugins: &[scanner::ScannedPlugin],
    matches: &[Matched],
    registry: Option<&registry::Registry>,
    state: &mut InstallState,
) -> Result<usize> {
    let Some(registry) = registry else {
        return Ok(0);
    };

    let mut by_slug: HashMap<String, ExternalAdoption> = HashMap::new();

    for (plugin, matched) in plugins.iter().zip(matches.iter()) {
        let Some(slug) = &matched.slug else {
            continue;
        };
        let Some(definition) = registry.find(slug) else {
            continue;
        };
        let Some(format) = registry_format(plugin.format) else {
            continue;
        };

        let entry = by_slug
            .entry(slug.clone())
            .or_insert_with(|| ExternalAdoption {
                slug: slug.clone(),
                version: if plugin.version == "unknown" {
                    definition.version.clone()
                } else {
                    plugin.version.clone()
                },
                vendor: definition.vendor.clone(),
                source: definition
                    .source_name
                    .clone()
                    .unwrap_or_else(|| "official".to_string()),
                formats: Vec::new(),
            });

        if entry.version == "unknown" && plugin.version != "unknown" {
            entry.version = plugin.version.clone();
        }

        if !entry
            .formats
            .iter()
            .any(|format_entry| format_entry.format == format && format_entry.path == plugin.path)
        {
            entry.formats.push(InstalledFormat {
                format,
                path: plugin.path.clone(),
                sha256: String::new(),
            });
        }
    }

    let mut changed = 0usize;
    for adoption in by_slug.into_values() {
        if adoption.formats.is_empty() {
            continue;
        }

        if let Some(existing) = state.find_mut(&adoption.slug) {
            if existing.origin == InstallOrigin::Apm {
                continue;
            }

            let before = existing.formats.len();
            for format in adoption.formats {
                if !existing.formats.iter().any(|existing_format| {
                    existing_format.format == format.format && existing_format.path == format.path
                }) {
                    existing.formats.push(format);
                }
            }
            if existing.formats.len() != before {
                changed += 1;
            }
            continue;
        }

        state.record_install(InstalledPlugin {
            name: adoption.slug,
            version: adoption.version,
            vendor: adoption.vendor,
            formats: adoption.formats,
            installed_at: Utc::now(),
            source: adoption.source,
            pinned: false,
            origin: InstallOrigin::External,
        });
        changed += 1;
    }

    if changed > 0 {
        state.save(config)?;
    }

    Ok(changed)
}

fn registry_format(format: PluginFormat) -> Option<registry::PluginFormat> {
    match format {
        PluginFormat::Au => Some(registry::PluginFormat::Au),
        PluginFormat::Vst3 => Some(registry::PluginFormat::Vst3),
    }
}
