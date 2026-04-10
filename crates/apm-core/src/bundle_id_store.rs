// Local bundle ID store — persists learned CFBundleIdentifier → registry slug
// mappings so scanned plugins can be matched reliably across sessions.
//
// Stored at `<data_dir>/bundle_ids.toml` and populated automatically when
// `apm scan` finds matches via name/vendor that haven't been seen before.

use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::config::Config;

/// A single learned mapping from a bundle ID prefix to a registry slug.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Mapping {
    slug: String,
}

/// Persistent store for learned bundle ID → registry slug mappings.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct StoreData {
    /// Maps bundle ID prefix (e.g. "com.fabfilter.Pro-Q") to registry slug.
    #[serde(default)]
    mappings: HashMap<String, Mapping>,
}

pub struct BundleIdStore {
    path: PathBuf,
    data: StoreData,
}

impl BundleIdStore {
    /// Open the store, creating it if it doesn't exist.
    pub fn open(config: &Config) -> Result<Self> {
        let path = config.resolved_data_dir().join("bundle_ids.toml");
        let data = if path.exists() {
            let content = std::fs::read_to_string(&path)?;
            toml::from_str(&content).unwrap_or_default()
        } else {
            StoreData::default()
        };
        Ok(Self { path, data })
    }

    /// Look up a registry slug by bundle ID prefix.
    ///
    /// Checks if any stored prefix matches the start of the given bundle ID.
    pub fn find_slug(&self, bundle_id: &str) -> Option<&str> {
        let bid_lower = bundle_id.to_lowercase();
        for (prefix, mapping) in &self.data.mappings {
            if bid_lower.starts_with(&prefix.to_lowercase()) {
                return Some(&mapping.slug);
            }
        }
        None
    }

    /// Record a learned mapping. Returns true if this was a new mapping.
    pub fn learn(&mut self, bundle_id_prefix: &str, slug: &str) -> bool {
        if self.data.mappings.contains_key(bundle_id_prefix) {
            return false;
        }
        self.data.mappings.insert(
            bundle_id_prefix.to_string(),
            Mapping {
                slug: slug.to_string(),
            },
        );
        true
    }

    /// Returns all learned mappings as (bundle_id_prefix, slug) pairs.
    pub fn all_mappings(&self) -> Vec<(&str, &str)> {
        self.data
            .mappings
            .iter()
            .map(|(k, v)| (k.as_str(), v.slug.as_str()))
            .collect()
    }

    /// Persist the store to disk.
    pub fn save(&self) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = toml::to_string_pretty(&self.data)?;
        std::fs::write(&self.path, content)?;
        Ok(())
    }
}
