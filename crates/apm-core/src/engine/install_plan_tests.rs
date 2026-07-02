use chrono::Utc;

use super::*;
use crate::config::Config;
use crate::state::{InstallOrigin, InstalledFormat, InstalledPlugin};

fn test_config() -> (tempfile::TempDir, Config) {
    let temp = tempfile::tempdir().expect("temp dir");
    let config = Config {
        data_dir: Some(temp.path().join("data")),
        cache_dir: Some(temp.path().join("cache")),
        ..Config::default()
    };
    write_test_registry(&config, temp.path());
    (temp, config)
}

fn write_test_registry(config: &Config, root: &std::path::Path) {
    write_test_registry_with_app_paths(config, root, true);
}

fn write_test_registry_with_app_paths(
    config: &Config,
    root: &std::path::Path,
    create_vendor_app: bool,
) {
    let registry_dir = config.registries_cache_dir().join("official");
    let plugins_dir = registry_dir.join("plugins");
    std::fs::create_dir_all(&plugins_dir).expect("create registry plugins dir");

    std::fs::write(
        plugins_dir.join("direct-synth.toml"),
        r#"
slug = "direct-synth"
name = "Direct Synth"
vendor = "Test Vendor"
version = "2.1.0"
description = "A direct install fixture"
category = "instruments"
license = "MIT"

[formats.vst3]
url = "https://example.com/direct-synth.zip"
sha256 = "def456"
install_type = "zip"
bundle_path = "DirectSynth.vst3"

[formats.au]
url = "https://example.com/direct-synth-au.zip"
sha256 = "def457"
install_type = "zip"
bundle_path = "DirectSynth.component"

[[releases]]
version = "2.0.0"

[releases.formats.vst3]
url = "https://example.com/direct-synth-2.0.0.zip"
sha256 = "def400"
install_type = "zip"
bundle_path = "DirectSynth.vst3"
"#,
    )
    .expect("write direct fixture");

    std::fs::write(
        plugins_dir.join("manual-effect.toml"),
        r#"
slug = "manual-effect"
name = "Manual Effect"
vendor = "Test Vendor"
version = "1.0.0"
description = "A manual install fixture"
category = "effects"
license = "freeware"
homepage = "https://example.com/manual"

[formats.vst3]
url = "manual"
sha256 = "manual"
install_type = "zip"
download_type = "manual"
"#,
    )
    .expect("write manual fixture");

    std::fs::write(
        plugins_dir.join("managed-synth.toml"),
        r#"
slug = "managed-synth"
name = "Managed Synth"
vendor = "Managed Vendor"
version = "3.0.0"
description = "A managed install fixture"
category = "instruments"
license = "commercial"
installer = "native-access"

[formats.vst3]
url = "native-access"
sha256 = "manual"
install_type = "pkg"
download_type = "managed"
"#,
    )
    .expect("write managed fixture");

    std::fs::write(
        plugins_dir.join("direct-pkg.toml"),
        r#"
slug = "direct-pkg"
name = "Direct PKG"
vendor = "Test Vendor"
version = "1.0.0"
description = "A privileged pkg install fixture"
category = "effects"
license = "MIT"

[formats.vst3]
url = "https://example.com/direct-pkg.pkg"
sha256 = "manual"
install_type = "pkg"
bundle_path = "DirectPkg.vst3"
"#,
    )
    .expect("write direct pkg fixture");

    std::fs::write(
        plugins_dir.join("app-store-synth.toml"),
        r#"
slug = "app-store-synth"
name = "App Store Synth"
vendor = "Test Vendor"
version = "1.0.0"
description = "An App Store install fixture"
category = "instruments"
license = "commercial"

[formats.app]
url = "https://apps.apple.com/us/app/app-store-synth/id123456789"
sha256 = "manual"
install_type = "mas"
bundle_path = "App Store Synth.app"
"#,
    )
    .expect("write app store fixture");

    std::fs::write(
        plugins_dir.join("preset-pack.toml"),
        r#"
slug = "preset-pack"
name = "Preset Pack"
vendor = "Test Vendor"
version = "1.0.0"
description = "A non-installable fixture"
category = "presets"
product_type = "preset_pack"
license = "freeware"

[formats.vst3]
url = "https://example.com/preset.zip"
sha256 = "abc123"
install_type = "zip"
"#,
    )
    .expect("write preset fixture");

    let fake_app = root.join("Native Access.app");
    if create_vendor_app {
        std::fs::write(&fake_app, "").expect("write fake app");
    }
    std::fs::write(
        registry_dir.join("installers.toml"),
        format!(
            r#"
[native-access]
name = "Native Access"
vendor = "Native Instruments"
app_paths = ["{}"]
download_url = "https://example.com/native-access"
homepage = "https://example.com/native"
"#,
            fake_app.to_string_lossy()
        ),
    )
    .expect("write installers fixture");
}

fn test_config_without_vendor_app() -> (tempfile::TempDir, Config) {
    let temp = tempfile::tempdir().expect("temp dir");
    let config = Config {
        data_dir: Some(temp.path().join("data")),
        cache_dir: Some(temp.path().join("cache")),
        ..Config::default()
    };
    write_test_registry_with_app_paths(&config, temp.path(), false);
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

fn installed_plugin(name: &str, version: &str) -> InstalledPlugin {
    InstalledPlugin {
        name: name.to_string(),
        version: version.to_string(),
        vendor: "Test Vendor".to_string(),
        formats: vec![InstalledFormat {
            format: PluginFormat::Vst3,
            path: PathBuf::from(format!("/tmp/{name}.vst3")),
            sha256: "abc123".to_string(),
        }],
        installed_at: Utc::now(),
        source: "official".to_string(),
        pinned: false,
        origin: InstallOrigin::Apm,
    }
}

#[test]
fn plan_install_returns_ready_direct_plan() {
    let (_temp, config) = test_config();
    let engine = ApmEngine::new(config);

    let result = engine
        .plan_install(InstallPlanRequest {
            slug: "direct-synth".to_string(),
            ..InstallPlanRequest::default()
        })
        .expect("plan should load");

    let InstallPlanResult::Plan { plan } = result else {
        panic!("expected install plan");
    };
    assert_eq!(plan.status, InstallPlanStatus::Ready);
    assert_eq!(
        plan.destination.as_deref(),
        Some("~/Library/Audio/Plug-Ins/")
    );
    assert_eq!(plan.formats.len(), 2);
}

#[test]
fn plan_install_resolves_requested_version_and_format() {
    let (_temp, config) = test_config();
    let engine = ApmEngine::new(config);

    let result = engine
        .plan_install(InstallPlanRequest {
            slug: "direct-synth".to_string(),
            version: Some("2.0.0".to_string()),
            format: Some(PluginFormat::Vst3),
            ..InstallPlanRequest::default()
        })
        .expect("plan should load");

    let InstallPlanResult::Plan { plan } = result else {
        panic!("expected install plan");
    };
    assert_eq!(plan.version, "2.0.0");
    assert_eq!(plan.formats.len(), 1);
    assert_eq!(plan.formats[0].format, PluginFormat::Vst3);
}

#[test]
fn plan_install_reports_already_installed() {
    let (_temp, config) = test_config();
    write_state(&config, vec![installed_plugin("direct-synth", "2.1.0")]);
    let engine = ApmEngine::new(config);

    let result = engine
        .plan_install(InstallPlanRequest {
            slug: "direct-synth".to_string(),
            format: Some(PluginFormat::Vst3),
            ..InstallPlanRequest::default()
        })
        .expect("plan should load");

    let InstallPlanResult::Plan { plan } = result else {
        panic!("expected install plan");
    };
    assert_eq!(plan.status, InstallPlanStatus::AlreadyInstalled);
    assert_eq!(plan.installed_version.as_deref(), Some("2.1.0"));
}

#[test]
fn plan_install_reports_manual_handoff() {
    let (_temp, config) = test_config();
    let engine = ApmEngine::new(config);

    let result = engine
        .plan_install(InstallPlanRequest {
            slug: "manual-effect".to_string(),
            ..InstallPlanRequest::default()
        })
        .expect("plan should load");

    let InstallPlanResult::Plan { plan } = result else {
        panic!("expected install plan");
    };
    assert_eq!(plan.status, InstallPlanStatus::ManualRequired);
    assert_eq!(plan.formats[0].source, "https://example.com/manual");
    assert!(!plan.formats[0].has_checksum);
}

#[test]
fn plan_install_reports_vendor_handoff() {
    let (_temp, config) = test_config();
    let engine = ApmEngine::new(config);

    let result = engine
        .plan_install(InstallPlanRequest {
            slug: "managed-synth".to_string(),
            ..InstallPlanRequest::default()
        })
        .expect("plan should load");

    let InstallPlanResult::Plan { plan } = result else {
        panic!("expected install plan");
    };
    assert_eq!(plan.status, InstallPlanStatus::VendorInstallerAvailable);
    assert_eq!(
        plan.installer
            .as_ref()
            .map(|installer| installer.key.as_str()),
        Some("native-access")
    );
}

#[test]
fn plan_install_reports_privileged_pkg_policy() {
    let (_temp, config) = test_config();
    let engine = ApmEngine::new(config);

    let result = engine
        .plan_install(InstallPlanRequest {
            slug: "direct-pkg".to_string(),
            ..InstallPlanRequest::default()
        })
        .expect("plan should load");

    let InstallPlanResult::Plan { plan } = result else {
        panic!("expected install plan");
    };
    assert_eq!(plan.status, InstallPlanStatus::PrivilegedInstallerRequired);
    assert!(plan
        .message
        .contains("PKG installers can run privileged scripts"));
    assert!(plan.destination.is_none());
}

#[test]
fn plan_install_reports_app_store_policy() {
    let (_temp, config) = test_config();
    let engine = ApmEngine::new(config);

    let result = engine
        .plan_install(InstallPlanRequest {
            slug: "app-store-synth".to_string(),
            ..InstallPlanRequest::default()
        })
        .expect("plan should load");

    let InstallPlanResult::Plan { plan } = result else {
        panic!("expected install plan");
    };
    assert_eq!(plan.status, InstallPlanStatus::AppStoreRequired);
    assert!(plan.message.contains("Mac App Store"));
    assert!(plan.destination.is_none());
}

#[test]
fn plan_install_rejects_non_installable_catalog_items() {
    let (_temp, config) = test_config();
    let engine = ApmEngine::new(config);

    let result = engine
        .plan_install(InstallPlanRequest {
            slug: "preset-pack".to_string(),
            ..InstallPlanRequest::default()
        })
        .expect("plan should load");

    assert!(matches!(
        result,
        InstallPlanResult::NotInstallable {
            product_type: ProductType::PresetPack,
            ..
        }
    ));
}

#[test]
fn install_handoff_returns_manual_download_target() {
    let (_temp, config) = test_config();
    let engine = ApmEngine::new(config);

    let result = engine
        .install_handoff(InstallPlanRequest {
            slug: "manual-effect".to_string(),
            ..InstallPlanRequest::default()
        })
        .expect("handoff should load");

    let InstallHandoffResult::Open { handoff, .. } = result else {
        panic!("expected open handoff");
    };
    assert_eq!(handoff.kind, InstallHandoffKind::ManualDownload);
    assert_eq!(
        handoff.target,
        InstallHandoffTarget::Url {
            url: "https://example.com/manual".to_string(),
        }
    );
}

#[test]
fn install_handoff_returns_vendor_app_target_when_installed() {
    let (temp, config) = test_config();
    let expected_path = temp.path().join("Native Access.app");
    let engine = ApmEngine::new(config);

    let result = engine
        .install_handoff(InstallPlanRequest {
            slug: "managed-synth".to_string(),
            ..InstallPlanRequest::default()
        })
        .expect("handoff should load");

    let InstallHandoffResult::Open { handoff, .. } = result else {
        panic!("expected open handoff");
    };
    assert_eq!(handoff.kind, InstallHandoffKind::VendorApp);
    assert_eq!(
        handoff.target,
        InstallHandoffTarget::App {
            path: expected_path,
        }
    );
}

#[test]
fn install_handoff_returns_vendor_download_when_app_missing() {
    let (_temp, config) = test_config_without_vendor_app();
    let engine = ApmEngine::new(config);

    let result = engine
        .install_handoff(InstallPlanRequest {
            slug: "managed-synth".to_string(),
            ..InstallPlanRequest::default()
        })
        .expect("handoff should load");

    let InstallHandoffResult::Open { handoff, .. } = result else {
        panic!("expected open handoff");
    };
    assert_eq!(handoff.kind, InstallHandoffKind::VendorDownload);
    assert_eq!(
        handoff.target,
        InstallHandoffTarget::Url {
            url: "https://example.com/native-access".to_string(),
        }
    );
}

#[test]
fn install_handoff_returns_privileged_pkg_download_target() {
    let (_temp, config) = test_config();
    let engine = ApmEngine::new(config);

    let result = engine
        .install_handoff(InstallPlanRequest {
            slug: "direct-pkg".to_string(),
            ..InstallPlanRequest::default()
        })
        .expect("handoff should load");

    let InstallHandoffResult::Open { handoff, .. } = result else {
        panic!("expected open handoff");
    };
    assert_eq!(handoff.kind, InstallHandoffKind::PrivilegedInstaller);
    assert_eq!(
        handoff.target,
        InstallHandoffTarget::Url {
            url: "https://example.com/direct-pkg.pkg".to_string(),
        }
    );
}

#[test]
fn install_handoff_returns_app_store_target() {
    let (_temp, config) = test_config();
    let engine = ApmEngine::new(config);

    let result = engine
        .install_handoff(InstallPlanRequest {
            slug: "app-store-synth".to_string(),
            ..InstallPlanRequest::default()
        })
        .expect("handoff should load");

    let InstallHandoffResult::Open { handoff, .. } = result else {
        panic!("expected open handoff");
    };
    assert_eq!(handoff.kind, InstallHandoffKind::AppStore);
    assert_eq!(
        handoff.target,
        InstallHandoffTarget::Url {
            url: "https://apps.apple.com/us/app/app-store-synth/id123456789".to_string(),
        }
    );
}

#[test]
fn install_handoff_reports_no_handoff_for_direct_install() {
    let (_temp, config) = test_config();
    let engine = ApmEngine::new(config);

    let result = engine
        .install_handoff(InstallPlanRequest {
            slug: "direct-synth".to_string(),
            ..InstallPlanRequest::default()
        })
        .expect("handoff should load");

    assert!(matches!(
        result,
        InstallHandoffResult::NoHandoff {
            plan,
            ..
        } if plan.status == InstallPlanStatus::Ready
    ));
}

#[test]
fn install_handoff_preserves_unavailable_plan_result() {
    let (_temp, config) = test_config();
    let engine = ApmEngine::new(config);

    let result = engine
        .install_handoff(InstallPlanRequest {
            slug: "preset-pack".to_string(),
            ..InstallPlanRequest::default()
        })
        .expect("handoff should load");

    assert!(matches!(
        result,
        InstallHandoffResult::PlanUnavailable {
            plan: InstallPlanResult::NotInstallable { .. }
        }
    ));
}
