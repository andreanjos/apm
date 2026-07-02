use std::path::Path;
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

use crate::bundle_id_store::BundleIdStore;
use crate::config::{self, Config};
use crate::registry::{matcher, Registry};
use crate::scanner;
use crate::state::InstallState;

mod model_store;
mod privileged;
use model_store::check_model_store;
use privileged::check_privileged_helper_artifacts;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticStatus {
    Ok,
    Warning,
    Failure,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagnosticCheck {
    pub name: String,
    pub status: DiagnosticStatus,
    pub detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagnosticsSummary {
    pub ok: usize,
    pub warnings: usize,
    pub failures: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagnosticsReport {
    pub checks: Vec<DiagnosticCheck>,
    pub summary: DiagnosticsSummary,
}

pub fn run_diagnostics(config: &Config) -> DiagnosticsReport {
    let mut report = DiagnosticsReport {
        checks: Vec::new(),
        summary: DiagnosticsSummary::default(),
    };

    for check in plugin_directory_checks() {
        report.push(check);
    }
    report.push(check_quarantine());
    report.push(check_config_file());
    report.push(check_state_file(config));
    report.push(check_managed_installs(config));
    report.push(check_registry_provenance(config));
    report.push(check_registry_cache(config));
    report.push(check_model_store());
    report.push(check_vendor_installers(config));
    report.push(check_privileged_helper_artifacts());
    report.push(check_registry_freshness(config));
    report.push(check_orphaned_state_entries(config));

    report
}

impl DiagnosticsReport {
    fn push(&mut self, check: DiagnosticCheck) {
        match check.status {
            DiagnosticStatus::Ok => self.summary.ok += 1,
            DiagnosticStatus::Warning => self.summary.warnings += 1,
            DiagnosticStatus::Failure => self.summary.failures += 1,
        }
        self.checks.push(check);
    }
}

impl DiagnosticCheck {
    fn ok(name: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            status: DiagnosticStatus::Ok,
            detail: detail.into(),
            hint: None,
        }
    }

    fn warning(
        name: impl Into<String>,
        detail: impl Into<String>,
        hint: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            status: DiagnosticStatus::Warning,
            detail: detail.into(),
            hint: Some(hint.into()),
        }
    }

    fn failure(
        name: impl Into<String>,
        detail: impl Into<String>,
        hint: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            status: DiagnosticStatus::Failure,
            detail: detail.into(),
            hint: Some(hint.into()),
        }
    }
}

fn plugin_directory_checks() -> Vec<DiagnosticCheck> {
    [
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
    ]
    .into_iter()
    .map(|(path, label, check_writable)| check_plugin_dir(&path, label, check_writable))
    .collect()
}

fn check_plugin_dir(path: &Path, label: &str, check_writable: bool) -> DiagnosticCheck {
    if !path.exists() {
        if check_writable {
            return DiagnosticCheck::warning(
                label,
                "directory does not exist",
                format!("Create it with: mkdir -p \"{}\"", path.display()),
            );
        }

        return DiagnosticCheck::warning(
            label,
            "directory does not exist (no system plugins installed)",
            "System plugin directory is absent. This is normal if no system-wide plugins are installed.",
        );
    }

    if std::fs::read_dir(path).is_err() {
        return DiagnosticCheck::failure(
            label,
            "not readable",
            format!(
                "Check permissions with: ls -la \"{}\"",
                path.parent().unwrap_or(path).display()
            ),
        );
    }

    if !check_writable {
        return DiagnosticCheck::ok(label, "readable");
    }

    if is_writable(path) {
        DiagnosticCheck::ok(label, "readable, writable")
    } else {
        DiagnosticCheck::warning(
            label,
            "readable but not writable",
            format!("Fix permissions with: chmod u+w \"{}\"", path.display()),
        )
    }
}

fn check_quarantine() -> DiagnosticCheck {
    let dirs = [config::user_au_dir(), config::user_vst3_dir()];
    let mut quarantined = Vec::new();

    for dir in dirs {
        if !dir.exists() {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let ext = path.extension().and_then(|ext| ext.to_str()).unwrap_or("");
            if ext != "component" && ext != "vst3" {
                continue;
            }

            if let Ok(output) = std::process::Command::new("xattr")
                .arg("-l")
                .arg(&path)
                .output()
            {
                let stdout = String::from_utf8_lossy(&output.stdout);
                if stdout.contains("com.apple.quarantine") {
                    let name = path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or("unknown")
                        .to_string();
                    quarantined.push(name);
                }
            }
        }
    }

    if quarantined.is_empty() {
        DiagnosticCheck::ok("Quarantine", "no quarantined plugins found")
    } else {
        DiagnosticCheck::warning(
            "Quarantine",
            format!(
                "{} quarantined plugin(s): {}",
                quarantined.len(),
                quarantined.join(", ")
            ),
            "Remove quarantine with: xattr -r -d com.apple.quarantine <bundle-path>",
        )
    }
}

fn check_config_file() -> DiagnosticCheck {
    let config_path = config::config_dir().join("config.toml");

    if !config_path.exists() {
        return DiagnosticCheck::ok(
            "Config file",
            format!(
                "{} (will be created on next run)",
                display_path(&config_path)
            ),
        );
    }

    match config::load_config(&config_path) {
        Ok(_) => DiagnosticCheck::ok("Config file", display_path(&config_path)),
        Err(error) => DiagnosticCheck::failure(
            "Config file",
            format!("invalid TOML: {error}"),
            format!(
                "Edit or delete {} to fix. apm will recreate it with defaults if deleted.",
                display_path(&config_path)
            ),
        ),
    }
}

fn check_state_file(config: &Config) -> DiagnosticCheck {
    let state_path = config.state_file();

    if !state_path.exists() {
        return DiagnosticCheck::ok(
            "State file",
            format!("{} (no plugins installed yet)", display_path(&state_path)),
        );
    }

    match InstallState::load_from(&state_path) {
        Ok(state) => DiagnosticCheck::ok(
            "State file",
            format!(
                "{} ({} plugin{} managed)",
                display_path(&state_path),
                state.plugins.len(),
                if state.plugins.len() == 1 { "" } else { "s" }
            ),
        ),
        Err(error) => DiagnosticCheck::failure(
            "State file",
            format!("invalid: {error}"),
            format!(
                "Back up and delete {} to reset install state, then reinstall plugins.",
                display_path(&state_path)
            ),
        ),
    }
}

fn check_managed_installs(config: &Config) -> DiagnosticCheck {
    let state = match InstallState::load(config) {
        Ok(state) => state,
        Err(error) => {
            return DiagnosticCheck::failure(
                "Managed installs",
                format!("could not load install state: {error}"),
                "Fix the state file first, then rerun `apm doctor`.",
            )
        }
    };

    if state.plugins.is_empty() {
        return DiagnosticCheck::ok("Managed installs", "no managed plugins to verify");
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
        return DiagnosticCheck::ok(
            "Managed installs",
            format!(
                "verified {} managed plugin{} on disk",
                state.plugins.len(),
                if state.plugins.len() == 1 { "" } else { "s" }
            ),
        );
    }

    DiagnosticCheck::warning(
        "Managed installs",
        format!(
            "{} tracked bundle(s) missing on disk: {}",
            missing.len(),
            preview_list(&missing, 3)
        ),
        "Run `apm remove <plugin>` to clean stale state entries, or reinstall the missing bundles.",
    )
}

fn check_registry_provenance(config: &Config) -> DiagnosticCheck {
    let state = match InstallState::load(config) {
        Ok(state) => state,
        Err(error) => {
            return DiagnosticCheck::failure(
                "Registry provenance",
                format!("could not load install state: {error}"),
                "Fix the state file first, then rerun `apm doctor`.",
            )
        }
    };

    if state.plugins.is_empty() {
        return DiagnosticCheck::ok("Registry provenance", "no managed plugins to verify");
    }

    let registry = match Registry::load_all_sources(config) {
        Ok(registry) => registry,
        Err(error) => {
            return DiagnosticCheck::warning(
                "Registry provenance",
                format!("registry unavailable: {error}"),
                "Run `apm sync` so doctor can verify install provenance against the local registry cache.",
            )
        }
    };

    let known_sources = config
        .sources()
        .into_iter()
        .map(|source| source.name)
        .collect::<std::collections::HashSet<_>>();

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
        return DiagnosticCheck::ok(
            "Registry provenance",
            "all managed plugins map to configured sources",
        );
    }

    DiagnosticCheck::warning(
        "Registry provenance",
        format!("{} provenance issue(s): {}", issues.len(), preview_list(&issues, 3)),
        "Re-add the missing registry source, run `apm sync`, or reinstall plugins from an available source.",
    )
}

fn check_registry_cache(config: &Config) -> DiagnosticCheck {
    match Registry::load_all_sources(config) {
        Ok(registry) if registry.is_empty() => DiagnosticCheck::warning(
            "Registry cache",
            "empty - no plugins available",
            "Run `apm sync` to download the plugin registry.",
        ),
        Ok(registry) => DiagnosticCheck::ok(
            "Registry cache",
            format!(
                "{} plugin{} cached",
                registry.len(),
                if registry.len() == 1 { "" } else { "s" }
            ),
        ),
        Err(error) => DiagnosticCheck::failure(
            "Registry cache",
            format!("could not load: {error}"),
            "Run `apm sync` to rebuild the registry cache.",
        ),
    }
}

fn check_vendor_installers(config: &Config) -> DiagnosticCheck {
    let registry =
        match Registry::load_all_sources(config) {
            Ok(registry) if registry.is_empty() => return DiagnosticCheck::warning(
                "Vendor installers",
                "registry unavailable for installer matching",
                "Run `apm sync` so doctor can detect vendor manager apps for installed plugins.",
            ),
            Ok(registry) => registry,
            Err(error) => return DiagnosticCheck::warning(
                "Vendor installers",
                format!("registry unavailable: {error}"),
                "Run `apm sync` so doctor can detect vendor manager apps for installed plugins.",
            ),
        };

    let scanned = scanner::scan_plugins(config);
    if scanned.is_empty() {
        return DiagnosticCheck::ok(
            "Vendor installers",
            "no installed plugins found to evaluate",
        );
    }

    let bundle_store = BundleIdStore::open(config).ok();
    let mut relevant_keys = std::collections::BTreeSet::new();

    for plugin in &scanned {
        if let Some(matched) = matcher::match_plugin(plugin, &registry, bundle_store.as_ref()) {
            if let Some(key) = matched.registry_plugin.installer.as_deref() {
                relevant_keys.insert(key.to_string());
            }
        }
    }

    if relevant_keys.is_empty() {
        return DiagnosticCheck::ok(
            "Vendor installers",
            "no vendor-managed plugins detected in the current library",
        );
    }

    let mut installed = Vec::new();
    let mut missing = Vec::new();
    let mut unknown = Vec::new();

    for key in relevant_keys {
        let Some(installer) = registry.find_installer(&key) else {
            unknown.push(key);
            continue;
        };

        if installer.app_paths.iter().any(|path| path.exists()) {
            installed.push(installer.name.clone());
        } else {
            missing.push(installer.name.clone());
        }
    }

    let mut detail = Vec::new();
    if !installed.is_empty() {
        detail.push(format!("installed: {}", installed.join(", ")));
    }
    if !missing.is_empty() {
        detail.push(format!("missing: {}", missing.join(", ")));
    }
    if !unknown.is_empty() {
        detail.push(format!("unknown registry keys: {}", unknown.join(", ")));
    }

    if missing.is_empty() && unknown.is_empty() {
        DiagnosticCheck::ok("Vendor installers", detail.join(" | "))
    } else {
        DiagnosticCheck::warning(
            "Vendor installers",
            detail.join(" | "),
            "Install the missing vendor manager apps, then rerun `apm doctor` or `apm install <plugin>`.",
        )
    }
}

fn check_registry_freshness(config: &Config) -> DiagnosticCheck {
    let official_dir = config.registries_cache_dir().join("official");

    if !official_dir.exists() {
        return DiagnosticCheck::warning(
            "Registry freshness",
            "registry cache does not exist",
            "Run `apm sync` to download the plugin registry.",
        );
    }

    let plugins_dir = official_dir.join("plugins");
    let probe = if plugins_dir.exists() {
        plugins_dir
    } else {
        official_dir
    };

    let modified = match std::fs::metadata(&probe).and_then(|metadata| metadata.modified()) {
        Ok(modified) => modified,
        Err(_) => {
            return DiagnosticCheck::warning(
                "Registry freshness",
                "could not determine registry cache age",
                "Run `apm sync` to refresh the registry.",
            )
        }
    };

    let age = SystemTime::now()
        .duration_since(modified)
        .unwrap_or_default();
    let age_days = age.as_secs() / 86400;

    if age_days > 30 {
        DiagnosticCheck::warning(
            "Registry freshness",
            format!("registry cache is {age_days} days old"),
            "Run `apm sync` to update.",
        )
    } else {
        DiagnosticCheck::ok(
            "Registry freshness",
            format!(
                "synced {} day{} ago",
                age_days,
                if age_days == 1 { "" } else { "s" }
            ),
        )
    }
}

fn check_orphaned_state_entries(config: &Config) -> DiagnosticCheck {
    let state = match InstallState::load(config) {
        Ok(state) => state,
        Err(error) => {
            return DiagnosticCheck::failure(
                "Orphaned state entries",
                format!("could not load install state: {error}"),
                "Fix the state file first, then rerun `apm doctor`.",
            )
        }
    };

    if state.plugins.is_empty() {
        return DiagnosticCheck::ok("Orphaned state entries", "no managed plugins to verify");
    }

    let mut missing_lines = Vec::new();
    for plugin in &state.plugins {
        for format in plugin.formats.iter().filter(|format| !format.path.exists()) {
            missing_lines.push(format!(
                "{}: {} missing",
                plugin.name,
                display_path(&format.path)
            ));
        }
    }

    if missing_lines.is_empty() {
        return DiagnosticCheck::ok(
            "Orphaned state entries",
            "all installed plugins have bundles on disk",
        );
    }

    DiagnosticCheck::warning(
        "Orphaned state entries",
        format!(
            "{} missing bundle{}: {}",
            missing_lines.len(),
            if missing_lines.len() == 1 { "" } else { "s" },
            preview_list(&missing_lines, 5)
        ),
        "Run `apm remove <plugin>` to clean stale state entries, or reinstall with `apm install <plugin>`.",
    )
}

fn preview_list(values: &[String], limit: usize) -> String {
    let preview = values
        .iter()
        .take(limit)
        .cloned()
        .collect::<Vec<_>>()
        .join(", ");
    if values.len() > limit {
        format!("{preview} (+{} more)", values.len() - limit)
    } else {
        preview
    }
}

fn is_writable(path: &Path) -> bool {
    path.metadata()
        .map(|metadata| !metadata.permissions().readonly())
        .unwrap_or(false)
}

fn display_path(path: &Path) -> String {
    let path_string = path.display().to_string();
    if let Ok(home) = std::env::var("HOME") {
        let home = home.trim_end_matches('/');
        if !home.is_empty() && path_string == home {
            return "~".to_string();
        }
        if !home.is_empty() && path_string.starts_with(&format!("{home}/")) {
            return format!("~{}", &path_string[home.len()..]);
        }
    }
    path_string
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use tempfile::TempDir;

    use crate::registry::PluginFormat;
    use crate::state::{InstalledFormat, InstalledPlugin};

    #[test]
    fn diagnostics_report_counts_state_file_failure() {
        let env = TestEnv::new();
        std::fs::create_dir_all(env.config.resolved_data_dir()).expect("create data dir");
        std::fs::write(env.config.state_file(), "not toml").expect("write bad state");

        let report = run_diagnostics(&env.config);

        assert!(report.summary.failures >= 1);
        assert!(report
            .checks
            .iter()
            .any(|check| check.name == "State file" && check.status == DiagnosticStatus::Failure));
    }

    #[test]
    fn diagnostics_report_warns_for_missing_tracked_bundle() {
        let env = TestEnv::new();
        let mut state = InstallState::default();
        state.plugins.push(InstalledPlugin {
            name: "ghost-plugin".to_string(),
            version: "1.0.0".to_string(),
            vendor: "Ghost Audio".to_string(),
            formats: vec![InstalledFormat {
                format: PluginFormat::Vst3,
                path: env.root.path().join("missing/Ghost.vst3"),
                sha256: "deadbeef".to_string(),
            }],
            installed_at: Utc::now(),
            source: "official".to_string(),
            pinned: false,
            origin: crate::state::InstallOrigin::Apm,
        });
        state.save(&env.config).expect("save state");

        let report = run_diagnostics(&env.config);

        assert!(report.checks.iter().any(|check| {
            check.name == "Managed installs"
                && check.status == DiagnosticStatus::Warning
                && check.detail.contains("ghost-plugin")
                && check.detail.contains("missing on disk")
        }));
    }

    struct TestEnv {
        root: TempDir,
        config: Config,
    }

    impl TestEnv {
        fn new() -> Self {
            let root = tempfile::tempdir().expect("temp dir");
            let config = Config {
                data_dir: Some(root.path().join("data")),
                cache_dir: Some(root.path().join("cache")),
                ..Config::default()
            };
            Self { root, config }
        }
    }
}
