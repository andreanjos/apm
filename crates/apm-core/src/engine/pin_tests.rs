use std::path::PathBuf;

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

fn write_state(config: &Config, plugins: Vec<InstalledPlugin>) {
    InstallState {
        version: 1,
        plugins,
    }
    .save(config)
    .expect("save state");
}

fn installed_plugin(name: &str, version: &str, pinned: bool) -> InstalledPlugin {
    InstalledPlugin {
        name: name.to_string(),
        version: version.to_string(),
        vendor: "Test Vendor".to_string(),
        formats: vec![InstalledFormat {
            format: PluginFormat::Vst3,
            path: PathBuf::from(format!("/tmp/{name}.vst3")),
            sha256: String::new(),
        }],
        installed_at: Utc::now(),
        source: "official".to_string(),
        pinned,
        origin: InstallOrigin::Apm,
    }
}

#[test]
fn set_package_pin_marks_installed_package_pinned() {
    let (_temp, config) = test_config();
    write_state(
        &config,
        vec![installed_plugin("direct-zip", "1.0.0", false)],
    );
    let engine = ApmEngine::new(config.clone());

    let result = engine
        .set_package_pin(SetPackagePinRequest {
            slug: "direct-zip".to_string(),
            pinned: true,
        })
        .expect("pin should save");

    assert!(matches!(
        result,
        SetPackagePinResult::Changed {
            package,
            pinned: true,
        } if package.slug == "direct-zip" && package.pinned
    ));
    assert!(
        InstallState::load(&config)
            .expect("state should load")
            .find("direct-zip")
            .expect("plugin should exist")
            .pinned
    );
}

#[test]
fn set_package_pin_unpins_installed_package() {
    let (_temp, config) = test_config();
    write_state(&config, vec![installed_plugin("direct-zip", "1.0.0", true)]);
    let engine = ApmEngine::new(config.clone());

    let result = engine
        .set_package_pin(SetPackagePinRequest {
            slug: "direct-zip".to_string(),
            pinned: false,
        })
        .expect("unpin should save");

    assert!(matches!(
        result,
        SetPackagePinResult::Changed {
            package,
            pinned: false,
        } if package.slug == "direct-zip" && !package.pinned
    ));
    assert!(
        !InstallState::load(&config)
            .expect("state should load")
            .find("direct-zip")
            .expect("plugin should exist")
            .pinned
    );
}

#[test]
fn set_package_pin_reports_unchanged_without_saving() {
    let (_temp, config) = test_config();
    write_state(&config, vec![installed_plugin("direct-zip", "1.0.0", true)]);
    let engine = ApmEngine::new(config);

    let result = engine
        .set_package_pin(SetPackagePinRequest {
            slug: "direct-zip".to_string(),
            pinned: true,
        })
        .expect("unchanged pin should classify");

    assert!(matches!(
        result,
        SetPackagePinResult::Unchanged {
            package,
            pinned: true,
        } if package.slug == "direct-zip" && package.pinned
    ));
}

#[test]
fn set_package_pin_reports_missing_package() {
    let (_temp, config) = test_config();
    let engine = ApmEngine::new(config);

    let result = engine
        .set_package_pin(SetPackagePinRequest {
            slug: "missing".to_string(),
            pinned: true,
        })
        .expect("missing pin should classify");

    assert_eq!(
        result,
        SetPackagePinResult::NotInstalled {
            slug: "missing".to_string()
        }
    );
}

#[test]
fn pinned_packages_returns_sorted_pinned_summaries() {
    let (_temp, config) = test_config();
    write_state(
        &config,
        vec![
            installed_plugin("z-delay", "1.0.0", true),
            installed_plugin("a-reverb", "2.0.0", true),
            installed_plugin("m-synth", "3.0.0", false),
        ],
    );
    let engine = ApmEngine::new(config);

    let pinned = engine
        .pinned_packages(PinnedPackagesRequest)
        .expect("pinned list should load");

    assert_eq!(pinned.len(), 2);
    assert_eq!(pinned[0].slug, "a-reverb");
    assert_eq!(pinned[1].slug, "z-delay");
}
