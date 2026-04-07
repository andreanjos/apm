mod support;

use std::{fs, path::Path};

use chrono::Utc;
use ed25519_dalek::{Signer, SigningKey};
use serde_json::Value;

use apm_core::{
    registry::PluginFormat,
    state::{InstallState, InstalledFormat, InstalledPlugin},
    LicensePayload, LicenseStatus, SignedLicense,
};

use support::{command, spawn_mock_commerce_server, test_plugin_archive_sha256, CliTestEnv};

const TEST_LICENSE_SIGNING_KEY_HEX: &str =
    "1f1e1d1c1b1a19181716151413121110ffeeddccbbaa99887766554433221100";

fn parse_stdout(output: &std::process::Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn licenses_json_reads_from_cache_offline() {
    let env = CliTestEnv::new();
    write_registry_fixture(&env, "https://example.com/paid-plugin.zip", "00");
    seed_license_cache(&env, "paid-plugin", "active", false);

    let output = command(&env)
        .env("APM_SERVER_URL", "http://192.0.2.1:1")
        .args(["--json", "licenses"])
        .output()
        .unwrap();
    assert!(output.status.success());

    let json = parse_stdout(&output);
    assert_eq!(json.as_array().unwrap().len(), 1);
    assert_eq!(json[0]["plugin_slug"], "paid-plugin");
    assert_eq!(json[0]["plugin_name"], "Paid Plugin");
    assert_eq!(json[0]["status"], "active");
    assert_eq!(json[0]["verified"], true);
    assert!(json[0]["issued_at"].is_string());
}

#[test]
fn licenses_human_reports_missing_cache() {
    let env = CliTestEnv::new();
    let output = command(&env).args(["licenses"]).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("No local license cache found"));
}

#[tokio::test(flavor = "multi_thread")]
async fn restore_json_reinstalls_purchased_plugin_on_fresh_machine() {
    let bootstrap_env = CliTestEnv::new();
    let restore_env = CliTestEnv::new();
    let home_dir = tempfile::tempdir().unwrap();
    let server = spawn_mock_commerce_server(1).await;
    let archive_sha = test_plugin_archive_sha256("paid-plugin");

    write_registry_fixture(
        &restore_env,
        &server.download_url("paid-plugin"),
        &archive_sha,
    );

    let bootstrap_buy = command(&bootstrap_env)
        .env("APM_SERVER_URL", &server.base_url)
        .env("APM_API_KEY", "apm_live_test_secret")
        .env("APM_BUY_POLL_INTERVAL_MS", "1")
        .env("APM_BUY_MAX_POLLS", "3")
        .args(["--json", "buy", "paid-plugin"])
        .output()
        .unwrap();
    assert!(bootstrap_buy.status.success());

    let output = command(&restore_env)
        .env("HOME", home_dir.path())
        .env("APM_SERVER_URL", &server.base_url)
        .env("APM_API_KEY", "apm_live_test_secret")
        .args(["--json", "restore"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");

    let json = parse_stdout(&output);
    assert_eq!(json["restored_plugins"].as_array().unwrap().len(), 1);
    assert_eq!(json["restored_plugins"][0]["plugin_slug"], "paid-plugin");
    assert_eq!(json["restored_plugins"][0]["status"], "active");
    assert!(json["skipped_plugins"].as_array().unwrap().is_empty());

    let state_path = restore_env.data_home.path().join("apm/state.toml");
    let state = InstallState::load_from(&state_path).unwrap();
    assert!(state.is_installed("paid-plugin"));
}

#[test]
fn list_json_merges_installed_plugins_with_license_annotations() {
    let env = CliTestEnv::new();
    let home_dir = tempfile::tempdir().unwrap();
    let paid_path = home_dir
        .path()
        .join("Library/Audio/Plug-Ins/Components/paid-plugin.component");
    let free_path = home_dir
        .path()
        .join("Library/Audio/Plug-Ins/Components/free-plugin.component");
    fs::create_dir_all(&paid_path).unwrap();
    fs::create_dir_all(&free_path).unwrap();

    write_registry_fixture(&env, "https://example.com/paid-plugin.zip", "00");
    write_free_registry_fixture(&env);
    seed_state(&env, &paid_path, &free_path);
    seed_license_cache(&env, "paid-plugin", "active", false);

    let output = command(&env)
        .env("HOME", home_dir.path())
        .args(["--json", "list"])
        .output()
        .unwrap();
    assert!(output.status.success());

    let json = parse_stdout(&output);
    assert_eq!(json.as_array().unwrap().len(), 2);

    let paid = json
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["name"] == "paid-plugin")
        .unwrap();
    assert_eq!(paid["license_status"], "licensed");

    let free = json
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["name"] == "free-plugin")
        .unwrap();
    assert_eq!(free["license_status"], "free");
}

fn write_registry_fixture(env: &CliTestEnv, download_url: &str, sha256: &str) {
    let plugins_dir = env
        .cache_home
        .path()
        .join("apm/registries/official/plugins");
    fs::create_dir_all(&plugins_dir).unwrap();

    fs::write(
        plugins_dir.join("paid-plugin.toml"),
        format!(
            r#"
slug = "paid-plugin"
name = "Paid Plugin"
vendor = "Apm"
version = "2.0.0"
description = "A paid plugin."
category = "effect"
license = "Commercial"
tags = ["paid", "commercial"]
is_paid = true
price_cents = 4999
currency = "usd"

[formats.au]
url = "{download_url}"
sha256 = "{sha256}"
install_type = "zip"
download_type = "direct"
"#
        ),
    )
    .unwrap();
}

fn write_free_registry_fixture(env: &CliTestEnv) {
    let plugins_dir = env
        .cache_home
        .path()
        .join("apm/registries/official/plugins");
    fs::create_dir_all(&plugins_dir).unwrap();

    fs::write(
        plugins_dir.join("free-plugin.toml"),
        r#"
slug = "free-plugin"
name = "Free Plugin"
vendor = "Apm"
version = "1.0.0"
description = "A free plugin."
category = "effect"
license = "Freeware"
tags = ["free"]

[formats.au]
url = "https://example.com/free-plugin.zip"
sha256 = "manual"
install_type = "zip"
"#,
    )
    .unwrap();
}

fn seed_state(env: &CliTestEnv, paid_path: &Path, free_path: &Path) {
    let mut state = InstallState::default();
    state.record_install(installed_plugin("paid-plugin", "2.0.0", paid_path));
    state.record_install(installed_plugin("free-plugin", "1.0.0", free_path));

    let state_path = env.data_home.path().join("apm/state.toml");
    if let Some(parent) = state_path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    state.save_to(&state_path).unwrap();
}

fn installed_plugin(name: &str, version: &str, path: &Path) -> InstalledPlugin {
    InstalledPlugin {
        name: name.to_string(),
        version: version.to_string(),
        vendor: "Apm".to_string(),
        formats: vec![InstalledFormat {
            format: PluginFormat::Au,
            path: path.to_path_buf(),
            sha256: "abc123".to_string(),
        }],
        installed_at: Utc::now(),
        source: "official".to_string(),
        pinned: false,
    }
}

fn seed_license_cache(env: &CliTestEnv, plugin_slug: &str, status: &str, tamper_signature: bool) {
    let db_path = env.data_home.path().join("apm/licenses.sqlite3");
    if let Some(parent) = db_path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    let connection = rusqlite::Connection::open(db_path).unwrap();
    connection
        .execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS licenses (
                plugin_slug TEXT PRIMARY KEY,
                order_id INTEGER NOT NULL,
                status TEXT NOT NULL,
                license_json TEXT NOT NULL,
                public_key_hex TEXT NOT NULL,
                last_synced_at TEXT NOT NULL
            );
            "#,
        )
        .unwrap();

    let mut signed = sign_license(plugin_slug, status);
    if tamper_signature {
        signed.signature = "00".repeat(64);
    }
    connection
        .execute(
            "INSERT OR REPLACE INTO licenses (plugin_slug, order_id, status, license_json, public_key_hex, last_synced_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                plugin_slug,
                signed.payload.order_id,
                status,
                serde_json::to_string(&signed).unwrap(),
                public_key_hex(),
                Utc::now(),
            ],
        )
        .unwrap();
}

fn sign_license(plugin_slug: &str, status: &str) -> SignedLicense {
    let status = match status {
        "active" => LicenseStatus::Active,
        "refunded" => LicenseStatus::Refunded,
        _ => LicenseStatus::Revoked,
    };
    let key_bytes = hex::decode(TEST_LICENSE_SIGNING_KEY_HEX).unwrap();
    let signing_key = SigningKey::from_bytes(&key_bytes.try_into().unwrap());
    let payload = LicensePayload {
        schema_version: 1,
        license_id: "lic_test".to_string(),
        user_id: 1,
        plugin_slug: plugin_slug.to_string(),
        plugin_version: Some("2.0.0".to_string()),
        order_id: 42,
        issued_at: Utc::now(),
        revoked_at: match status {
            LicenseStatus::Active => None,
            _ => Some(Utc::now()),
        },
        status,
    };
    let signature = signing_key.sign(&serde_json::to_vec(&payload).unwrap());

    SignedLicense {
        payload,
        signature: hex::encode(signature.to_bytes()),
        key_id: "dev-ed25519-1".to_string(),
    }
}

fn public_key_hex() -> String {
    let key_bytes = hex::decode(TEST_LICENSE_SIGNING_KEY_HEX).unwrap();
    let signing_key = SigningKey::from_bytes(&key_bytes.try_into().unwrap());
    hex::encode(signing_key.verifying_key().to_bytes())
}
