use serde::{Deserialize, Serialize};

use super::{ensure_not_cancelled, ApmEngine, EngineEvent, EventSink};
use crate::registry::{self, Registry};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegistrySyncResult {
    pub sources: Vec<RegistrySyncSourceResult>,
}

impl RegistrySyncResult {
    pub fn failed_count(&self) -> usize {
        self.sources
            .iter()
            .filter(|source| source.is_error())
            .count()
    }

    pub fn has_errors(&self) -> bool {
        self.failed_count() > 0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum RegistrySyncSourceResult {
    Ok {
        name: String,
        catalog_item_count: usize,
        installable_product_count: usize,
    },
    Error {
        name: String,
        error: String,
    },
}

impl RegistrySyncSourceResult {
    pub fn name(&self) -> &str {
        match self {
            Self::Ok { name, .. } | Self::Error { name, .. } => name,
        }
    }

    pub fn is_error(&self) -> bool {
        matches!(self, Self::Error { .. })
    }
}

impl ApmEngine {
    pub fn sync_registries(
        &self,
        events: &mut impl EventSink,
    ) -> anyhow::Result<RegistrySyncResult> {
        let sources = self.config.sources();
        let registries_cache_dir = self.config.registries_cache_dir();
        let mut results = Vec::with_capacity(sources.len());

        events.emit(EngineEvent::RegistrySyncStarted {
            source_count: sources.len(),
        });
        ensure_not_cancelled(events)?;

        for source in &sources {
            ensure_not_cancelled(events)?;
            events.emit(EngineEvent::RegistrySourceSyncStarted {
                source: source.name.clone(),
            });

            let result = match registry::sync::sync_source(source, &registries_cache_dir) {
                Ok(()) => {
                    let source_cache = registries_cache_dir.join(&source.name);
                    let loaded = Registry::load_from_cache(&source_cache).ok();
                    let catalog_item_count = loaded.as_ref().map(Registry::len).unwrap_or(0);
                    let installable_product_count = loaded
                        .as_ref()
                        .map(|registry| {
                            registry
                                .plugins
                                .values()
                                .filter(|package| package.is_installable_product())
                                .count()
                        })
                        .unwrap_or(0);

                    events.emit(EngineEvent::RegistrySourceSyncFinished {
                        source: source.name.clone(),
                        catalog_item_count,
                        installable_product_count,
                    });

                    RegistrySyncSourceResult::Ok {
                        name: source.name.clone(),
                        catalog_item_count,
                        installable_product_count,
                    }
                }
                Err(error) => {
                    let error = error.to_string();
                    events.emit(EngineEvent::RegistrySourceSyncFailed {
                        source: source.name.clone(),
                        error: error.clone(),
                    });
                    RegistrySyncSourceResult::Error {
                        name: source.name.clone(),
                        error,
                    }
                }
            };

            results.push(result);
            ensure_not_cancelled(events)?;
        }

        let result = RegistrySyncResult { sources: results };
        events.emit(EngineEvent::RegistrySyncFinished {
            source_count: result.sources.len(),
            failed_count: result.failed_count(),
        });
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn write_registry(root: &std::path::Path) {
        let plugins = root.join("plugins");
        std::fs::create_dir_all(&plugins).expect("create plugins dir");
        std::fs::write(
            plugins.join("sync-test.toml"),
            r#"
slug = "sync-test"
name = "Sync Test"
vendor = "apm"
version = "1.0.0"
description = "Registry sync fixture"
category = "effects"
license = "freeware"

[formats.vst3]
url = "https://example.com/sync-test.zip"
sha256 = "abc123"
install_type = "zip"
download_type = "manual"
"#,
        )
        .expect("write plugin fixture");
    }

    #[test]
    fn sync_registries_emits_events_and_returns_source_counts() {
        let temp = tempfile::tempdir().expect("temp dir");
        let registry = temp.path().join("registry");
        write_registry(&registry);
        let config = Config {
            default_registry_url: registry.to_string_lossy().into_owned(),
            data_dir: Some(temp.path().join("data")),
            cache_dir: Some(temp.path().join("cache")),
            ..Config::default()
        };
        let engine = ApmEngine::new(config);
        let mut events = Vec::new();

        let result = engine
            .sync_registries(&mut |event| events.push(event))
            .expect("sync should succeed");

        assert_eq!(
            result.sources,
            vec![RegistrySyncSourceResult::Ok {
                name: "official".to_string(),
                catalog_item_count: 1,
                installable_product_count: 1,
            }]
        );
        assert!(!result.has_errors());

        assert!(matches!(
            events.first(),
            Some(EngineEvent::RegistrySyncStarted { source_count: 1 })
        ));
        assert!(matches!(
            events.get(1),
            Some(EngineEvent::RegistrySourceSyncStarted { source })
                if source == "official"
        ));
        assert!(matches!(
            events.get(2),
            Some(EngineEvent::RegistrySourceSyncFinished {
                source,
                catalog_item_count: 1,
                installable_product_count: 1,
            }) if source == "official"
        ));
        assert!(matches!(
            events.last(),
            Some(EngineEvent::RegistrySyncFinished {
                source_count: 1,
                failed_count: 0,
            })
        ));
    }

    struct CancelAfterFirstEvent {
        events: Vec<EngineEvent>,
    }

    impl EventSink for CancelAfterFirstEvent {
        fn emit(&mut self, event: EngineEvent) {
            self.events.push(event);
        }

        fn cancel_requested(&self) -> bool {
            !self.events.is_empty()
        }
    }

    #[test]
    fn sync_registries_stops_when_cancellation_is_requested() {
        let temp = tempfile::tempdir().expect("temp dir");
        let registry = temp.path().join("registry");
        write_registry(&registry);
        let config = Config {
            default_registry_url: registry.to_string_lossy().into_owned(),
            data_dir: Some(temp.path().join("data")),
            cache_dir: Some(temp.path().join("cache")),
            ..Config::default()
        };
        let engine = ApmEngine::new(config);
        let mut sink = CancelAfterFirstEvent { events: Vec::new() };

        let error = engine
            .sync_registries(&mut sink)
            .expect_err("sync should stop on cancellation");

        assert_eq!(
            error.to_string(),
            crate::engine::OPERATION_CANCELED_BY_REQUEST
        );
        assert_eq!(sink.events.len(), 1);
        assert!(matches!(
            sink.events.first(),
            Some(EngineEvent::RegistrySyncStarted { source_count: 1 })
        ));
    }
}
