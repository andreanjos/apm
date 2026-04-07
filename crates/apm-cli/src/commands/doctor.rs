// doctor command — run diagnostic checks and report apm health.

use std::path::Path;

use anyhow::Result;

use apm_core::config::{self, Config};
use apm_core::registry::Registry;
use apm_core::state::InstallState;

use crate::license_cache::LicenseCache;

// ── Check result ──────────────────────────────────────────────────────────────

#[derive(Debug)]
enum CheckStatus {
    Ok(String),
    Warn(String),
    Fail(String),
}

struct Check {
    label: String,
    status: CheckStatus,
    hint: Option<String>,
}

impl Check {
    fn ok(label: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            status: CheckStatus::Ok(detail.into()),
            hint: None,
        }
    }

    fn warn(label: impl Into<String>, detail: impl Into<String>, hint: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            status: CheckStatus::Warn(detail.into()),
            hint: Some(hint.into()),
        }
    }

    fn fail(label: impl Into<String>, detail: impl Into<String>, hint: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            status: CheckStatus::Fail(detail.into()),
            hint: Some(hint.into()),
        }
    }
}

// ── Public entry point ────────────────────────────────────────────────────────

pub fn run(config: &Config) -> Result<()> {
    println!("apm doctor");
    println!("{}", "\u{2550}".repeat(35));
    println!();

    let mut all_checks: Vec<Check> = Vec::new();
    let mut failures = 0usize;
    let mut warnings = 0usize;

    // ── Plugin directories ────────────────────────────────────────────────────

    println!("Checking plugin directories...");

    let dirs = vec![
        (
            config::user_au_dir(),
            "~/Library/Audio/Plug-Ins/Components/",
            true,
        ),
        (
            config::user_vst3_dir(),
            "~/Library/Audio/Plug-Ins/VST3/",
            true,
        ),
        (
            config::system_au_dir(),
            "/Library/Audio/Plug-Ins/Components/",
            false,
        ),
        (
            config::system_vst3_dir(),
            "/Library/Audio/Plug-Ins/VST3/",
            false,
        ),
    ];

    for (path, label, check_writable) in dirs {
        let check = check_plugin_dir(&path, label, check_writable);
        print_check(&check);
        match &check.status {
            CheckStatus::Fail(_) => failures += 1,
            CheckStatus::Warn(_) => warnings += 1,
            CheckStatus::Ok(_) => {}
        }
        all_checks.push(check);
    }

    println!();

    // ── Quarantine checks ─────────────────────────────────────────────────────

    println!("Checking for quarantined plugins...");
    let quarantine_check = check_quarantine(config);
    print_check(&quarantine_check);
    match &quarantine_check.status {
        CheckStatus::Fail(_) => failures += 1,
        CheckStatus::Warn(_) => warnings += 1,
        CheckStatus::Ok(_) => {}
    }
    all_checks.push(quarantine_check);
    println!();

    // ── Configuration ─────────────────────────────────────────────────────────

    println!("Checking configuration...");

    let config_check = check_config_file();
    print_check(&config_check);
    match &config_check.status {
        CheckStatus::Fail(_) => failures += 1,
        CheckStatus::Warn(_) => warnings += 1,
        CheckStatus::Ok(_) => {}
    }
    all_checks.push(config_check);

    let state_check = check_state_file(config);
    print_check(&state_check);
    match &state_check.status {
        CheckStatus::Fail(_) => failures += 1,
        CheckStatus::Warn(_) => warnings += 1,
        CheckStatus::Ok(_) => {}
    }
    all_checks.push(state_check);

    let managed_install_check = check_managed_installs(config);
    print_check(&managed_install_check);
    match &managed_install_check.status {
        CheckStatus::Fail(_) => failures += 1,
        CheckStatus::Warn(_) => warnings += 1,
        CheckStatus::Ok(_) => {}
    }
    all_checks.push(managed_install_check);

    let provenance_check = check_registry_provenance(config);
    print_check(&provenance_check);
    match &provenance_check.status {
        CheckStatus::Fail(_) => failures += 1,
        CheckStatus::Warn(_) => warnings += 1,
        CheckStatus::Ok(_) => {}
    }
    all_checks.push(provenance_check);

    let license_check = check_paid_license_cache(config);
    print_check(&license_check);
    match &license_check.status {
        CheckStatus::Fail(_) => failures += 1,
        CheckStatus::Warn(_) => warnings += 1,
        CheckStatus::Ok(_) => {}
    }
    all_checks.push(license_check);

    let registry_check = check_registry_cache(config);
    print_check(&registry_check);
    match &registry_check.status {
        CheckStatus::Fail(_) => failures += 1,
        CheckStatus::Warn(_) => warnings += 1,
        CheckStatus::Ok(_) => {}
    }
    all_checks.push(registry_check);

    println!();

    // ── Hints for failed/warned checks ────────────────────────────────────────

    let problem_checks: Vec<&Check> = all_checks
        .iter()
        .filter(|c| matches!(c.status, CheckStatus::Fail(_) | CheckStatus::Warn(_)))
        .collect();

    if !problem_checks.is_empty() {
        println!("Remediation hints:");
        for check in &problem_checks {
            if let Some(hint) = &check.hint {
                println!("  {}: {}", check.label, hint);
            }
        }
        println!();
    }

    // ── Summary ───────────────────────────────────────────────────────────────

    if failures == 0 && warnings == 0 {
        println!("Summary: All checks passed. apm is ready to use.");
    } else if failures == 0 {
        println!(
            "Summary: {} warning(s) found. apm should work, but review the hints above.",
            warnings
        );
    } else {
        println!(
            "Summary: {} failure(s), {} warning(s) found. See hints above to resolve issues.",
            failures, warnings
        );
    }

    Ok(())
}

// ── Individual checks ─────────────────────────────────────────────────────────

fn check_plugin_dir(path: &Path, label: &str, check_writable: bool) -> Check {
    if !path.exists() {
        if check_writable {
            // User dirs: missing is fine, we can create them.
            return Check::warn(
                label,
                "directory does not exist",
                format!("Create it with: mkdir -p \"{}\"", path.display()),
            );
        } else {
            // System dirs: missing means no system plugins, not necessarily an error.
            return Check::warn(
                label,
                "directory does not exist (no system plugins installed)",
                "System plugin directory is absent — this is normal if no system-wide plugins are installed.",
            );
        }
    }

    let readable = std::fs::read_dir(path).is_ok();
    if !readable {
        return Check::fail(
            label,
            "not readable",
            format!(
                "Check permissions with: ls -la \"{}\"",
                path.parent().unwrap_or(path).display()
            ),
        );
    }

    if check_writable {
        // Test writability by checking the metadata permissions.
        let writable = is_writable(path);
        if writable {
            Check::ok(label, "readable, writable")
        } else {
            Check::warn(
                label,
                "readable but not writable",
                format!("Fix permissions with: chmod u+w \"{}\"", path.display()),
            )
        }
    } else {
        Check::ok(label, "readable")
    }
}

fn check_quarantine(_config: &Config) -> Check {
    // Check user plugin directories for quarantined bundles.
    let dirs = [config::user_au_dir(), config::user_vst3_dir()];
    let mut quarantined: Vec<String> = Vec::new();

    for dir in &dirs {
        if !dir.exists() {
            continue;
        }
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_dir() {
                    continue;
                }
                let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
                if ext != "component" && ext != "vst3" {
                    continue;
                }

                // Run `xattr -l <bundle>` and check for com.apple.quarantine.
                if let Ok(output) = std::process::Command::new("xattr")
                    .arg("-l")
                    .arg(&path)
                    .output()
                {
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    if stdout.contains("com.apple.quarantine") {
                        let name = path
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("unknown")
                            .to_string();
                        quarantined.push(name);
                    }
                }
            }
        }
    }

    if quarantined.is_empty() {
        Check::ok("Quarantine", "no quarantined plugins found")
    } else {
        let names = quarantined.join(", ");
        Check::warn(
            "Quarantine",
            format!("{} quarantined plugin(s): {}", quarantined.len(), names),
            "Remove quarantine with: xattr -r -d com.apple.quarantine <bundle-path>\n    \
             Or reinstall affected plugins with apm to have quarantine removed automatically.",
        )
    }
}

fn check_config_file() -> Check {
    let cfg_dir = config::config_dir();
    let cfg_path = cfg_dir.join("config.toml");

    if !cfg_path.exists() {
        // Missing is OK — apm creates it on next run.
        return Check::ok(
            "Config file",
            format!("{} (will be created on next run)", display_path(&cfg_path)),
        );
    }

    // Try to load and parse.
    match config::load_config(&cfg_path) {
        Ok(_) => Check::ok(
            "Config file",
            format!("\u{2713} {}", display_path(&cfg_path)),
        ),
        Err(e) => Check::fail(
            "Config file",
            format!("invalid TOML: {e}"),
            format!(
                "Edit or delete {} to fix. apm will recreate it with defaults if deleted.",
                display_path(&cfg_path)
            ),
        ),
    }
}

fn check_state_file(config: &Config) -> Check {
    let state_path = config.state_file();

    if !state_path.exists() {
        return Check::ok(
            "State file",
            format!("{} (no plugins installed yet)", display_path(&state_path)),
        );
    }

    match InstallState::load_from(&state_path) {
        Ok(state) => Check::ok(
            "State file",
            format!(
                "{} ({} plugin{} managed)",
                display_path(&state_path),
                state.plugins.len(),
                if state.plugins.len() == 1 { "" } else { "s" }
            ),
        ),
        Err(e) => Check::fail(
            "State file",
            format!("invalid: {e}"),
            format!(
                "Back up and delete {} to reset install state, then reinstall plugins.",
                display_path(&state_path)
            ),
        ),
    }
}

fn check_managed_installs(config: &Config) -> Check {
    let state = match InstallState::load(config) {
        Ok(state) => state,
        Err(error) => {
            return Check::fail(
                "Managed installs",
                format!("could not load install state: {error}"),
                "Fix the state file first, then rerun `apm doctor`.",
            )
        }
    };

    if state.plugins.is_empty() {
        return Check::ok("Managed installs", "no managed plugins to verify");
    }

    let mut missing = Vec::new();
    for plugin in &state.plugins {
        for format in &plugin.formats {
            if !format.path.exists() {
                missing.push(format!(
                    "{} {} ({}) at {}",
                    plugin.name,
                    plugin.version,
                    format.format,
                    display_path(&format.path)
                ));
            }
        }
    }

    if missing.is_empty() {
        return Check::ok(
            "Managed installs",
            format!(
                "verified {} managed plugin{} on disk",
                state.plugins.len(),
                if state.plugins.len() == 1 { "" } else { "s" }
            ),
        );
    }

    let preview = missing
        .iter()
        .take(3)
        .cloned()
        .collect::<Vec<_>>()
        .join(", ");
    let suffix = if missing.len() > 3 {
        format!(" (+{} more)", missing.len() - 3)
    } else {
        String::new()
    };

    Check::warn(
        "Managed installs",
        format!(
            "{} tracked bundle(s) missing on disk: {}{}",
            missing.len(),
            preview,
            suffix
        ),
        "Run `apm remove <plugin>` to clean stale state entries, or reinstall the missing bundles.",
    )
}

fn check_registry_provenance(config: &Config) -> Check {
    let state = match InstallState::load(config) {
        Ok(state) => state,
        Err(error) => {
            return Check::fail(
                "Registry provenance",
                format!("could not load install state: {error}"),
                "Fix the state file first, then rerun `apm doctor`.",
            )
        }
    };

    if state.plugins.is_empty() {
        return Check::ok("Registry provenance", "no managed plugins to verify");
    }

    let registry = match Registry::load_all_sources(config) {
        Ok(registry) => registry,
        Err(error) => {
            return Check::warn(
                "Registry provenance",
                format!("registry unavailable: {error}"),
                "Run `apm sync` so doctor can verify install provenance against the local registry cache.",
            )
        }
    };

    let known_sources: std::collections::HashSet<String> = config
        .sources()
        .into_iter()
        .map(|source| source.name)
        .collect();

    let mut issues = Vec::new();
    for plugin in &state.plugins {
        if !known_sources.contains(&plugin.source) {
            issues.push(format!(
                "{} (unknown source '{}')",
                plugin.name, plugin.source
            ));
            continue;
        }

        if registry
            .find_in_source(&plugin.source, &plugin.name)
            .is_none()
        {
            issues.push(format!(
                "{} (missing from source '{}')",
                plugin.name, plugin.source
            ));
        }
    }

    if issues.is_empty() {
        return Check::ok(
            "Registry provenance",
            "all managed plugins map to configured sources",
        );
    }

    let preview = issues
        .iter()
        .take(3)
        .cloned()
        .collect::<Vec<_>>()
        .join(", ");
    let suffix = if issues.len() > 3 {
        format!(" (+{} more)", issues.len() - 3)
    } else {
        String::new()
    };

    Check::warn(
        "Registry provenance",
        format!("{} provenance issue(s): {}{}", issues.len(), preview, suffix),
        "Re-add the missing registry source, run `apm sync`, or reinstall plugins from an available source.",
    )
}

fn check_paid_license_cache(config: &Config) -> Check {
    let state = match InstallState::load(config) {
        Ok(state) => state,
        Err(error) => {
            return Check::fail(
                "Paid license cache",
                format!("could not load install state: {error}"),
                "Fix the state file first, then rerun `apm doctor`.",
            )
        }
    };

    if state.plugins.is_empty() {
        return Check::ok("Paid license cache", "no managed plugins to verify");
    }

    let registry = match Registry::load_all_sources(config) {
        Ok(registry) => registry,
        Err(_) => {
            return Check::ok(
                "Paid license cache",
                "registry unavailable; skipping paid-license verification",
            )
        }
    };

    let paid_plugins = state
        .plugins
        .iter()
        .filter(|plugin| {
            registry
                .find_in_source(&plugin.source, &plugin.name)
                .or_else(|| registry.find(&plugin.name))
                .map(|entry| entry.is_paid)
                .unwrap_or(false)
        })
        .collect::<Vec<_>>();

    if paid_plugins.is_empty() {
        return Check::ok(
            "Paid license cache",
            "no installed paid plugins require cached licenses",
        );
    }

    if !config.license_cache_db_path().exists() {
        return Check::warn(
            "Paid license cache",
            format!(
                "{} paid plugin{} installed but no local license cache exists",
                paid_plugins.len(),
                if paid_plugins.len() == 1 { "" } else { "s" }
            ),
            "Run `apm licenses` or `apm restore` after authenticating to repopulate the local license cache.",
        );
    }

    let cache = match LicenseCache::open(config) {
        Ok(cache) => cache,
        Err(error) => {
            return Check::fail(
                "Paid license cache",
                format!("could not open license cache: {error}"),
                "Repair or delete the local license cache, then resync licenses.",
            )
        }
    };

    let mut issues = Vec::new();
    for plugin in paid_plugins {
        match cache.load_license(&plugin.name) {
            Ok(Some(license)) if license.status == "active" => {}
            Ok(Some(license)) => issues.push(format!(
                "{} (license status: {})",
                plugin.name, license.status
            )),
            Ok(None) => issues.push(format!("{} (no cached license)", plugin.name)),
            Err(error) => issues.push(format!("{} ({error})", plugin.name)),
        }
    }

    if issues.is_empty() {
        return Check::ok(
            "Paid license cache",
            "all installed paid plugins have active cached licenses",
        );
    }

    let preview = issues
        .iter()
        .take(3)
        .cloned()
        .collect::<Vec<_>>()
        .join(", ");
    let suffix = if issues.len() > 3 {
        format!(" (+{} more)", issues.len() - 3)
    } else {
        String::new()
    };

    Check::warn(
        "Paid license cache",
        format!("{} paid-license issue(s): {}{}", issues.len(), preview, suffix),
        "Run `apm licenses` or `apm restore` to refresh license state, or reinstall affected paid plugins.",
    )
}

fn check_registry_cache(config: &Config) -> Check {
    match Registry::load_all_sources(config) {
        Ok(registry) if registry.is_empty() => Check::warn(
            "Registry cache",
            "empty — no plugins available",
            "Run `apm sync` to download the plugin registry.",
        ),
        Ok(registry) => Check::ok(
            "Registry cache",
            format!(
                "{} plugin{} cached",
                registry.len(),
                if registry.len() == 1 { "" } else { "s" }
            ),
        ),
        Err(e) => Check::fail(
            "Registry cache",
            format!("could not load: {e}"),
            "Run `apm sync` to rebuild the registry cache.",
        ),
    }
}

// ── Display helpers ───────────────────────────────────────────────────────────

fn print_check(check: &Check) {
    let (symbol, detail) = match &check.status {
        CheckStatus::Ok(d) => ("\u{2713}", d.as_str()),
        CheckStatus::Warn(d) => ("!", d.as_str()),
        CheckStatus::Fail(d) => ("\u{2717}", d.as_str()),
    };
    println!("  {:<45}  {} {}", check.label, symbol, detail);
}

fn display_path(path: &Path) -> String {
    let path_str = path.to_string_lossy();
    if let Some(home) = dirs::home_dir() {
        let home_str = home.to_string_lossy();
        if let Some(rest) = path_str.strip_prefix(home_str.as_ref()) {
            return format!("~{rest}");
        }
    }
    path_str.into_owned()
}

fn is_writable(path: &Path) -> bool {
    // Use std::fs::metadata to check permissions.
    use std::os::unix::fs::PermissionsExt;
    path.metadata()
        .map(|m| m.permissions().mode() & 0o200 != 0)
        .unwrap_or(false)
}
