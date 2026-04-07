mod support;

use std::fs;

use ed25519_dalek::{Signer, SigningKey};
use rusqlite::Connection;
use serde_json::Value;

use apm_core::{verify_signed_license, LicensePayload, LicenseStatus, SignedLicense};

use support::{command, spawn_mock_commerce_server, CliTestEnv};

const TEST_LICENSE_SIGNING_KEY_HEX: &str =
    "1f1e1d1c1b1a19181716151413121110ffeeddccbbaa99887766554433221100";

fn parse_stdout(output: &std::process::Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn fulfilled_buy_caches_signed_license_locally() {
    let env = CliTestEnv::new();
    let server = spawn_mock_commerce_server(1).await;

    let output = command(&env)
        .env("APM_SERVER_URL", &server.base_url)
        .env("APM_API_KEY", "apm_live_test_secret")
        .env("APM_BUY_POLL_INTERVAL_MS", "1")
        .env("APM_BUY_MAX_POLLS", "3")
        .args(["--json", "buy", "paid-plugin"])
        .output()
        .unwrap();
    assert!(output.status.success());

    let json = parse_stdout(&output);
    assert_eq!(json["status"], "fulfilled");

    let db_path = env.data_home.path().join("apm/licenses.sqlite3");
    let connection = Connection::open(db_path).unwrap();
    let (status, license_json, public_key_hex): (String, String, String) = connection
        .query_row(
            "SELECT status, license_json, public_key_hex FROM licenses WHERE plugin_slug = ?1",
            ["paid-plugin"],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(status, "active");

    let license: SignedLicense = serde_json::from_str(&license_json).unwrap();
    verify_signed_license(&public_key_hex, &license).unwrap();
}

#[test]
fn cached_paid_license_verifies_offline_during_install() {
    let env = CliTestEnv::new();
    write_paid_plugin_fixture(&env);
    seed_license_cache(&env, "paid-plugin", "active", false);

    let output = command(&env)
        .env("APM_SERVER_URL", "http://192.0.2.1:1")
        .args(["install", "paid-plugin", "--dry-run"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{:?}", output);
}

#[test]
fn invalid_cached_paid_license_blocks_install() {
    let env = CliTestEnv::new();
    write_paid_plugin_fixture(&env);
    seed_license_cache(&env, "paid-plugin", "active", true);

    let output = command(&env)
        .env("APM_SERVER_URL", "http://192.0.2.1:1")
        .args(["install", "paid-plugin", "--dry-run"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("requires a valid cached license"));
}

fn write_paid_plugin_fixture(env: &CliTestEnv) {
    let plugins_dir = env
        .cache_home
        .path()
        .join("apm/registries/official/plugins");
    fs::create_dir_all(&plugins_dir).unwrap();

    fs::write(
        plugins_dir.join("paid-plugin.toml"),
        r#"
slug = "paid-plugin"
name = "Paid Plugin"
vendor = "Apm"
version = "2.0.0"
description = "A paid plugin."
category = "effect"
license = "Commercial"
tags = ["paid"]
is_paid = true
price_cents = 4999
currency = "usd"

[formats.au]
url = "https://example.com/paid-plugin-au.zip"
sha256 = "manual"
install_type = "zip"
"#,
    )
    .unwrap();
}

fn seed_license_cache(env: &CliTestEnv, plugin_slug: &str, status: &str, tamper_signature: bool) {
    let db_path = env.data_home.path().join("apm/licenses.sqlite3");
    if let Some(parent) = db_path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    let connection = Connection::open(db_path).unwrap();
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
                chrono::Utc::now(),
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
        issued_at: chrono::Utc::now(),
        revoked_at: match status {
            LicenseStatus::Active => None,
            _ => Some(chrono::Utc::now()),
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
