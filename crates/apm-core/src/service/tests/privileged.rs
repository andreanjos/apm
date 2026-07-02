use std::path::PathBuf;

use chrono::{TimeZone, Utc};

use crate::config::Config;

use super::*;

#[test]
fn privileged_receipt_store_path_lives_under_data_dir() {
    let temp = tempfile::tempdir().expect("temp dir");
    let config = Config {
        data_dir: Some(temp.path().join("data")),
        ..Config::default()
    };

    assert_eq!(
        privileged_install_receipt_store_path(&config),
        temp.path()
            .join("data")
            .join(PRIVILEGED_INSTALL_RECEIPT_STORE_RELATIVE_PATH)
    );
}

#[test]
fn privileged_receipt_store_loads_empty_when_missing() {
    let temp = tempfile::tempdir().expect("temp dir");
    let missing = temp.path().join("service/privileged-install-receipts.json");

    let store = PrivilegedInstallReceiptStore::load_from(&missing).expect("load missing store");

    assert_eq!(
        store.schema_version,
        PRIVILEGED_INSTALL_RECEIPT_STORE_SCHEMA_VERSION
    );
    assert!(store.receipts.is_empty());
}

#[test]
fn privileged_receipt_store_round_trips_json() {
    let temp = tempfile::tempdir().expect("temp dir");
    let path = temp.path().join("nested/privileged-install-receipts.json");
    let mut store = PrivilegedInstallReceiptStore::default();
    store.record_receipt(make_receipt("op-2", "zebra", "2.0.0"));
    store.record_receipt(make_receipt("op-1", "analog-lab", "1.0.0"));

    store.save_to(&path).expect("save store");
    let loaded = PrivilegedInstallReceiptStore::load_from(&path).expect("reload store");

    assert_eq!(loaded, store);
    assert_eq!(
        loaded
            .receipts
            .iter()
            .map(|receipt| receipt.operation_id.as_str())
            .collect::<Vec<_>>(),
        vec!["op-1", "op-2"]
    );
}

#[test]
fn privileged_receipt_store_rejects_unsupported_schema() {
    let temp = tempfile::tempdir().expect("temp dir");
    let path = temp.path().join("privileged-install-receipts.json");
    std::fs::write(&path, r#"{"schema_version":999,"receipts":[]}"#)
        .expect("write unsupported schema");

    let error = PrivilegedInstallReceiptStore::load_from(&path)
        .expect_err("unsupported schema should fail");

    assert!(
        error
            .to_string()
            .contains("Unsupported privileged install receipt store schema 999"),
        "unexpected error: {error}"
    );
}

#[test]
fn privileged_receipt_store_requires_schema_on_disk() {
    let temp = tempfile::tempdir().expect("temp dir");
    let path = temp.path().join("privileged-install-receipts.json");
    std::fs::write(&path, r#"{"receipts":[]}"#).expect("write schema-less store");

    let error =
        PrivilegedInstallReceiptStore::load_from(&path).expect_err("missing schema should fail");

    assert!(
        error.to_string().contains("missing field `schema_version`"),
        "unexpected error: {error}"
    );
}

#[test]
fn privileged_receipt_store_replaces_existing_operation() {
    let mut store = PrivilegedInstallReceiptStore::default();
    store.record_receipt(make_receipt("op-1", "analog-lab", "1.0.0"));
    store.record_receipt(make_receipt("op-1", "analog-lab", "1.1.0"));

    assert_eq!(store.receipts.len(), 1);
    assert_eq!(store.receipts[0].package_version, "1.1.0");
}

#[test]
fn privileged_contract_uses_receipt_store_constants() {
    let policy = privileged_install_policy();

    assert_eq!(
        policy.design.helper.bundle_identifier,
        PRIVILEGED_HELPER_BUNDLE_IDENTIFIER
    );
    assert_eq!(
        policy.design.helper.install_path,
        PRIVILEGED_HELPER_INSTALL_PATH
    );
    assert_eq!(
        policy.design.rollback.receipt_store_relative_path,
        PRIVILEGED_INSTALL_RECEIPT_STORE_RELATIVE_PATH
    );
}

fn make_receipt(
    operation_id: impl Into<String>,
    package_slug: impl Into<String>,
    package_version: impl Into<String>,
) -> PrivilegedInstallReceipt {
    let recorded_at = Utc.with_ymd_and_hms(2026, 7, 1, 12, 0, 0).unwrap();
    let package_slug = package_slug.into();

    PrivilegedInstallReceipt {
        operation_id: operation_id.into(),
        package_slug: package_slug.clone(),
        package_name: package_slug,
        package_version: package_version.into(),
        source_name: "official".to_string(),
        installer_path: PathBuf::from("/tmp/apm/example.pkg"),
        installer_sha256: Some(
            "9a129038d9a00aed0cf6a7ea059ca50a813449061ab87848cf1a13eafdf33b2c".to_string(),
        ),
        preflight_snapshot: PrivilegedInstallPreflightSnapshot {
            captured_at: recorded_at,
            paths: vec![PathBuf::from(
                "/Library/Audio/Plug-Ins/Components/Example.component",
            )],
        },
        pkgutil_receipt_identifiers: vec!["com.example.pkg".to_string()],
        installed_paths: vec![PathBuf::from(
            "/Library/Audio/Plug-Ins/Components/Example.component",
        )],
        recorded_at,
    }
}
