use anyhow::Result;
use colored::Colorize;
use serde::Serialize;

use apm_core::bundle_id_store::BundleIdStore;
use apm_core::config::Config;
use apm_core::registry;
use apm_core::registry::matcher;
use apm_core::scanner::{self, PluginFormat};
use apm_core::state::InstallState;

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
    #[serde(skip_serializing_if = "Option::is_none")]
    registry_slug: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    match_method: Option<String>,
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
    let state = InstallState::load(config).unwrap_or_default();

    // Load registry and bundle ID store for matching scanned plugins.
    let reg = registry::Registry::load_all_sources(config).ok();
    let mut bid_store = BundleIdStore::open(config).ok();

    // Helper: determine whether a scanned plugin is managed by apm.
    let is_managed = |p: &scanner::ScannedPlugin| -> bool {
        state.plugins.iter().any(|sp| {
            sp.formats.iter().any(|f| f.path == p.path) || sp.name.eq_ignore_ascii_case(&p.name)
        })
    };

    // Apply --managed / --unmanaged filter.
    let plugins: Vec<_> = if managed {
        plugins.into_iter().filter(&is_managed).collect()
    } else if unmanaged {
        plugins.into_iter().filter(|p| !is_managed(p)).collect()
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
    struct Matched {
        slug: Option<String>,
        method: Option<String>,
    }
    let mut matches: Vec<Matched> = Vec::new();
    let mut learned = 0usize;

    for p in &plugins {
        let m = reg
            .as_ref()
            .and_then(|r| matcher::match_plugin(p, r, bid_store.as_ref()));

        if let Some(ref pm) = m {
            // Auto-learn: if matched by name/vendor, record the bundle ID for next time
            if pm.method != matcher::MatchMethod::BundleId {
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
        // Submit learned mappings to the server in the background.
        if let Some(ref store) = bid_store {
            let mappings = store.all_mappings().into_iter()
                .map(|(prefix, slug)| (prefix.to_string(), slug.to_string()))
                .collect::<Vec<_>>();
            tokio::spawn(submit_bundle_ids(mappings));
        }
    }

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
                managed_by_apm: is_managed(p),
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
        let source_cell = if is_managed(p) {
            "apm".green().to_string()
        } else {
            "-".dimmed().to_string()
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

    Ok(())
}

/// Submit learned bundle ID mappings to the apm server. Fire-and-forget —
/// failures are silently ignored since this is best-effort telemetry.
async fn submit_bundle_ids(mappings: Vec<(String, String)>) {
    let server_url = std::env::var("APM_SERVER_URL")
        .unwrap_or_else(|_| "https://api.apm-pm.dev".to_string());

    let reporter_hash = machine_hash();

    let body = serde_json::json!({
        "mappings": mappings.iter().map(|(prefix, slug)| {
            serde_json::json!({
                "bundle_id_prefix": prefix,
                "registry_slug": slug,
            })
        }).collect::<Vec<_>>(),
        "reporter_hash": reporter_hash,
    });

    let _ = reqwest::Client::new()
        .post(format!("{server_url}/api/bundle-ids"))
        .json(&body)
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await;
}

/// Generate a stable anonymous hash for this machine using the macOS
/// hardware UUID. Produces a consistent identifier without leaking PII.
fn machine_hash() -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();

    if let Ok(output) = std::process::Command::new("ioreg")
        .args(["-rd1", "-c", "IOPlatformExpertDevice"])
        .output()
    {
        if let Ok(stdout) = String::from_utf8(output.stdout) {
            if let Some(line) = stdout.lines().find(|l| l.contains("IOPlatformUUID")) {
                line.hash(&mut hasher);
            }
        }
    }

    format!("{:016x}", hasher.finish())
}
