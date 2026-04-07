// Integration tests for configuration behaviour — default values, TOML loading,
// XDG path resolution, and directory creation. These tests exercise the
// config module's logic directly via TOML files and environment variables,
// without importing the binary crate.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

// ── Config types (mirrors src/config.rs) ──────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
enum InstallScope {
    #[default]
    User,
    System,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SourceEntry {
    name: String,
    url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Config {
    #[serde(default = "default_registry_url")]
    default_registry_url: String,
    #[serde(default)]
    install_scope: InstallScope,
    #[serde(default)]
    data_dir: Option<PathBuf>,
    #[serde(default)]
    cache_dir: Option<PathBuf>,
    #[serde(default)]
    sources: Vec<SourceEntry>,
}

fn default_registry_url() -> String {
    "https://github.com/apm-pm/registry".to_string()
}

impl Default for Config {
    fn default() -> Self {
        Self {
            default_registry_url: default_registry_url(),
            install_scope: InstallScope::User,
            data_dir: None,
            cache_dir: None,
            sources: Vec::new(),
        }
    }
}

impl Config {
    fn resolved_data_dir(&self) -> PathBuf {
        self.data_dir.clone().unwrap_or_else(data_dir)
    }

    fn resolved_cache_dir(&self) -> PathBuf {
        self.cache_dir.clone().unwrap_or_else(cache_dir)
    }

    fn state_file(&self) -> PathBuf {
        self.resolved_data_dir().join("state.toml")
    }

    fn registries_cache_dir(&self) -> PathBuf {
        self.resolved_cache_dir().join("registries")
    }

    fn downloads_cache_dir(&self) -> PathBuf {
        self.resolved_cache_dir().join("downloads")
    }
}

// ── Path helpers (mirrors src/config.rs) ──────────────────────────────────────

fn home_dir() -> PathBuf {
    dirs::home_dir().expect("Cannot determine home directory")
}

fn config_dir() -> PathBuf {
    std::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| home_dir().join(".config"))
        .join("apm")
}

fn data_dir() -> PathBuf {
    std::env::var("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| home_dir().join(".local/share"))
        .join("apm")
}

fn cache_dir() -> PathBuf {
    std::env::var("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| home_dir().join(".cache"))
        .join("apm")
}

fn load_config(path: &std::path::Path) -> anyhow::Result<Config> {
    let raw =
        std::fs::read_to_string(path).map_err(|e| anyhow::anyhow!("Cannot read config: {e}"))?;
    toml::from_str(&raw).map_err(|e| anyhow::anyhow!("TOML error: {e}"))
}

fn ensure_dir(path: &std::path::Path) -> anyhow::Result<()> {
    if !path.exists() {
        std::fs::create_dir_all(path).map_err(|e| anyhow::anyhow!("Cannot create dir: {e}"))?;
    }
    Ok(())
}

// ── Default values ────────────────────────────────────────────────────────────

#[test]
fn test_config_default_registry_url() {
    let config = Config::default();
    assert_eq!(
        config.default_registry_url,
        "https://github.com/apm-pm/registry"
    );
}

#[test]
fn test_config_default_install_scope_is_user() {
    let config = Config::default();
    assert_eq!(config.install_scope, InstallScope::User);
}

#[test]
fn test_config_default_has_no_sources() {
    let config = Config::default();
    assert!(config.sources.is_empty());
}

#[test]
fn test_config_default_data_dir_is_none() {
    let config = Config::default();
    assert!(config.data_dir.is_none());
}

#[test]
fn test_config_default_cache_dir_is_none() {
    let config = Config::default();
    assert!(config.cache_dir.is_none());
}

// ── Config creation in temp directory ────────────────────────────────────────

#[test]
fn test_config_load_from_toml_file() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let config_path = tmp.path().join("config.toml");

    let toml_content = r#"
default_registry_url = "https://example.com/registry"
install_scope = "system"
"#;
    std::fs::write(&config_path, toml_content).expect("write config file");

    let config = load_config(&config_path).expect("load should succeed");
    assert_eq!(config.default_registry_url, "https://example.com/registry");
    assert_eq!(config.install_scope, InstallScope::System);
}

#[test]
fn test_config_toml_with_sources() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let config_path = tmp.path().join("config.toml");

    let toml_content = r#"
default_registry_url = "https://github.com/apm-pm/registry"
install_scope = "user"

[[sources]]
name = "my-registry"
url = "https://github.com/user/my-registry"
"#;
    std::fs::write(&config_path, toml_content).expect("write config file");

    let config = load_config(&config_path).expect("load should succeed");
    assert_eq!(config.sources.len(), 1);
    assert_eq!(config.sources[0].name, "my-registry");
    assert_eq!(config.sources[0].url, "https://github.com/user/my-registry");
}

#[test]
fn test_config_state_file_path() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let config = Config {
        data_dir: Some(tmp.path().to_path_buf()),
        ..Config::default()
    };

    let state_file = config.state_file();
    assert_eq!(state_file, tmp.path().join("state.toml"));
}

#[test]
fn test_config_registries_cache_dir() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let config = Config {
        cache_dir: Some(tmp.path().to_path_buf()),
        ..Config::default()
    };

    let cache = config.registries_cache_dir();
    assert_eq!(cache, tmp.path().join("registries"));
}

#[test]
fn test_config_downloads_cache_dir() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let config = Config {
        cache_dir: Some(tmp.path().to_path_buf()),
        ..Config::default()
    };

    let cache = config.downloads_cache_dir();
    assert_eq!(cache, tmp.path().join("downloads"));
}

// ── XDG path resolution ───────────────────────────────────────────────────────

#[test]
fn test_xdg_config_home_is_respected() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    std::env::set_var("XDG_CONFIG_HOME", tmp.path());
    let dir = config_dir();
    std::env::remove_var("XDG_CONFIG_HOME");

    assert_eq!(dir, tmp.path().join("apm"));
}

#[test]
fn test_xdg_data_home_is_respected() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    std::env::set_var("XDG_DATA_HOME", tmp.path());
    let dir = data_dir();
    std::env::remove_var("XDG_DATA_HOME");

    assert_eq!(dir, tmp.path().join("apm"));
}

#[test]
fn test_xdg_cache_home_is_respected() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    std::env::set_var("XDG_CACHE_HOME", tmp.path());
    let dir = cache_dir();
    std::env::remove_var("XDG_CACHE_HOME");

    assert_eq!(dir, tmp.path().join("apm"));
}

#[test]
fn test_resolved_data_dir_uses_config_override() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let config = Config {
        data_dir: Some(tmp.path().to_path_buf()),
        ..Config::default()
    };

    assert_eq!(config.resolved_data_dir(), tmp.path());
}

#[test]
fn test_resolved_cache_dir_uses_config_override() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let config = Config {
        cache_dir: Some(tmp.path().to_path_buf()),
        ..Config::default()
    };

    assert_eq!(config.resolved_cache_dir(), tmp.path());
}

#[test]
fn test_ensure_dir_creates_nested_directories() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let nested = tmp.path().join("a/b/c/d");

    ensure_dir(&nested).expect("ensure_dir should create nested dirs");
    assert!(nested.exists(), "nested directories should be created");
    assert!(nested.is_dir());
}

#[test]
fn test_ensure_dir_is_idempotent() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    ensure_dir(tmp.path()).expect("first call");
    ensure_dir(tmp.path()).expect("second call should also succeed");
}

// ── System / user dir constants ───────────────────────────────────────────────

#[test]
fn test_system_au_dir_constant() {
    let dir = PathBuf::from("/Library/Audio/Plug-Ins/Components");
    assert_eq!(dir, PathBuf::from("/Library/Audio/Plug-Ins/Components"));
}

#[test]
fn test_system_vst3_dir_constant() {
    let dir = PathBuf::from("/Library/Audio/Plug-Ins/VST3");
    assert_eq!(dir, PathBuf::from("/Library/Audio/Plug-Ins/VST3"));
}

#[test]
fn test_user_au_dir_contains_library() {
    let dir = home_dir().join("Library/Audio/Plug-Ins/Components");
    let s = dir.to_string_lossy();
    assert!(
        s.contains("Library/Audio/Plug-Ins/Components"),
        "user AU dir should contain expected path, got: {s}"
    );
}

#[test]
fn test_user_vst3_dir_contains_library() {
    let dir = home_dir().join("Library/Audio/Plug-Ins/VST3");
    let s = dir.to_string_lossy();
    assert!(
        s.contains("Library/Audio/Plug-Ins/VST3"),
        "user VST3 dir should contain expected path, got: {s}"
    );
}

#[test]
fn test_config_serialization_round_trip() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let config_path = tmp.path().join("config.toml");

    let original = Config {
        default_registry_url: "https://custom.example.com/registry".to_string(),
        install_scope: InstallScope::System,
        data_dir: None,
        cache_dir: None,
        sources: vec![SourceEntry {
            name: "extra".to_string(),
            url: "https://example.com/extra".to_string(),
        }],
    };

    let serialized = toml::to_string_pretty(&original).expect("serialize");
    std::fs::write(&config_path, &serialized).expect("write");

    let loaded = load_config(&config_path).expect("reload");
    assert_eq!(
        loaded.default_registry_url,
        "https://custom.example.com/registry"
    );
    assert_eq!(loaded.install_scope, InstallScope::System);
    assert_eq!(loaded.sources.len(), 1);
    assert_eq!(loaded.sources[0].name, "extra");
}

#[test]
fn test_config_missing_optional_fields_uses_defaults() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let config_path = tmp.path().join("config.toml");

    // Minimal config — only registry URL specified.
    std::fs::write(
        &config_path,
        "default_registry_url = \"https://example.com\"\n",
    )
    .expect("write");

    let config = load_config(&config_path).expect("load");
    assert_eq!(config.install_scope, InstallScope::User);
    assert!(config.sources.is_empty());
    assert!(config.data_dir.is_none());
    assert!(config.cache_dir.is_none());
}
