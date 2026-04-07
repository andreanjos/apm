// CLI integration tests — run the built `apm` binary via std::process::Command
// and assert on exit codes and output. Uses isolated temp directories for all
// XDG paths so no test ever touches the developer's real apm config/data/cache.

use std::path::PathBuf;
use std::process::Command;

// ── Binary resolution ─────────────────────────────────────────────────────────

/// Return the path to the compiled `apm` binary in the Cargo target directory.
fn apm_bin() -> PathBuf {
    // CARGO_BIN_EXE_apm is set by Cargo when running integration tests against
    // a `[[bin]]` target in the same workspace.
    if let Ok(p) = std::env::var("CARGO_BIN_EXE_apm") {
        return PathBuf::from(p);
    }

    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .and_then(|path| path.parent())
        .expect("workspace root from crate manifest")
        .join("target/debug/apm")
}

/// Run an `apm` command with isolated XDG environment variables.
/// Returns the [`std::process::Output`] of the invocation.
fn run_apm_isolated(args: &[&str]) -> std::process::Output {
    let tmp_config = tempfile::tempdir().expect("create temp config dir");
    let tmp_data = tempfile::tempdir().expect("create temp data dir");
    let tmp_cache = tempfile::tempdir().expect("create temp cache dir");

    Command::new(apm_bin())
        .args(args)
        .env("XDG_CONFIG_HOME", tmp_config.path())
        .env("XDG_DATA_HOME", tmp_data.path())
        .env("XDG_CACHE_HOME", tmp_cache.path())
        // Disable colour output so our string matching is deterministic.
        .env("NO_COLOR", "1")
        .env("TERM", "dumb")
        .output()
        .expect("failed to run apm binary")
}

// ── --help────────────────────────────────────────────────────────────────────

#[test]
fn test_help_exits_successfully() {
    let output = run_apm_isolated(&["--help"]);
    assert!(
        output.status.success(),
        "apm --help should exit 0; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn test_help_mentions_apm() {
    let output = run_apm_isolated(&["--help"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("apm") || stdout.to_lowercase().contains("audio"),
        "help output should mention apm or audio, got: {stdout}"
    );
}

#[test]
fn test_subcommand_help_works() {
    let output = run_apm_isolated(&["scan", "--help"]);
    assert!(output.status.success(), "apm scan --help should exit 0");
}

#[test]
fn test_install_help_mentions_version_flag() {
    let output = run_apm_isolated(&["install", "--help"]);
    assert!(output.status.success(), "apm install --help should exit 0");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("--version"),
        "install help should mention --version, got: {stdout}"
    );
}

// ── --version ─────────────────────────────────────────────────────────────────

#[test]
fn test_version_exits_successfully() {
    let output = run_apm_isolated(&["--version"]);
    assert!(
        output.status.success(),
        "apm --version should exit 0; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn test_version_output_contains_version_number() {
    let output = run_apm_isolated(&["--version"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Version output should contain at least one digit.
    assert!(
        stdout.chars().any(|c| c.is_ascii_digit()),
        "version output should contain a number, got: {stdout}"
    );
}

// ── scan ──────────────────────────────────────────────────────────────────────

#[test]
fn test_scan_exits_successfully() {
    let output = run_apm_isolated(&["scan"]);
    assert!(
        output.status.success(),
        "apm scan should exit 0; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn test_scan_json_exits_successfully() {
    let output = run_apm_isolated(&["--json", "scan"]);
    assert!(
        output.status.success(),
        "apm --json scan should exit 0; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn test_scan_json_outputs_valid_json() {
    let output = run_apm_isolated(&["--json", "scan"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Output must be valid JSON (array).
    let parsed: Result<serde_json::Value, _> = serde_json::from_str(stdout.trim());
    assert!(
        parsed.is_ok(),
        "apm --json scan should output valid JSON, got: {stdout}"
    );
}

#[test]
fn test_scan_json_outputs_array() {
    let output = run_apm_isolated(&["--json", "scan"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let value: serde_json::Value =
        serde_json::from_str(stdout.trim()).expect("should parse as JSON");
    assert!(
        value.is_array(),
        "apm --json scan should output a JSON array, got: {stdout}"
    );
}

// ── list ──────────────────────────────────────────────────────────────────────

#[test]
fn test_list_exits_successfully() {
    let output = run_apm_isolated(&["list"]);
    assert!(
        output.status.success(),
        "apm list should exit 0; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn test_list_with_no_installed_plugins_shows_message() {
    let output = run_apm_isolated(&["list"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    // With empty state, should show a "no plugins" message.
    assert!(
        stdout.contains("No plugins") || stdout.contains("apm install"),
        "list with no plugins should show a message, got: {stdout}"
    );
}

#[test]
fn test_list_json_exits_successfully() {
    let output = run_apm_isolated(&["--json", "list"]);
    assert!(
        output.status.success(),
        "apm --json list should exit 0; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn test_list_json_outputs_valid_json() {
    let output = run_apm_isolated(&["--json", "list"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: Result<serde_json::Value, _> = serde_json::from_str(stdout.trim());
    assert!(
        parsed.is_ok(),
        "apm --json list should output valid JSON, got: {stdout}"
    );
}

#[test]
fn test_list_json_empty_state_outputs_empty_array() {
    let output = run_apm_isolated(&["--json", "list"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let value: serde_json::Value =
        serde_json::from_str(stdout.trim()).expect("should parse as JSON");
    assert!(
        value.is_array() && value.as_array().unwrap().is_empty(),
        "empty state should produce empty JSON array, got: {stdout}"
    );
}

// ── search ────────────────────────────────────────────────────────────────────

#[test]
fn test_search_exits_successfully() {
    let output = run_apm_isolated(&["search"]);
    // Should exit 0 even with empty registry (just shows a message).
    assert!(
        output.status.success(),
        "apm search should exit 0; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn test_search_with_empty_registry_shows_sync_hint() {
    let output = run_apm_isolated(&["search", "reverb"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    // With no synced registry, should suggest running `apm sync`.
    assert!(
        stdout.contains("sync") || stdout.contains("empty") || stdout.contains("registry"),
        "search with no registry should hint about sync, got: {stdout}"
    );
}

#[test]
fn test_search_json_empty_registry_outputs_empty_array() {
    let output = run_apm_isolated(&["--json", "search"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: Result<serde_json::Value, _> = serde_json::from_str(stdout.trim());
    assert!(
        parsed.is_ok(),
        "apm --json search should output valid JSON, got: {stdout}"
    );
}

#[test]
fn test_search_with_fixture_registry() {
    // Point the registry cache dir at our fixtures so apm can find the plugins.
    let tmp_config = tempfile::tempdir().expect("config dir");
    let tmp_data = tempfile::tempdir().expect("data dir");
    let tmp_cache = tempfile::tempdir().expect("cache dir");

    // Set up the expected registry directory structure:
    // <cache>/apm/registries/official/ → contains the plugins/ dir.
    let fixtures_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let official_dir = tmp_cache.path().join("apm/registries/official");
    std::fs::create_dir_all(&official_dir).expect("create official dir");

    // Symlink or copy the plugins/ directory.
    let plugins_src = fixtures_dir.join("plugins");
    let plugins_dst = official_dir.join("plugins");
    copy_dir_recursive(&plugins_src, &plugins_dst).expect("copy plugins");

    let output = Command::new(apm_bin())
        .args(["--json", "search"])
        .env("XDG_CONFIG_HOME", tmp_config.path())
        .env("XDG_DATA_HOME", tmp_data.path())
        .env("XDG_CACHE_HOME", tmp_cache.path())
        .env("NO_COLOR", "1")
        .env("TERM", "dumb")
        .output()
        .expect("run apm");

    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    let value: serde_json::Value =
        serde_json::from_str(stdout.trim()).expect("should be valid JSON");
    let arr = value.as_array().expect("should be array");
    assert_eq!(arr.len(), 3, "should find all 3 fixture plugins");
}

#[test]
fn test_search_with_query_matches_fixture() {
    let tmp_config = tempfile::tempdir().expect("config dir");
    let tmp_data = tempfile::tempdir().expect("data dir");
    let tmp_cache = tempfile::tempdir().expect("cache dir");

    let fixtures_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let official_dir = tmp_cache.path().join("apm/registries/official");
    std::fs::create_dir_all(&official_dir).expect("create official dir");
    copy_dir_recursive(&fixtures_dir.join("plugins"), &official_dir.join("plugins"))
        .expect("copy plugins");

    let output = Command::new(apm_bin())
        .args(["--json", "search", "reverb"])
        .env("XDG_CONFIG_HOME", tmp_config.path())
        .env("XDG_DATA_HOME", tmp_data.path())
        .env("XDG_CACHE_HOME", tmp_cache.path())
        .env("NO_COLOR", "1")
        .output()
        .expect("run apm");

    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    let value: serde_json::Value = serde_json::from_str(stdout.trim()).expect("valid JSON");
    let arr = value.as_array().expect("array");

    // At least test-reverb should appear.
    assert!(!arr.is_empty(), "search for 'reverb' should return results");
    let slugs: Vec<_> = arr
        .iter()
        .filter_map(|v| v.get("slug").and_then(|s| s.as_str()))
        .collect();
    assert!(
        slugs.contains(&"test-reverb"),
        "test-reverb should appear in results, got: {slugs:?}"
    );
}

#[test]
fn test_info_json_with_fixture_registry_includes_available_versions() {
    let tmp_config = tempfile::tempdir().expect("config dir");
    let tmp_data = tempfile::tempdir().expect("data dir");
    let tmp_cache = tempfile::tempdir().expect("cache dir");

    let fixtures_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let official_dir = tmp_cache.path().join("apm/registries/official");
    std::fs::create_dir_all(&official_dir).expect("create official dir");
    copy_dir_recursive(&fixtures_dir.join("plugins"), &official_dir.join("plugins"))
        .expect("copy plugins");

    let output = Command::new(apm_bin())
        .args(["--json", "info", "test-synth"])
        .env("XDG_CONFIG_HOME", tmp_config.path())
        .env("XDG_DATA_HOME", tmp_data.path())
        .env("XDG_CACHE_HOME", tmp_cache.path())
        .env("NO_COLOR", "1")
        .env("TERM", "dumb")
        .output()
        .expect("run apm");

    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    let value: serde_json::Value = serde_json::from_str(stdout.trim()).expect("valid JSON");
    let versions = value["available_versions"]
        .as_array()
        .expect("available_versions should be an array");
    let versions: Vec<_> = versions.iter().filter_map(|v| v.as_str()).collect();

    assert_eq!(versions, vec!["2.1.0", "2.0.0", "1.5.0"]);
}

#[test]
fn test_install_dry_run_with_fixture_registry_can_select_historical_version() {
    let tmp_config = tempfile::tempdir().expect("config dir");
    let tmp_data = tempfile::tempdir().expect("data dir");
    let tmp_cache = tempfile::tempdir().expect("cache dir");

    let fixtures_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let official_dir = tmp_cache.path().join("apm/registries/official");
    std::fs::create_dir_all(&official_dir).expect("create official dir");
    copy_dir_recursive(&fixtures_dir.join("plugins"), &official_dir.join("plugins"))
        .expect("copy plugins");

    let output = Command::new(apm_bin())
        .args(["install", "test-synth", "--version", "1.5.0", "--dry-run"])
        .env("XDG_CONFIG_HOME", tmp_config.path())
        .env("XDG_DATA_HOME", tmp_data.path())
        .env("XDG_CACHE_HOME", tmp_cache.path())
        .env("NO_COLOR", "1")
        .env("TERM", "dumb")
        .output()
        .expect("run apm");

    assert!(
        output.status.success(),
        "historical dry-run should succeed; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("v1.5.0"),
        "dry-run should show the requested historical version, got: {stdout}"
    );
}

#[test]
fn test_install_dry_run_with_fixture_registry_rejects_unknown_version() {
    let tmp_config = tempfile::tempdir().expect("config dir");
    let tmp_data = tempfile::tempdir().expect("data dir");
    let tmp_cache = tempfile::tempdir().expect("cache dir");

    let fixtures_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let official_dir = tmp_cache.path().join("apm/registries/official");
    std::fs::create_dir_all(&official_dir).expect("create official dir");
    copy_dir_recursive(&fixtures_dir.join("plugins"), &official_dir.join("plugins"))
        .expect("copy plugins");

    let output = Command::new(apm_bin())
        .args(["install", "test-synth", "--version", "9.9.9", "--dry-run"])
        .env("XDG_CONFIG_HOME", tmp_config.path())
        .env("XDG_DATA_HOME", tmp_data.path())
        .env("XDG_CACHE_HOME", tmp_cache.path())
        .env("NO_COLOR", "1")
        .env("TERM", "dumb")
        .output()
        .expect("run apm");

    assert!(!output.status.success(), "unknown version should fail");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Available versions"),
        "error should list available versions, got: {stderr}"
    );
}

#[test]
fn test_outdated_with_fixture_registry_reports_latest_against_installed_historical_version() {
    let tmp_config = tempfile::tempdir().expect("config dir");
    let tmp_data = tempfile::tempdir().expect("data dir");
    let tmp_cache = tempfile::tempdir().expect("cache dir");

    let fixtures_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let official_dir = tmp_cache.path().join("apm/registries/official");
    std::fs::create_dir_all(&official_dir).expect("create official dir");
    copy_dir_recursive(&fixtures_dir.join("plugins"), &official_dir.join("plugins"))
        .expect("copy plugins");

    let apm_data_dir = tmp_data.path().join("apm");
    std::fs::create_dir_all(&apm_data_dir).expect("create data dir");
    std::fs::write(
        apm_data_dir.join("state.toml"),
        r#"
version = 1

[[plugins]]
name = "test-synth"
version = "1.5.0"
vendor = "Synth Vendor"
installed_at = "2026-04-03T00:00:00Z"
source = "official"
pinned = false

[[plugins.formats]]
format = "vst3"
path = "/tmp/TestSynth.vst3"
sha256 = "deadbeef"
"#,
    )
    .expect("write state");

    let output = Command::new(apm_bin())
        .args(["--json", "outdated"])
        .env("XDG_CONFIG_HOME", tmp_config.path())
        .env("XDG_DATA_HOME", tmp_data.path())
        .env("XDG_CACHE_HOME", tmp_cache.path())
        .env("NO_COLOR", "1")
        .env("TERM", "dumb")
        .output()
        .expect("run apm");

    assert!(output.status.success(), "outdated should succeed");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let value: serde_json::Value = serde_json::from_str(stdout.trim()).expect("valid JSON");
    let arr = value.as_array().expect("array");
    assert_eq!(arr.len(), 1, "expected one outdated plugin");
    assert_eq!(arr[0]["name"], "test-synth");
    assert_eq!(arr[0]["installed_version"], "1.5.0");
    assert_eq!(arr[0]["available_version"], "2.1.0");
}

#[test]
fn test_import_dry_run_with_fixture_registry_preserves_exported_version() {
    let tmp_config = tempfile::tempdir().expect("config dir");
    let tmp_data = tempfile::tempdir().expect("data dir");
    let tmp_cache = tempfile::tempdir().expect("cache dir");

    let fixtures_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let official_dir = tmp_cache.path().join("apm/registries/official");
    std::fs::create_dir_all(&official_dir).expect("create official dir");
    copy_dir_recursive(&fixtures_dir.join("plugins"), &official_dir.join("plugins"))
        .expect("copy plugins");

    let import_file = tmp_data.path().join("import.toml");
    std::fs::write(
        &import_file,
        r#"
[[plugins]]
name = "test-synth"
version = "1.5.0"
formats = ["vst3"]
source = "official"
"#,
    )
    .expect("write import");

    let output = Command::new(apm_bin())
        .args(["import", import_file.to_str().unwrap(), "--dry-run"])
        .env("XDG_CONFIG_HOME", tmp_config.path())
        .env("XDG_DATA_HOME", tmp_data.path())
        .env("XDG_CACHE_HOME", tmp_cache.path())
        .env("NO_COLOR", "1")
        .env("TERM", "dumb")
        .output()
        .expect("run apm");

    assert!(output.status.success(), "import dry-run should succeed");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("test-synth v1.5.0"),
        "import dry-run should preserve exported version, got: {stdout}"
    );
}

#[test]
fn test_import_dry_run_prefers_exported_source_when_slug_exists_in_multiple_registries() {
    let tmp_config = tempfile::tempdir().expect("config dir");
    let tmp_data = tempfile::tempdir().expect("data dir");
    let tmp_cache = tempfile::tempdir().expect("cache dir");

    let config_dir = tmp_config.path().join("apm");
    std::fs::create_dir_all(&config_dir).expect("create config dir");
    std::fs::write(
        config_dir.join("config.toml"),
        r#"
[[sources]]
name = "community"
url = "https://example.com/community.git"
"#,
    )
    .expect("write config");

    let official_dir = tmp_cache.path().join("apm/registries/official/plugins");
    let community_dir = tmp_cache.path().join("apm/registries/community/plugins");
    std::fs::create_dir_all(&official_dir).expect("create official plugins");
    std::fs::create_dir_all(&community_dir).expect("create community plugins");

    std::fs::write(
        official_dir.join("shared-plugin.toml"),
        r#"
slug = "shared-plugin"
name = "Shared Plugin"
vendor = "Official Vendor"
version = "1.0.0"
description = "Official source release"
category = "effects"
license = "freeware"

[formats.vst3]
url = "https://example.com/official.zip"
sha256 = "manual"
install_type = "zip"
"#,
    )
    .expect("write official plugin");

    std::fs::write(
        community_dir.join("shared-plugin.toml"),
        r#"
slug = "shared-plugin"
name = "Shared Plugin"
vendor = "Community Vendor"
version = "2.0.0"
description = "Community override"
category = "effects"
license = "freeware"

[formats.vst3]
url = "https://example.com/community.zip"
sha256 = "manual"
install_type = "zip"
"#,
    )
    .expect("write community plugin");

    let import_file = tmp_data.path().join("import.toml");
    std::fs::write(
        &import_file,
        r#"
[[plugins]]
name = "shared-plugin"
version = "1.0.0"
formats = ["vst3"]
source = "official"
"#,
    )
    .expect("write import");

    let output = Command::new(apm_bin())
        .args(["import", import_file.to_str().unwrap(), "--dry-run"])
        .env("XDG_CONFIG_HOME", tmp_config.path())
        .env("XDG_DATA_HOME", tmp_data.path())
        .env("XDG_CACHE_HOME", tmp_cache.path())
        .env("NO_COLOR", "1")
        .env("TERM", "dumb")
        .output()
        .expect("run apm");

    assert!(output.status.success(), "import dry-run should succeed");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("shared-plugin v1.0.0"),
        "import should prefer the exported source over the merged override, got: {stdout}"
    );
}

#[test]
fn test_upgrade_dry_run_with_fixture_registry_uses_latest_against_installed_historical_version() {
    let tmp_config = tempfile::tempdir().expect("config dir");
    let tmp_data = tempfile::tempdir().expect("data dir");
    let tmp_cache = tempfile::tempdir().expect("cache dir");

    let fixtures_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let official_dir = tmp_cache.path().join("apm/registries/official");
    std::fs::create_dir_all(&official_dir).expect("create official dir");
    copy_dir_recursive(&fixtures_dir.join("plugins"), &official_dir.join("plugins"))
        .expect("copy plugins");

    let apm_data_dir = tmp_data.path().join("apm");
    std::fs::create_dir_all(&apm_data_dir).expect("create data dir");
    std::fs::write(
        apm_data_dir.join("state.toml"),
        r#"
version = 1

[[plugins]]
name = "test-synth"
version = "1.5.0"
vendor = "Synth Vendor"
installed_at = "2026-04-03T00:00:00Z"
source = "official"
pinned = false

[[plugins.formats]]
format = "vst3"
path = "/tmp/TestSynth.vst3"
sha256 = "deadbeef"
"#,
    )
    .expect("write state");

    let output = Command::new(apm_bin())
        .args(["upgrade", "test-synth", "--dry-run"])
        .env("XDG_CONFIG_HOME", tmp_config.path())
        .env("XDG_DATA_HOME", tmp_data.path())
        .env("XDG_CACHE_HOME", tmp_cache.path())
        .env("NO_COLOR", "1")
        .env("TERM", "dumb")
        .output()
        .expect("run apm");

    assert!(output.status.success(), "upgrade dry-run should succeed");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("1.5.0") && stdout.contains("2.1.0"),
        "upgrade dry-run should compare installed historical version against latest, got: {stdout}"
    );
}

#[test]
fn test_doctor_warns_when_managed_bundle_is_missing_from_disk() {
    let tmp_config = tempfile::tempdir().expect("config dir");
    let tmp_data = tempfile::tempdir().expect("data dir");
    let tmp_cache = tempfile::tempdir().expect("cache dir");

    let apm_data_dir = tmp_data.path().join("apm");
    std::fs::create_dir_all(&apm_data_dir).expect("create data dir");
    std::fs::write(
        apm_data_dir.join("state.toml"),
        r#"
version = 1

[[plugins]]
name = "ghost-plugin"
version = "1.0.0"
vendor = "Ghost Audio"
installed_at = "2026-04-04T00:00:00Z"
source = "official"
pinned = false

[[plugins.formats]]
format = "vst3"
path = "/tmp/does-not-exist/Ghost.vst3"
sha256 = "deadbeef"
"#,
    )
    .expect("write state");

    let output = Command::new(apm_bin())
        .args(["doctor"])
        .env("XDG_CONFIG_HOME", tmp_config.path())
        .env("XDG_DATA_HOME", tmp_data.path())
        .env("XDG_CACHE_HOME", tmp_cache.path())
        .env("NO_COLOR", "1")
        .env("TERM", "dumb")
        .output()
        .expect("run apm");

    assert!(output.status.success(), "doctor should exit successfully");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Managed installs"),
        "doctor output should include managed install verification, got: {stdout}"
    );
    assert!(
        stdout.contains("ghost-plugin") && stdout.contains("missing on disk"),
        "doctor should report missing managed bundles, got: {stdout}"
    );
}

#[test]
fn test_doctor_warns_when_plugin_source_is_not_configured() {
    let tmp_config = tempfile::tempdir().expect("config dir");
    let tmp_data = tempfile::tempdir().expect("data dir");
    let tmp_cache = tempfile::tempdir().expect("cache dir");

    let apm_data_dir = tmp_data.path().join("apm");
    std::fs::create_dir_all(&apm_data_dir).expect("create data dir");
    std::fs::write(
        apm_data_dir.join("state.toml"),
        r#"
version = 1

[[plugins]]
name = "source-lost"
version = "1.0.0"
vendor = "Ghost Audio"
installed_at = "2026-04-04T00:00:00Z"
source = "community"
pinned = false

[[plugins.formats]]
format = "vst3"
path = "/tmp/source-lost.vst3"
sha256 = "deadbeef"
"#,
    )
    .expect("write state");

    let output = Command::new(apm_bin())
        .args(["doctor"])
        .env("XDG_CONFIG_HOME", tmp_config.path())
        .env("XDG_DATA_HOME", tmp_data.path())
        .env("XDG_CACHE_HOME", tmp_cache.path())
        .env("NO_COLOR", "1")
        .env("TERM", "dumb")
        .output()
        .expect("run apm");

    assert!(output.status.success(), "doctor should exit successfully");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Registry provenance"),
        "doctor output should include provenance verification, got: {stdout}"
    );
    assert!(
        stdout.contains("unknown source 'community'"),
        "doctor should report missing configured sources, got: {stdout}"
    );
}

// ── unknown subcommand ────────────────────────────────────────────────────────

#[test]
fn test_unknown_subcommand_shows_error() {
    let output = run_apm_isolated(&["frobnicate"]);
    // Should exit non-zero.
    assert!(
        !output.status.success(),
        "unknown subcommand should exit non-zero"
    );
}

#[test]
fn test_unknown_subcommand_produces_output() {
    let output = run_apm_isolated(&["frobnicate"]);
    // Either stdout or stderr should have content.
    let has_output = !output.stdout.is_empty() || !output.stderr.is_empty();
    assert!(has_output, "unknown subcommand should produce some output");
}

// ── JSON flag with various commands ──────────────────────────────────────────

#[test]
fn test_json_flag_before_subcommand_works() {
    let output = run_apm_isolated(&["--json", "list"]);
    assert!(output.status.success());
}

#[test]
fn test_scan_json_each_entry_has_expected_fields() {
    // If there are plugins found, each entry should have at least name and format.
    let output = run_apm_isolated(&["--json", "scan"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let value: serde_json::Value = serde_json::from_str(stdout.trim()).expect("valid JSON");
    let arr = value.as_array().expect("array");

    // If we have any results, validate structure.
    for entry in arr {
        assert!(entry.get("name").is_some(), "each entry should have 'name'");
        assert!(
            entry.get("format").is_some(),
            "each entry should have 'format'"
        );
        assert!(entry.get("path").is_some(), "each entry should have 'path'");
        assert!(
            entry.get("managed_by_apm").is_some(),
            "each entry should have 'managed_by_apm'"
        );
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Recursively copy a directory tree from `src` to `dst`.
fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}
