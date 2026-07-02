use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tracing::debug;

use crate::{
    config::{ensure_dir, Config},
    file::atomic_write,
};

pub const PRIVILEGED_HELPER_LABEL: &str = "apm privileged install helper";
pub const PRIVILEGED_HELPER_BUNDLE_IDENTIFIER: &str = "com.apm.pkg-helper";
pub const PRIVILEGED_HELPER_MACH_SERVICE_NAME: &str = "com.apm.pkg-helper";
pub const PRIVILEGED_HELPER_INSTALL_PATH: &str =
    "/Library/PrivilegedHelperTools/com.apm.pkg-helper";
pub const PRIVILEGED_HELPER_LAUNCHD_PLIST_PATH: &str =
    "/Library/LaunchDaemons/com.apm.pkg-helper.plist";
pub const PRIVILEGED_HELPER_REQUIRED_SIGNING_IDENTITY: &str = "Developer ID Application";
pub const PRIVILEGED_INSTALL_RECEIPT_STORE_RELATIVE_PATH: &str =
    "service/privileged-install-receipts.json";
pub const PRIVILEGED_INSTALL_RECEIPT_STORE_SCHEMA_VERSION: u32 = 1;

pub fn privileged_install_receipt_store_path(config: &Config) -> PathBuf {
    config
        .resolved_data_dir()
        .join(PRIVILEGED_INSTALL_RECEIPT_STORE_RELATIVE_PATH)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrivilegedInstallReceiptStore {
    pub schema_version: u32,
    #[serde(default)]
    pub receipts: Vec<PrivilegedInstallReceipt>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrivilegedInstallReceipt {
    pub operation_id: String,
    pub package_slug: String,
    pub package_name: String,
    pub package_version: String,
    pub source_name: String,
    pub installer_path: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub installer_sha256: Option<String>,
    pub preflight_snapshot: PrivilegedInstallPreflightSnapshot,
    #[serde(default)]
    pub pkgutil_receipt_identifiers: Vec<String>,
    #[serde(default)]
    pub installed_paths: Vec<PathBuf>,
    pub recorded_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrivilegedInstallPreflightSnapshot {
    pub captured_at: DateTime<Utc>,
    #[serde(default)]
    pub paths: Vec<PathBuf>,
}

impl Default for PrivilegedInstallReceiptStore {
    fn default() -> Self {
        Self {
            schema_version: PRIVILEGED_INSTALL_RECEIPT_STORE_SCHEMA_VERSION,
            receipts: Vec::new(),
        }
    }
}

impl PrivilegedInstallReceiptStore {
    pub fn load(config: &Config) -> Result<Self> {
        Self::load_from(&privileged_install_receipt_store_path(config))
    }

    pub fn load_from(path: &Path) -> Result<Self> {
        if !path.exists() {
            debug!(
                "Privileged install receipt store not found at {}; starting empty.",
                path.display()
            );
            return Ok(Self::default());
        }

        let raw = std::fs::read_to_string(path).with_context(|| {
            format!(
                "Cannot read privileged install receipts: {}",
                path.display()
            )
        })?;
        let store: Self = serde_json::from_str(&raw).map_err(|error| {
            anyhow::anyhow!(
                "JSON parse error in privileged install receipt store {}: {}",
                path.display(),
                error
            )
        })?;
        if store.schema_version != PRIVILEGED_INSTALL_RECEIPT_STORE_SCHEMA_VERSION {
            bail!(
                "Unsupported privileged install receipt store schema {} in {}; expected {}",
                store.schema_version,
                path.display(),
                PRIVILEGED_INSTALL_RECEIPT_STORE_SCHEMA_VERSION
            );
        }
        Ok(store)
    }

    pub fn save(&self, config: &Config) -> Result<()> {
        self.save_to(&privileged_install_receipt_store_path(config))
    }

    pub fn save_to(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            ensure_dir(parent).with_context(|| {
                format!(
                    "Cannot create privileged install receipt directory: {}",
                    parent.display()
                )
            })?;
        }

        let mut content = serde_json::to_vec_pretty(self)
            .context("Failed to serialize privileged install receipts to JSON")?;
        content.push(b'\n');
        atomic_write(path, &content).with_context(|| {
            format!(
                "Cannot write privileged install receipt store atomically: {}",
                path.display()
            )
        })?;

        debug!(
            "Privileged install receipt store saved to {}",
            path.display()
        );
        Ok(())
    }

    pub fn record_receipt(&mut self, receipt: PrivilegedInstallReceipt) {
        if let Some(existing) = self
            .receipts
            .iter_mut()
            .find(|existing| existing.operation_id == receipt.operation_id)
        {
            *existing = receipt;
        } else {
            self.receipts.push(receipt);
        }

        self.receipts.sort_by(|left, right| {
            left.package_slug
                .cmp(&right.package_slug)
                .then(left.package_version.cmp(&right.package_version))
                .then(left.operation_id.cmp(&right.operation_id))
        });
    }
}
