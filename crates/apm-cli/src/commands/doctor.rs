// doctor command — run diagnostic checks and report apm health.

use std::path::Path;

use anyhow::Result;

use crate::config::{self, Config};
use crate::registry::Registry;
use crate::state::InstallState;

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
        (config::user_au_dir(), "~/Library/Audio/Plug-Ins/Components/", true),
        (config::user_vst3_dir(), "~/Library/Audio/Plug-Ins/VST3/", true),
        (config::system_au_dir(), "/Library/Audio/Plug-Ins/Components/", false),
        (config::system_vst3_dir(), "/Library/Audio/Plug-Ins/VST3/", false),
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
                format!(
                    "Create it with: mkdir -p \"{}\"",
                    path.display()
                ),
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
                format!(
                    "Fix permissions with: chmod u+w \"{}\"",
                    path.display()
                ),
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
        // Sample the first few bundles in each directory.
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.take(5).flatten() {
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
        Ok(_) => Check::ok("Config file", format!("\u{2713} {}", display_path(&cfg_path))),
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

fn check_registry_cache(config: &Config) -> Check {
    match Registry::load_all_sources(config) {
        Ok(registry) if registry.is_empty() => Check::warn(
            "Registry cache",
            "empty — no plugins available",
            "Run `apm sync` to download the plugin registry.",
        ),
        Ok(registry) => Check::ok(
            "Registry cache",
            format!("{} plugin{} cached", registry.len(), if registry.len() == 1 { "" } else { "s" }),
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
