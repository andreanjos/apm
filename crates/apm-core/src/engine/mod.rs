use std::collections::HashSet;
use std::path::PathBuf;

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::registry::{
    self, DownloadType, InstallType, PluginDefinition, PluginFormat, ProductType, Registry,
};
use crate::state::{InstallOrigin, InstallState};

mod events;
mod install_archive;
#[cfg(feature = "reqwest")]
mod install_download;
mod install_execute;
mod install_plan;
mod pin;
mod remove;
mod scan;
mod sync;
mod updates;

pub use crate::cancel::{ensure_not_cancelled, CancellationToken, NoopCancellationToken};
pub use events::{
    is_operation_canceled, EngineEvent, EventSink, NoopEventSink, OPERATION_CANCELED_BY_REQUEST,
};
pub use install_execute::{InstallPackageRequest, InstallPackageResult};
pub use install_plan::{
    InstallHandoff, InstallHandoffKind, InstallHandoffResult, InstallHandoffTarget,
    InstallPlanFormat, InstallPlanRequest, InstallPlanResult, InstallPlanStatus,
    PackageInstallPlan, VendorInstallerPlan,
};
pub use pin::{PinnedPackagesRequest, SetPackagePinRequest, SetPackagePinResult};
pub use remove::{RemoveFormatSummary, RemovePackageRequest, RemovePackageResult};
pub use scan::{
    ScanMatchMethod, ScanPackageFilter, ScanPackagesRequest, ScanPackagesResult,
    ScannedPackageSummary,
};
pub use sync::{RegistrySyncResult, RegistrySyncSourceResult};
pub use updates::{
    AvailableUpdatesRequest, AvailableUpdatesResult, PackageUpdateAction, PackageUpdateSummary,
    UpdatePackageRequest, UpdatePackageResult,
};

#[derive(Debug, Clone)]
pub struct ApmEngine {
    config: Config,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageSearchRequest {
    #[serde(default)]
    pub query: String,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub vendor: Option<String>,
    #[serde(default)]
    pub tag: Option<String>,
    #[serde(default)]
    pub access: PackageAccessFilter,
    #[serde(default)]
    pub install_state: PackageInstallStateFilter,
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstalledPackagesRequest {
    #[serde(default)]
    pub format: Option<PluginFormat>,
    #[serde(default)]
    pub sort: InstalledPackageSort,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstalledPackageSort {
    #[default]
    Name,
    Version,
    Date,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PackageAccessFilter {
    #[default]
    Any,
    Free,
    Paid,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PackageInstallStateFilter {
    #[default]
    Any,
    Installed,
    NotInstalled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum PackageSearchResult {
    CatalogEmpty,
    Matches {
        total_matches: usize,
        packages: Vec<PackageSummary>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageSummary {
    pub slug: String,
    pub name: String,
    pub vendor: String,
    pub version: String,
    pub product_type: ProductType,
    pub category: String,
    pub subcategory: Option<String>,
    pub license: String,
    pub description: String,
    pub tags: Vec<String>,
    pub is_paid: bool,
    pub price_cents: Option<i64>,
    pub currency: Option<String>,
    pub source_name: Option<String>,
    pub is_installable: bool,
    pub installed: bool,
    pub installed_version: Option<String>,
    pub formats: Vec<PackageFormatSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageFormatSummary {
    pub format: PluginFormat,
    pub install_type: InstallType,
    pub download_type: DownloadType,
    pub bundle_path: Option<String>,
    pub has_checksum: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageDetails {
    pub summary: PackageSummary,
    pub aliases: Vec<String>,
    pub homepage: Option<String>,
    pub purchase_url: Option<String>,
    pub available_versions: Vec<String>,
    pub bundle_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum PackageDetailsResult {
    CatalogEmpty,
    NotFound,
    Found { package: Box<PackageDetails> },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstalledPackageSummary {
    pub slug: String,
    pub version: String,
    pub vendor: String,
    pub formats: Vec<InstalledFormatSummary>,
    pub installed_at: DateTime<Utc>,
    pub source: String,
    pub pinned: bool,
    pub origin: InstallOrigin,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstalledFormatSummary {
    pub format: PluginFormat,
    pub path: PathBuf,
    pub sha256: String,
}

impl ApmEngine {
    pub fn new(config: Config) -> Self {
        Self { config }
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn search_packages(&self, request: PackageSearchRequest) -> Result<PackageSearchResult> {
        let registry = Registry::load_all_sources(&self.config)?;
        if registry.is_empty() {
            return Ok(PackageSearchResult::CatalogEmpty);
        }

        let state = InstallState::load(&self.config)?;
        let installed_slugs = installed_slug_set(&state);

        let matches: Vec<&PluginDefinition> = registry::search::search(
            &registry,
            &request.query,
            request.category.as_deref(),
            request.vendor.as_deref(),
            request.tag.as_deref(),
        )
        .into_iter()
        .filter(|package| package_matches_filters(package, &request, &installed_slugs))
        .collect();

        let total_matches = matches.len();
        let mut packages: Vec<PackageSummary> = matches
            .into_iter()
            .map(|package| package_summary(package, state.find(&package.slug)))
            .collect();
        if let Some(limit) = request.limit {
            packages.truncate(limit);
        }

        Ok(PackageSearchResult::Matches {
            total_matches,
            packages,
        })
    }

    pub fn package_details(
        &self,
        slug: &str,
        include_versions: bool,
    ) -> Result<PackageDetailsResult> {
        let registry = Registry::load_all_sources(&self.config)?;
        let state = InstallState::load(&self.config)?;
        if registry.is_empty() {
            return Ok(PackageDetailsResult::CatalogEmpty);
        }
        let Some(package) = registry.find(slug) else {
            return Ok(PackageDetailsResult::NotFound);
        };

        Ok(PackageDetailsResult::Found {
            package: Box::new(PackageDetails {
                summary: package_summary(package, state.find(&package.slug)),
                aliases: package.aliases.clone(),
                homepage: package.homepage.clone(),
                purchase_url: package.purchase_url.clone(),
                available_versions: if include_versions {
                    package.available_versions()
                } else {
                    Vec::new()
                },
                bundle_ids: package.bundle_ids.clone(),
            }),
        })
    }

    pub fn installed_packages(
        &self,
        request: InstalledPackagesRequest,
    ) -> Result<Vec<InstalledPackageSummary>> {
        let state = InstallState::load(&self.config)?;
        let mut packages: Vec<InstalledPackageSummary> = state
            .plugins
            .iter()
            .filter(|package| installed_package_matches_format(package, request.format))
            .map(installed_package_summary)
            .collect();
        sort_installed_packages(&mut packages, request.sort);
        Ok(packages)
    }
}

fn installed_slug_set(state: &InstallState) -> HashSet<String> {
    state
        .plugins
        .iter()
        .map(|plugin| plugin.name.to_lowercase())
        .collect()
}

fn package_matches_filters(
    package: &PluginDefinition,
    request: &PackageSearchRequest,
    installed_slugs: &HashSet<String>,
) -> bool {
    match request.access {
        PackageAccessFilter::Any => {}
        PackageAccessFilter::Free if package.is_paid => return false,
        PackageAccessFilter::Paid if !package.is_paid => return false,
        PackageAccessFilter::Free | PackageAccessFilter::Paid => {}
    }

    match request.install_state {
        PackageInstallStateFilter::Any => {}
        PackageInstallStateFilter::Installed => {
            if !installed_slugs.contains(&package.slug.to_lowercase()) {
                return false;
            }
        }
        PackageInstallStateFilter::NotInstalled => {
            if !package.is_installable_product() {
                return false;
            }
            if installed_slugs.contains(&package.slug.to_lowercase()) {
                return false;
            }
        }
    }

    true
}

fn package_summary(
    package: &PluginDefinition,
    installed: Option<&crate::state::InstalledPlugin>,
) -> PackageSummary {
    PackageSummary {
        slug: package.slug.clone(),
        name: package.name.clone(),
        vendor: package.vendor.clone(),
        version: package.version.clone(),
        product_type: package.product_type.clone(),
        category: package.category.clone(),
        subcategory: package.subcategory.clone(),
        license: package.license.clone(),
        description: package.description.clone(),
        tags: package.tags.clone(),
        is_paid: package.is_paid,
        price_cents: package.price_cents,
        currency: package.currency.clone(),
        source_name: package.source_name.clone(),
        is_installable: package.is_installable_product(),
        installed: installed.is_some(),
        installed_version: installed.map(|installed| installed.version.clone()),
        formats: package_format_summaries(package),
    }
}

fn package_format_summaries(package: &PluginDefinition) -> Vec<PackageFormatSummary> {
    let mut formats: Vec<PackageFormatSummary> = package
        .formats
        .iter()
        .map(|(format, source)| PackageFormatSummary {
            format: *format,
            install_type: source.install_type,
            download_type: source.download_type.clone(),
            bundle_path: source.bundle_path.clone(),
            has_checksum: !source.sha256.trim().is_empty(),
        })
        .collect();
    formats.sort_by_key(|format| format.format.to_string());
    formats
}

fn installed_package_summary(package: &crate::state::InstalledPlugin) -> InstalledPackageSummary {
    InstalledPackageSummary {
        slug: package.name.clone(),
        version: package.version.clone(),
        vendor: package.vendor.clone(),
        formats: package
            .formats
            .iter()
            .map(|format| InstalledFormatSummary {
                format: format.format,
                path: format.path.clone(),
                sha256: format.sha256.clone(),
            })
            .collect(),
        installed_at: package.installed_at,
        source: package.source.clone(),
        pinned: package.pinned,
        origin: package.origin,
    }
}

fn installed_package_matches_format(
    package: &crate::state::InstalledPlugin,
    format: Option<PluginFormat>,
) -> bool {
    match format {
        Some(format) => package.formats.iter().any(|entry| entry.format == format),
        None => true,
    }
}

fn sort_installed_packages(packages: &mut [InstalledPackageSummary], sort: InstalledPackageSort) {
    match sort {
        InstalledPackageSort::Name => {
            packages.sort_by_key(|package| package.slug.to_lowercase());
        }
        InstalledPackageSort::Version => {
            packages.sort_by(|a, b| a.version.cmp(&b.version));
        }
        InstalledPackageSort::Date => {
            packages.sort_by_key(|package| std::cmp::Reverse(package.installed_at));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use chrono::{Duration, Utc};

    use super::*;
    use crate::state::{InstalledFormat, InstalledPlugin};

    fn test_config() -> (tempfile::TempDir, Config) {
        let temp = tempfile::tempdir().expect("temp dir");
        let config = Config {
            data_dir: Some(temp.path().join("data")),
            cache_dir: Some(temp.path().join("cache")),
            ..Config::default()
        };
        write_test_registry(&config);
        (temp, config)
    }

    fn write_test_registry(config: &Config) {
        let plugins_dir = config.registries_cache_dir().join("official/plugins");
        std::fs::create_dir_all(&plugins_dir).expect("create registry plugins dir");
        std::fs::write(
            plugins_dir.join("test-reverb.toml"),
            r#"
slug = "test-reverb"
name = "Test Reverb"
vendor = "Test Vendor"
version = "1.0.0"
description = "A test reverb plugin"
category = "effects"
subcategory = "reverb"
license = "freeware"
tags = ["reverb", "test"]
homepage = "https://example.com"

[formats.vst3]
url = "https://example.com/test-reverb.zip"
sha256 = "abc123"
install_type = "zip"
bundle_path = "TestReverb.vst3"
"#,
        )
        .expect("write reverb fixture");
        std::fs::write(
            plugins_dir.join("test-synth.toml"),
            r#"
slug = "test-synth"
name = "Test Synth"
vendor = "Synth Vendor"
version = "2.1.0"
description = "A test synthesizer plugin"
category = "instruments"
subcategory = "synthesizer"
license = "MIT"
tags = ["synth", "synthesizer", "test"]
homepage = "https://example.com/synth"

[formats.vst3]
url = "https://example.com/test-synth.zip"
sha256 = "def456"
install_type = "zip"
bundle_path = "TestSynth.vst3"

[formats.au]
url = "https://example.com/test-synth-au.zip"
sha256 = "def457"
install_type = "zip"
bundle_path = "TestSynth.component"

[[releases]]
version = "2.0.0"

[releases.formats.vst3]
url = "https://example.com/test-synth-2.0.0.zip"
sha256 = "def400"
install_type = "zip"
bundle_path = "TestSynth.vst3"

[releases.formats.au]
url = "https://example.com/test-synth-2.0.0-au.zip"
sha256 = "def401"
install_type = "zip"
bundle_path = "TestSynth.component"

[[releases]]
version = "1.5.0"

[releases.formats.vst3]
url = "https://example.com/test-synth-1.5.0.zip"
sha256 = "def300"
install_type = "zip"
bundle_path = "TestSynth.vst3"

[releases.formats.au]
url = "https://example.com/test-synth-1.5.0-au.zip"
sha256 = "def301"
install_type = "zip"
bundle_path = "TestSynth.component"
"#,
        )
        .expect("write synth fixture");
    }

    fn write_state(config: &Config, plugins: Vec<InstalledPlugin>) {
        InstallState {
            version: 1,
            plugins,
        }
        .save(config)
        .expect("save state");
    }

    fn installed_plugin(name: &str, version: &str) -> InstalledPlugin {
        installed_plugin_with_format_and_date(name, version, PluginFormat::Vst3, Utc::now())
    }

    fn installed_plugin_with_format_and_date(
        name: &str,
        version: &str,
        format: PluginFormat,
        installed_at: DateTime<Utc>,
    ) -> InstalledPlugin {
        let extension = match format {
            PluginFormat::Au => "component",
            PluginFormat::Vst3 => "vst3",
            PluginFormat::App => "app",
        };

        InstalledPlugin {
            name: name.to_string(),
            version: version.to_string(),
            vendor: "Test Vendor".to_string(),
            formats: vec![InstalledFormat {
                format,
                path: PathBuf::from(format!("/tmp/{name}.{extension}")),
                sha256: "abc123".to_string(),
            }],
            installed_at,
            source: "official".to_string(),
            pinned: false,
            origin: InstallOrigin::Apm,
        }
    }

    #[test]
    fn search_packages_returns_structured_registry_results() {
        let (_temp, config) = test_config();
        let engine = ApmEngine::new(config);

        let result = engine
            .search_packages(PackageSearchRequest {
                query: "reverb".to_string(),
                ..PackageSearchRequest::default()
            })
            .expect("search should succeed");
        let PackageSearchResult::Matches {
            total_matches,
            packages,
        } = result
        else {
            panic!("catalog should be populated");
        };

        assert_eq!(total_matches, 1);
        assert_eq!(packages[0].slug, "test-reverb");
        assert_eq!(packages[0].product_type, ProductType::Plugin);
        assert!(!packages[0].installed);
    }

    #[test]
    fn search_packages_can_filter_installed_packages() {
        let (_temp, config) = test_config();
        write_state(&config, vec![installed_plugin("test-synth", "2.0.0")]);
        let engine = ApmEngine::new(config);

        let result = engine
            .search_packages(PackageSearchRequest {
                query: "test".to_string(),
                install_state: PackageInstallStateFilter::Installed,
                ..PackageSearchRequest::default()
            })
            .expect("search should succeed");
        let PackageSearchResult::Matches {
            total_matches,
            packages,
        } = result
        else {
            panic!("catalog should be populated");
        };

        assert_eq!(total_matches, 1);
        assert_eq!(packages[0].slug, "test-synth");
        assert!(packages[0].installed);
        assert_eq!(packages[0].installed_version.as_deref(), Some("2.0.0"));
    }

    #[test]
    fn search_packages_marks_installed_packages_without_filtering() {
        let (_temp, config) = test_config();
        write_state(&config, vec![installed_plugin("test-synth", "2.0.0")]);
        let engine = ApmEngine::new(config);

        let result = engine
            .search_packages(PackageSearchRequest {
                query: "synth".to_string(),
                ..PackageSearchRequest::default()
            })
            .expect("search should succeed");
        let PackageSearchResult::Matches {
            total_matches,
            packages,
        } = result
        else {
            panic!("catalog should be populated");
        };

        assert_eq!(total_matches, 1);
        assert_eq!(packages[0].slug, "test-synth");
        assert!(packages[0].installed);
    }

    #[test]
    fn search_packages_reports_empty_catalog() {
        let temp = tempfile::tempdir().expect("temp dir");
        let config = Config {
            data_dir: Some(temp.path().join("data")),
            cache_dir: Some(temp.path().join("cache")),
            ..Config::default()
        };
        let engine = ApmEngine::new(config);

        let result = engine
            .search_packages(PackageSearchRequest::default())
            .expect("search should load");

        assert_eq!(result, PackageSearchResult::CatalogEmpty);
    }

    #[test]
    fn package_details_can_include_versions() {
        let (_temp, config) = test_config();
        let engine = ApmEngine::new(config);

        let details = engine
            .package_details("test-synth", true)
            .expect("details should load");
        let PackageDetailsResult::Found { package: details } = details else {
            panic!("package should exist");
        };

        assert_eq!(details.summary.slug, "test-synth");
        assert_eq!(details.available_versions, vec!["2.1.0", "2.0.0", "1.5.0"]);
        assert_eq!(details.summary.formats.len(), 2);
    }

    #[test]
    fn package_details_reports_empty_catalog() {
        let temp = tempfile::tempdir().expect("temp dir");
        let config = Config {
            data_dir: Some(temp.path().join("data")),
            cache_dir: Some(temp.path().join("cache")),
            ..Config::default()
        };
        let engine = ApmEngine::new(config);

        let result = engine
            .package_details("missing", false)
            .expect("details should load");

        assert_eq!(result, PackageDetailsResult::CatalogEmpty);
    }

    #[test]
    fn installed_packages_returns_sorted_library_summaries() {
        let (_temp, config) = test_config();
        write_state(
            &config,
            vec![
                installed_plugin("z-delay", "1.0.0"),
                installed_plugin("a-reverb", "2.0.0"),
            ],
        );
        let engine = ApmEngine::new(config);

        let packages = engine
            .installed_packages(InstalledPackagesRequest::default())
            .expect("installed packages should load");

        assert_eq!(packages[0].slug, "a-reverb");
        assert_eq!(packages[1].slug, "z-delay");
        assert_eq!(packages[0].formats[0].format, PluginFormat::Vst3);
    }

    #[test]
    fn installed_packages_can_filter_by_format() {
        let (_temp, config) = test_config();
        write_state(
            &config,
            vec![
                installed_plugin_with_format_and_date(
                    "vst-plugin",
                    "1.0.0",
                    PluginFormat::Vst3,
                    Utc::now(),
                ),
                installed_plugin_with_format_and_date(
                    "au-plugin",
                    "1.0.0",
                    PluginFormat::Au,
                    Utc::now(),
                ),
            ],
        );
        let engine = ApmEngine::new(config);

        let packages = engine
            .installed_packages(InstalledPackagesRequest {
                format: Some(PluginFormat::Au),
                ..InstalledPackagesRequest::default()
            })
            .expect("installed packages should load");

        assert_eq!(packages.len(), 1);
        assert_eq!(packages[0].slug, "au-plugin");
        assert_eq!(packages[0].formats[0].format, PluginFormat::Au);
    }

    #[test]
    fn installed_packages_can_sort_by_install_date() {
        let (_temp, config) = test_config();
        let now = Utc::now();
        write_state(
            &config,
            vec![
                installed_plugin_with_format_and_date(
                    "old-plugin",
                    "1.0.0",
                    PluginFormat::Vst3,
                    now - Duration::hours(1),
                ),
                installed_plugin_with_format_and_date(
                    "new-plugin",
                    "1.0.0",
                    PluginFormat::Vst3,
                    now,
                ),
            ],
        );
        let engine = ApmEngine::new(config);

        let packages = engine
            .installed_packages(InstalledPackagesRequest {
                sort: InstalledPackageSort::Date,
                ..InstalledPackagesRequest::default()
            })
            .expect("installed packages should load");

        assert_eq!(packages[0].slug, "new-plugin");
        assert_eq!(packages[1].slug, "old-plugin");
    }
}
