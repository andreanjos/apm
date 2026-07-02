use std::collections::HashMap;

use anyhow::Result;
use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::bundle_id_store::BundleIdStore;
use crate::config::{Config, InstallScope};
use crate::registry::{matcher, PluginFormat, Registry};
use crate::scanner;
use crate::state::{InstallOrigin, InstallState, InstalledFormat, InstalledPlugin};

use super::{ensure_not_cancelled, ApmEngine, EngineEvent, EventSink};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScanPackageFilter {
    #[default]
    All,
    Tracked,
    Untracked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScanPackagesRequest {
    #[serde(default)]
    pub filter: ScanPackageFilter,
    #[serde(default)]
    pub learn_bundle_ids: bool,
    #[serde(default)]
    pub adopt_external: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScanPackagesResult {
    pub scanned_count: usize,
    pub visible_count: usize,
    pub matched_count: usize,
    pub tracked_count: usize,
    pub adopted_count: usize,
    pub learned_bundle_id_count: usize,
    pub au_count: usize,
    pub vst3_count: usize,
    pub plugins: Vec<ScannedPackageSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScannedPackageSummary {
    pub name: String,
    pub version: String,
    pub vendor: String,
    pub format: PluginFormat,
    pub scope: InstallScope,
    pub path: String,
    pub tracked_by_apm: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin: Option<InstallOrigin>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub registry_slug: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub match_method: Option<ScanMatchMethod>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScanMatchMethod {
    BundleId,
    NameVendor,
    NameOnly,
}

#[derive(Clone)]
struct Matched {
    slug: Option<String>,
    method: Option<ScanMatchMethod>,
}

struct ExternalAdoption {
    slug: String,
    version: String,
    vendor: String,
    source: String,
    formats: Vec<InstalledFormat>,
}

impl ScanPackagesRequest {
    pub fn inspect(filter: ScanPackageFilter) -> Self {
        Self {
            filter,
            learn_bundle_ids: false,
            adopt_external: false,
        }
    }

    pub fn reconcile() -> Self {
        Self {
            filter: ScanPackageFilter::All,
            learn_bundle_ids: true,
            adopt_external: true,
        }
    }
}

impl ApmEngine {
    pub fn scan_packages(
        &self,
        request: ScanPackagesRequest,
        observer: &mut impl EventSink,
    ) -> Result<ScanPackagesResult> {
        observer.emit(EngineEvent::ScanStarted);
        ensure_not_cancelled(observer)?;

        let scanned = scanner::scan_plugins(&self.config);
        let scanned_count = scanned.len();
        let mut state = InstallState::load(&self.config).unwrap_or_default();
        let registry = Registry::load_all_sources(&self.config).ok();
        let mut bundle_id_store = BundleIdStore::open(&self.config).ok();

        let visible = filter_scanned(scanned, &state, request.filter);
        let matched = match_scanned(
            &visible,
            registry.as_ref(),
            request.learn_bundle_ids,
            &mut bundle_id_store,
        );
        let learned_bundle_id_count = matched.learned_bundle_id_count;
        if learned_bundle_id_count > 0 {
            if let Some(store) = bundle_id_store.as_ref() {
                store.save()?;
            }
        }

        ensure_not_cancelled(observer)?;
        let adopted_count = if request.adopt_external && request.filter == ScanPackageFilter::All {
            adopt_external_matches(
                &self.config,
                &visible,
                &matched.matches,
                registry.as_ref(),
                &mut state,
            )?
        } else {
            0
        };

        let result = scan_result(
            scanned_count,
            &visible,
            &matched.matches,
            &state,
            adopted_count,
            learned_bundle_id_count,
        );
        observer.emit(EngineEvent::ScanFinished {
            scanned_count: result.scanned_count,
            matched_count: result.matched_count,
            adopted_count: result.adopted_count,
        });
        Ok(result)
    }
}

fn filter_scanned(
    scanned: Vec<scanner::ScannedPlugin>,
    state: &InstallState,
    filter: ScanPackageFilter,
) -> Vec<scanner::ScannedPlugin> {
    scanned
        .into_iter()
        .filter(|plugin| match filter {
            ScanPackageFilter::All => true,
            ScanPackageFilter::Tracked => scanned_origin(state, plugin).is_some(),
            ScanPackageFilter::Untracked => scanned_origin(state, plugin).is_none(),
        })
        .collect()
}

struct MatchScannedResult {
    matches: Vec<Matched>,
    learned_bundle_id_count: usize,
}

fn match_scanned(
    plugins: &[scanner::ScannedPlugin],
    registry: Option<&Registry>,
    learn_bundle_ids: bool,
    writable_bundle_id_store: &mut Option<BundleIdStore>,
) -> MatchScannedResult {
    let mut matches = Vec::with_capacity(plugins.len());
    let mut learned_bundle_id_count = 0usize;

    for plugin in plugins {
        let matched = registry.and_then(|registry| {
            matcher::match_plugin(plugin, registry, writable_bundle_id_store.as_ref())
        });

        if learn_bundle_ids {
            if let Some(plugin_match) = matched.as_ref() {
                if plugin_match.method != matcher::MatchMethod::BundleId {
                    if let Some(store) = writable_bundle_id_store.as_mut() {
                        if matcher::auto_learn(plugin, &plugin_match.registry_plugin.slug, store) {
                            learned_bundle_id_count += 1;
                        }
                    }
                }
            }
        }

        matches.push(Matched {
            slug: matched
                .as_ref()
                .map(|plugin_match| plugin_match.registry_plugin.slug.clone()),
            method: matched.map(|plugin_match| scan_match_method(plugin_match.method)),
        });
    }

    MatchScannedResult {
        matches,
        learned_bundle_id_count,
    }
}

fn scan_result(
    scanned_count: usize,
    plugins: &[scanner::ScannedPlugin],
    matches: &[Matched],
    state: &InstallState,
    adopted_count: usize,
    learned_bundle_id_count: usize,
) -> ScanPackagesResult {
    let summaries = plugins
        .iter()
        .zip(matches.iter())
        .map(|(plugin, matched)| scanned_package_summary(plugin, matched, state))
        .collect::<Vec<_>>();
    let au_count = summaries
        .iter()
        .filter(|plugin| plugin.format == PluginFormat::Au)
        .count();
    let vst3_count = summaries
        .iter()
        .filter(|plugin| plugin.format == PluginFormat::Vst3)
        .count();
    let matched_count = summaries
        .iter()
        .filter(|plugin| plugin.registry_slug.is_some())
        .count();
    let tracked_count = summaries
        .iter()
        .filter(|plugin| plugin.tracked_by_apm)
        .count();

    ScanPackagesResult {
        scanned_count,
        visible_count: summaries.len(),
        matched_count,
        tracked_count,
        adopted_count,
        learned_bundle_id_count,
        au_count,
        vst3_count,
        plugins: summaries,
    }
}

fn scanned_package_summary(
    plugin: &scanner::ScannedPlugin,
    matched: &Matched,
    state: &InstallState,
) -> ScannedPackageSummary {
    let origin = scanned_origin(state, plugin);
    ScannedPackageSummary {
        name: plugin.name.clone(),
        version: plugin.version.clone(),
        vendor: plugin.vendor.clone(),
        format: registry_format(plugin.format),
        scope: scan_scope(plugin.scope),
        path: plugin.path.display().to_string(),
        tracked_by_apm: origin.is_some(),
        origin,
        registry_slug: matched.slug.clone(),
        match_method: matched.method,
    }
}

fn scanned_origin(state: &InstallState, plugin: &scanner::ScannedPlugin) -> Option<InstallOrigin> {
    state
        .plugins
        .iter()
        .find(|installed| {
            installed
                .formats
                .iter()
                .any(|format| format.path == plugin.path)
                || installed.name.eq_ignore_ascii_case(&plugin.name)
        })
        .map(|installed| installed.origin)
}

fn adopt_external_matches(
    config: &Config,
    plugins: &[scanner::ScannedPlugin],
    matches: &[Matched],
    registry: Option<&Registry>,
    state: &mut InstallState,
) -> Result<usize> {
    let Some(registry) = registry else {
        return Ok(0);
    };

    let mut by_slug: HashMap<String, ExternalAdoption> = HashMap::new();

    for (plugin, matched) in plugins.iter().zip(matches.iter()) {
        let Some(slug) = &matched.slug else {
            continue;
        };
        let Some(definition) = registry.find(slug) else {
            continue;
        };
        let format = registry_format(plugin.format);
        if !definition.formats.contains_key(&format) {
            continue;
        }

        let entry = by_slug
            .entry(slug.clone())
            .or_insert_with(|| ExternalAdoption {
                slug: slug.clone(),
                version: if plugin.version == "unknown" {
                    definition.version.clone()
                } else {
                    plugin.version.clone()
                },
                vendor: definition.vendor.clone(),
                source: definition
                    .source_name
                    .clone()
                    .unwrap_or_else(|| "official".to_string()),
                formats: Vec::new(),
            });

        if entry.version == "unknown" && plugin.version != "unknown" {
            entry.version = plugin.version.clone();
        }

        if !entry
            .formats
            .iter()
            .any(|entry| entry.format == format && entry.path == plugin.path)
        {
            entry.formats.push(InstalledFormat {
                format,
                path: plugin.path.clone(),
                sha256: String::new(),
            });
        }
    }

    let mut changed = 0usize;
    for adoption in by_slug.into_values() {
        if adoption.formats.is_empty() {
            continue;
        }

        if let Some(existing) = state.find_mut(&adoption.slug) {
            if existing.origin == InstallOrigin::Apm {
                continue;
            }

            let before = existing.formats.len();
            for format in adoption.formats {
                if !existing
                    .formats
                    .iter()
                    .any(|entry| entry.format == format.format && entry.path == format.path)
                {
                    existing.formats.push(format);
                }
            }
            if existing.formats.len() != before {
                changed += 1;
            }
            continue;
        }

        state.record_install(InstalledPlugin {
            name: adoption.slug,
            version: adoption.version,
            vendor: adoption.vendor,
            formats: adoption.formats,
            installed_at: Utc::now(),
            source: adoption.source,
            pinned: false,
            origin: InstallOrigin::External,
        });
        changed += 1;
    }

    if changed > 0 {
        state.save(config)?;
    }

    Ok(changed)
}

fn scan_match_method(method: matcher::MatchMethod) -> ScanMatchMethod {
    match method {
        matcher::MatchMethod::BundleId => ScanMatchMethod::BundleId,
        matcher::MatchMethod::NameAndVendor => ScanMatchMethod::NameVendor,
        matcher::MatchMethod::NameOnly => ScanMatchMethod::NameOnly,
    }
}

fn registry_format(format: scanner::PluginFormat) -> PluginFormat {
    match format {
        scanner::PluginFormat::Au => PluginFormat::Au,
        scanner::PluginFormat::Vst3 => PluginFormat::Vst3,
    }
}

fn scan_scope(scope: scanner::InstallScope) -> InstallScope {
    match scope {
        scanner::InstallScope::System => InstallScope::System,
        scanner::InstallScope::User => InstallScope::User,
    }
}

#[cfg(test)]
#[path = "scan_tests.rs"]
mod tests;
