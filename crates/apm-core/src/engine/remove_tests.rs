use std::path::{Path, PathBuf};

use chrono::Utc;

use super::*;
use crate::config::Config;
use crate::registry::PluginFormat;
use crate::state::{InstallOrigin, InstallState, InstalledFormat, InstalledPlugin};

fn test_config() -> (tempfile::TempDir, Config) {
    let temp = tempfile::tempdir().expect("temp dir");
    let config = Config {
        data_dir: Some(temp.path().join("data")),
        cache_dir: Some(temp.path().join("cache")),
        ..Config::default()
    };
    (temp, config)
}

fn installed_plugin(
    name: &str,
    origin: InstallOrigin,
    formats: Vec<InstalledFormat>,
) -> InstalledPlugin {
    InstalledPlugin {
        name: name.to_string(),
        version: "1.0.0".to_string(),
        vendor: "Test Vendor".to_string(),
        formats,
        installed_at: Utc::now(),
        source: "official".to_string(),
        pinned: false,
        origin,
    }
}

fn installed_format(format: PluginFormat, path: PathBuf) -> InstalledFormat {
    InstalledFormat {
        format,
        path,
        sha256: String::new(),
    }
}

fn write_state(config: &Config, plugin: InstalledPlugin) {
    InstallState {
        version: 1,
        plugins: vec![plugin],
    }
    .save(config)
    .expect("save state");
}

fn make_bundle(path: &Path) {
    std::fs::create_dir_all(path.join("Contents")).expect("create bundle");
    std::fs::write(path.join("Contents/Info.plist"), "test").expect("write plist");
}

struct CancelAfterRemoveStart {
    events: Vec<EngineEvent>,
}

impl EventSink for CancelAfterRemoveStart {
    fn emit(&mut self, event: EngineEvent) {
        self.events.push(event);
    }

    fn cancel_requested(&self) -> bool {
        self.events
            .iter()
            .any(|event| matches!(event, EngineEvent::RemoveStarted { .. }))
    }
}

struct CancelAfterFirstFormatRemoved {
    events: Vec<EngineEvent>,
}

impl EventSink for CancelAfterFirstFormatRemoved {
    fn emit(&mut self, event: EngineEvent) {
        self.events.push(event);
    }

    fn cancel_requested(&self) -> bool {
        self.events
            .iter()
            .any(|event| matches!(event, EngineEvent::RemoveFormatRemoved { .. }))
    }
}

#[test]
fn remove_package_removes_apm_owned_bundles_and_state() {
    let (temp, config) = test_config();
    let bundle = temp.path().join("DirectZip.vst3");
    make_bundle(&bundle);
    write_state(
        &config,
        installed_plugin(
            "direct-zip",
            InstallOrigin::Apm,
            vec![installed_format(PluginFormat::Vst3, bundle.clone())],
        ),
    );
    let engine = ApmEngine::new(config.clone());
    let mut events = Vec::new();

    let result = engine
        .remove_package(
            RemovePackageRequest {
                slug: "direct-zip".to_string(),
                dry_run: false,
            },
            &mut |event| events.push(event),
        )
        .expect("remove should succeed");

    assert!(matches!(
        result,
        RemovePackageResult::Removed {
            state_only: false,
            removed_formats,
            ..
        } if removed_formats.len() == 1 && removed_formats[0].existed
    ));
    assert!(!bundle.exists());
    assert!(InstallState::load(&config)
        .expect("state should load")
        .find("direct-zip")
        .is_none());
    assert!(events.iter().any(|event| {
        matches!(
            event,
            EngineEvent::RemoveFormatRemoved {
                slug,
                format: PluginFormat::Vst3,
                path
            } if slug == "direct-zip" && path == &bundle
        )
    }));
    assert!(events.iter().any(|event| {
        matches!(
            event,
            EngineEvent::RemoveFinished {
                slug,
                removed_format_count: 1
            } if slug == "direct-zip"
        )
    }));
}

#[test]
fn remove_package_cancellation_after_one_format_repairs_partial_state() {
    let (temp, config) = test_config();
    let vst3 = temp.path().join("DirectZip.vst3");
    let au = temp.path().join("DirectZip.component");
    make_bundle(&vst3);
    make_bundle(&au);
    write_state(
        &config,
        installed_plugin(
            "direct-zip",
            InstallOrigin::Apm,
            vec![
                installed_format(PluginFormat::Vst3, vst3.clone()),
                installed_format(PluginFormat::Au, au.clone()),
            ],
        ),
    );
    let engine = ApmEngine::new(config.clone());
    let mut events = CancelAfterFirstFormatRemoved { events: Vec::new() };

    let error = engine
        .remove_package(
            RemovePackageRequest {
                slug: "direct-zip".to_string(),
                dry_run: false,
            },
            &mut events,
        )
        .expect_err("remove should stop after first format cancellation");

    assert_eq!(
        error.to_string(),
        crate::engine::OPERATION_CANCELED_BY_REQUEST
    );
    assert!(!vst3.exists());
    assert!(au.exists());

    let state = InstallState::load(&config).expect("state should load");
    let plugin = state.find("direct-zip").expect("remaining format in state");
    assert_eq!(plugin.formats.len(), 1);
    assert_eq!(plugin.formats[0].format, PluginFormat::Au);
    assert_eq!(plugin.formats[0].path, au);
    assert!(events.events.iter().any(|event| {
        matches!(
            event,
            EngineEvent::RemoveStateRecorded { slug } if slug == "direct-zip"
        )
    }));
    assert!(!events.events.iter().any(|event| {
        matches!(
            event,
            EngineEvent::RemoveFinished { slug, .. } if slug == "direct-zip"
        )
    }));
}

#[test]
fn remove_package_dry_run_does_not_touch_files_or_state() {
    let (temp, config) = test_config();
    let bundle = temp.path().join("DirectZip.vst3");
    make_bundle(&bundle);
    write_state(
        &config,
        installed_plugin(
            "direct-zip",
            InstallOrigin::Apm,
            vec![installed_format(PluginFormat::Vst3, bundle.clone())],
        ),
    );
    let engine = ApmEngine::new(config.clone());
    let mut events = Vec::new();

    let result = engine
        .remove_package(
            RemovePackageRequest {
                slug: "direct-zip".to_string(),
                dry_run: true,
            },
            &mut |event| events.push(event),
        )
        .expect("dry run should succeed");

    assert!(matches!(
        result,
        RemovePackageResult::DryRun {
            would_delete_files: true,
            formats,
            ..
        } if formats.len() == 1 && formats[0].existed
    ));
    assert!(bundle.exists());
    assert!(InstallState::load(&config)
        .expect("state should load")
        .find("direct-zip")
        .is_some());
    assert!(events.is_empty());
}

#[test]
fn remove_package_honors_cancellation_after_start_before_deleting_bundle() {
    let (temp, config) = test_config();
    let bundle = temp.path().join("DirectZip.vst3");
    make_bundle(&bundle);
    write_state(
        &config,
        installed_plugin(
            "direct-zip",
            InstallOrigin::Apm,
            vec![installed_format(PluginFormat::Vst3, bundle.clone())],
        ),
    );
    let engine = ApmEngine::new(config.clone());
    let mut events = CancelAfterRemoveStart { events: Vec::new() };

    let error = engine
        .remove_package(
            RemovePackageRequest {
                slug: "direct-zip".to_string(),
                dry_run: false,
            },
            &mut events,
        )
        .expect_err("remove should stop on cancellation");

    assert_eq!(
        error.to_string(),
        crate::engine::OPERATION_CANCELED_BY_REQUEST
    );
    assert!(bundle.exists());
    assert!(InstallState::load(&config)
        .expect("state should load")
        .find("direct-zip")
        .is_some());
    assert!(events.events.iter().any(|event| {
        matches!(
            event,
            EngineEvent::RemoveStarted { slug, .. } if slug == "direct-zip"
        )
    }));
    assert!(!events.events.iter().any(|event| {
        matches!(
            event,
            EngineEvent::RemoveFormatRemoved { slug, .. } if slug == "direct-zip"
        )
    }));
}

#[test]
fn remove_package_refuses_external_install_that_still_exists() {
    let (temp, config) = test_config();
    let bundle = temp.path().join("Scanned.component");
    make_bundle(&bundle);
    write_state(
        &config,
        installed_plugin(
            "scanned",
            InstallOrigin::External,
            vec![installed_format(PluginFormat::Au, bundle.clone())],
        ),
    );
    let engine = ApmEngine::new(config.clone());
    let mut events = Vec::new();

    let result = engine
        .remove_package(
            RemovePackageRequest {
                slug: "scanned".to_string(),
                dry_run: false,
            },
            &mut |event| events.push(event),
        )
        .expect("classification should succeed");

    assert!(matches!(
        result,
        RemovePackageResult::ExternalInstallPresent { .. }
    ));
    assert!(bundle.exists());
    assert!(InstallState::load(&config)
        .expect("state should load")
        .find("scanned")
        .is_some());
    assert!(events.is_empty());
}

#[test]
fn remove_package_cleans_stale_external_state() {
    let (temp, config) = test_config();
    let missing_bundle = temp.path().join("Missing.component");
    write_state(
        &config,
        installed_plugin(
            "scanned",
            InstallOrigin::External,
            vec![installed_format(PluginFormat::Au, missing_bundle.clone())],
        ),
    );
    let engine = ApmEngine::new(config.clone());
    let mut events = Vec::new();

    let result = engine
        .remove_package(
            RemovePackageRequest {
                slug: "scanned".to_string(),
                dry_run: false,
            },
            &mut |event| events.push(event),
        )
        .expect("stale state cleanup should succeed");

    assert!(matches!(
        result,
        RemovePackageResult::Removed {
            state_only: true,
            removed_formats,
            ..
        } if removed_formats.is_empty()
    ));
    assert!(InstallState::load(&config)
        .expect("state should load")
        .find("scanned")
        .is_none());
    assert!(events.iter().any(|event| {
        matches!(
            event,
            EngineEvent::RemoveFinished {
                slug,
                removed_format_count: 0
            } if slug == "scanned"
        )
    }));
}

#[test]
fn remove_package_honors_cancellation_after_start_before_clearing_stale_external_state() {
    let (temp, config) = test_config();
    let missing_bundle = temp.path().join("Missing.component");
    write_state(
        &config,
        installed_plugin(
            "scanned",
            InstallOrigin::External,
            vec![installed_format(PluginFormat::Au, missing_bundle)],
        ),
    );
    let engine = ApmEngine::new(config.clone());
    let mut events = CancelAfterRemoveStart { events: Vec::new() };

    let error = engine
        .remove_package(
            RemovePackageRequest {
                slug: "scanned".to_string(),
                dry_run: false,
            },
            &mut events,
        )
        .expect_err("stale state cleanup should stop on cancellation");

    assert_eq!(
        error.to_string(),
        crate::engine::OPERATION_CANCELED_BY_REQUEST
    );
    assert!(InstallState::load(&config)
        .expect("state should load")
        .find("scanned")
        .is_some());
    assert!(events.events.iter().any(|event| {
        matches!(
            event,
            EngineEvent::RemoveStarted { slug, .. } if slug == "scanned"
        )
    }));
    assert!(!events.events.iter().any(|event| {
        matches!(
            event,
            EngineEvent::RemoveStateRecorded { slug } if slug == "scanned"
        )
    }));
}

#[test]
fn remove_package_rejects_unexpected_bundle_extension() {
    let (temp, config) = test_config();
    let unsafe_path = temp.path().join("not-a-plugin.txt");
    std::fs::write(&unsafe_path, "do not delete").expect("write unsafe path");
    write_state(
        &config,
        installed_plugin(
            "bad-state",
            InstallOrigin::Apm,
            vec![installed_format(PluginFormat::Vst3, unsafe_path.clone())],
        ),
    );
    let engine = ApmEngine::new(config.clone());
    let mut events = Vec::new();

    let error = engine
        .remove_package(
            RemovePackageRequest {
                slug: "bad-state".to_string(),
                dry_run: false,
            },
            &mut |event| events.push(event),
        )
        .expect_err("unsafe extension should fail");

    assert!(error.to_string().contains("Refusing to remove"));
    assert!(unsafe_path.exists());
    assert!(InstallState::load(&config)
        .expect("state should load")
        .find("bad-state")
        .is_some());
    assert!(events.iter().any(|event| {
        matches!(
            event,
            EngineEvent::RemoveFailed { slug, .. } if slug == "bad-state"
        )
    }));
}
