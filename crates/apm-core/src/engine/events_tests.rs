use std::collections::HashSet;
use std::path::PathBuf;

use super::*;
use crate::registry::{InstallType, PluginFormat};

#[test]
fn engine_event_names_match_serialized_event_tags() {
    let names: Vec<String> = sample_events()
        .into_iter()
        .map(|event| {
            serde_json::to_value(event)
                .expect("engine event serializes")
                .get("event")
                .expect("serialized engine event should have an event tag")
                .as_str()
                .expect("event tag should be a string")
                .to_string()
        })
        .collect();

    assert_eq!(names, EngineEvent::SERIALIZED_NAMES);
    assert_eq!(
        names.len(),
        names.iter().collect::<HashSet<_>>().len(),
        "engine event names should be unique"
    );
}

fn sample_events() -> Vec<EngineEvent> {
    vec![
        EngineEvent::ScanStarted,
        EngineEvent::ScanFinished {
            scanned_count: 1,
            matched_count: 1,
            adopted_count: 1,
        },
        EngineEvent::RegistrySyncStarted { source_count: 1 },
        EngineEvent::RegistrySourceSyncStarted {
            source: "default".to_string(),
        },
        EngineEvent::RegistrySourceSyncFinished {
            source: "default".to_string(),
            catalog_item_count: 1,
            installable_product_count: 1,
        },
        EngineEvent::RegistrySourceSyncFailed {
            source: "default".to_string(),
            error: "failed".to_string(),
        },
        EngineEvent::RegistrySyncFinished {
            source_count: 1,
            failed_count: 0,
        },
        EngineEvent::InstallStarted {
            slug: "pkg".to_string(),
            version: "1.0.0".to_string(),
            format_count: 1,
        },
        EngineEvent::InstallFormatStarted {
            slug: "pkg".to_string(),
            format: PluginFormat::Vst3,
        },
        EngineEvent::InstallDownloadStarted {
            slug: "pkg".to_string(),
            format: PluginFormat::Vst3,
            url: "https://example.test/pkg.zip".to_string(),
        },
        EngineEvent::InstallDownloadProgress {
            slug: "pkg".to_string(),
            format: PluginFormat::Vst3,
            bytes: 1,
            total_bytes: Some(2),
        },
        EngineEvent::InstallDownloadFinished {
            slug: "pkg".to_string(),
            format: PluginFormat::Vst3,
            path: PathBuf::from("/tmp/pkg.zip"),
            bytes: 2,
        },
        EngineEvent::InstallArchiveInstallStarted {
            slug: "pkg".to_string(),
            format: PluginFormat::Vst3,
            install_type: InstallType::Zip,
            path: PathBuf::from("/tmp/pkg.zip"),
        },
        EngineEvent::InstallArchiveVerified {
            slug: "pkg".to_string(),
            format: PluginFormat::Vst3,
            path: PathBuf::from("/tmp/pkg.zip"),
            sha256: "abc".to_string(),
        },
        EngineEvent::InstallQuarantineRemovalStarted {
            slug: "pkg".to_string(),
            format: PluginFormat::Vst3,
            path: PathBuf::from("/tmp/pkg.vst3"),
        },
        EngineEvent::InstallFormatPlaced {
            slug: "pkg".to_string(),
            format: PluginFormat::Vst3,
            path: PathBuf::from("/tmp/pkg.vst3"),
        },
        EngineEvent::InstallStateRecordingStarted {
            slug: "pkg".to_string(),
        },
        EngineEvent::InstallStateRecorded {
            slug: "pkg".to_string(),
        },
        EngineEvent::InstallRolledBack {
            slug: "pkg".to_string(),
            format: PluginFormat::Vst3,
            path: PathBuf::from("/tmp/pkg.vst3"),
        },
        EngineEvent::InstallFinished {
            slug: "pkg".to_string(),
            installed_format_count: 1,
        },
        EngineEvent::InstallFailed {
            slug: "pkg".to_string(),
            error: "failed".to_string(),
        },
        EngineEvent::RemoveStarted {
            slug: "pkg".to_string(),
            version: "1.0.0".to_string(),
            format_count: 1,
        },
        EngineEvent::RemoveFormatRemoved {
            slug: "pkg".to_string(),
            format: PluginFormat::Vst3,
            path: PathBuf::from("/tmp/pkg.vst3"),
        },
        EngineEvent::RemoveFormatMissing {
            slug: "pkg".to_string(),
            format: PluginFormat::Vst3,
            path: PathBuf::from("/tmp/pkg.vst3"),
        },
        EngineEvent::RemoveStateRecorded {
            slug: "pkg".to_string(),
        },
        EngineEvent::RemoveFinished {
            slug: "pkg".to_string(),
            removed_format_count: 1,
        },
        EngineEvent::RemoveFailed {
            slug: "pkg".to_string(),
            error: "failed".to_string(),
        },
        EngineEvent::ModelWeightPullStarted {
            package_id: "model@1.0.0".to_string(),
        },
        EngineEvent::ModelWeightPullProgress {
            package_id: "model@1.0.0".to_string(),
            bytes: 1,
            total_bytes: Some(2),
        },
        EngineEvent::ModelWeightPullFinished {
            package_id: "model@1.0.0".to_string(),
            status: "present".to_string(),
            bytes: 2,
        },
        EngineEvent::ModelWeightPullFailed {
            package_id: "model@1.0.0".to_string(),
            error: "failed".to_string(),
        },
        EngineEvent::ModelInstallStarted {
            package_id: "model@1.0.0".to_string(),
        },
        EngineEvent::ModelInstallFinished {
            package_id: "model@1.0.0".to_string(),
            adapter: "mock".to_string(),
            runtime_mode: "native".to_string(),
            runtime_status: "prepared".to_string(),
            weights_status: "present".to_string(),
        },
        EngineEvent::ModelInstallFailed {
            package_id: "model@1.0.0".to_string(),
            error: "failed".to_string(),
        },
        EngineEvent::ModelRunStarted {
            package_id: "model@1.0.0".to_string(),
        },
        EngineEvent::ModelRunCompleted {
            package_id: "model@1.0.0".to_string(),
            output_path: "/tmp/out.wav".to_string(),
            message: "complete".to_string(),
        },
        EngineEvent::ModelRunBlocked {
            package_id: "model@1.0.0".to_string(),
            blocker: "adapter_runner_unavailable".to_string(),
            message: "blocked".to_string(),
        },
        EngineEvent::ModelRunFailed {
            package_id: "model@1.0.0".to_string(),
            error: "failed".to_string(),
        },
    ]
}
