use std::fs;
use std::path::Path;

use assert_fs::TempDir;

fn write_free_plugin_fixture(cache_root: &Path) {
    let plugins_dir = cache_root.join("apm/registries/official/plugins");
    fs::create_dir_all(&plugins_dir).unwrap();

    let plugin_toml = r#"
slug = "test-free"
name = "Test Free"
vendor = "Apm"
version = "1.0.0"
description = "A free plugin for offline install testing."
category = "effect"
license = "Freeware"
tags = ["free"]

[formats.au]
url = "https://example.com/test-free-au.zip"
sha256 = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
install_type = "zip"
download_type = "direct"
"#;

    fs::write(plugins_dir.join("test-free.toml"), plugin_toml).unwrap();
}

#[test]
fn free_install_does_not_depend_on_server() {
    let tmp = TempDir::new().unwrap();
    let config_home = tmp.path().join("config");
    let data_home = tmp.path().join("data");
    let cache_home = tmp.path().join("cache");

    write_free_plugin_fixture(&cache_home);

    let assert = assert_cmd::cargo::cargo_bin_cmd!("apm")
        .args(["install", "test-free", "--dry-run"])
        .env("APM_SERVER_URL", "http://192.0.2.1:1")
        .env("XDG_CONFIG_HOME", &config_home)
        .env("XDG_DATA_HOME", &data_home)
        .env("XDG_CACHE_HOME", &cache_home)
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    let output = format!("{stdout}{stderr}");

    assert!(
        stdout.contains("[dry-run] Would install Test Free v1.0.0 (AU)"),
        "expected dry-run install output, got: {stdout}"
    );
    assert!(
        !output.contains("connection refused"),
        "install should not try to reach apm-server: {output}"
    );
    assert!(
        !output.contains("timed out"),
        "install should not hang on apm-server: {output}"
    );
}
