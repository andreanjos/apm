use std::io::Write;

#[cfg(feature = "reqwest")]
use sha2::{Digest, Sha256};
#[cfg(feature = "reqwest")]
use std::io::Read;
#[cfg(feature = "reqwest")]
use std::net::TcpListener;
#[cfg(feature = "reqwest")]
use std::thread;
use zip::write::SimpleFileOptions;

use super::*;

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
        plugins_dir.join("direct-zip.toml"),
        r#"
slug = "direct-zip"
name = "Direct ZIP"
vendor = "Test Vendor"
version = "1.0.0"
description = "A direct zip install fixture"
category = "effects"
license = "MIT"

[formats.vst3]
url = "https://example.com/direct-zip.zip"
sha256 = "manual"
install_type = "zip"
bundle_path = "DirectZip.vst3"
"#,
    )
    .expect("write direct zip fixture");
    std::fs::write(
        plugins_dir.join("multi-format.toml"),
        r#"
slug = "multi-format"
name = "Multi Format"
vendor = "Test Vendor"
version = "1.0.0"
description = "A multi-format fixture"
category = "effects"
license = "MIT"

[formats.vst3]
url = "https://example.com/multi-vst3.zip"
sha256 = "manual"
install_type = "zip"
bundle_path = "Multi.vst3"

[formats.au]
url = "https://example.com/multi-au.zip"
sha256 = "manual"
install_type = "zip"
bundle_path = "Multi.component"
"#,
    )
    .expect("write multi fixture");
    std::fs::write(
        plugins_dir.join("direct-dmg.toml"),
        r#"
slug = "direct-dmg"
name = "Direct DMG"
vendor = "Test Vendor"
version = "1.0.0"
description = "A direct dmg install fixture"
category = "effects"
license = "MIT"

[formats.vst3]
url = "https://example.com/direct-dmg.dmg"
sha256 = "manual"
install_type = "dmg"
bundle_path = "DirectDmg.vst3"
"#,
    )
    .expect("write direct dmg fixture");
    std::fs::write(
        plugins_dir.join("direct-pkg.toml"),
        r#"
slug = "direct-pkg"
name = "Direct PKG"
vendor = "Test Vendor"
version = "1.0.0"
description = "A direct pkg install fixture"
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
        plugins_dir.join("manual-effect.toml"),
        r#"
slug = "manual-effect"
name = "Manual Effect"
vendor = "Test Vendor"
version = "1.0.0"
description = "A manual install fixture"
category = "effects"
license = "freeware"

[formats.vst3]
url = "manual"
sha256 = "manual"
install_type = "zip"
download_type = "manual"
"#,
    )
    .expect("write manual fixture");
}

#[test]
fn install_package_from_archive_installs_zip_and_records_state() {
    let (temp, config) = test_config();
    let archive = temp.path().join("direct.zip");
    write_zip(&archive, &["DirectZip.vst3/Contents/Info.plist"]);
    let install_root = temp.path().join("install-root");
    let engine = ApmEngine::new(config.clone());
    let mut events = Vec::new();

    let result = engine
        .install_package_from_archive_with_dest_resolver(
            InstallPackageRequest {
                slug: "direct-zip".to_string(),
                format: Some(PluginFormat::Vst3),
                archive_path: Some(archive),
                ..InstallPackageRequest::default()
            },
            &mut |event| events.push(event),
            |format, _scope| install_root.join(format.to_string().to_lowercase()),
        )
        .expect("install should succeed");

    let InstallPackageResult::Installed { package } = result else {
        panic!("expected installed result");
    };
    assert_eq!(package.slug, "direct-zip");
    assert_eq!(
        package.formats[0].path,
        install_root.join("vst3/DirectZip.vst3")
    );
    assert!(package.formats[0].path.join("Contents/Info.plist").exists());

    let state = InstallState::load(&config).expect("state should load");
    assert_eq!(state.find("direct-zip").expect("recorded").version, "1.0.0");
    assert!(events.iter().any(|event| {
        matches!(
            event,
            EngineEvent::InstallArchiveInstallStarted {
                slug,
                format: PluginFormat::Vst3,
                install_type: InstallType::Zip,
                path
            } if slug == "direct-zip" && path.ends_with("direct.zip")
        )
    }));
    assert!(events.iter().any(|event| {
        matches!(
            event,
            EngineEvent::InstallQuarantineRemovalStarted {
                slug,
                format: PluginFormat::Vst3,
                path
            } if slug == "direct-zip" && path == &install_root.join("vst3/DirectZip.vst3")
        )
    }));
    assert!(events.iter().any(|event| {
        matches!(
            event,
            EngineEvent::InstallStateRecordingStarted { slug } if slug == "direct-zip"
        )
    }));
    assert!(events.iter().any(|event| {
        matches!(
            event,
            EngineEvent::InstallFinished {
                slug,
                installed_format_count: 1
            } if slug == "direct-zip"
        )
    }));
}

struct CancelAfterInstallStart {
    events: Vec<EngineEvent>,
}

impl EventSink for CancelAfterInstallStart {
    fn emit(&mut self, event: EngineEvent) {
        self.events.push(event);
    }

    fn cancel_requested(&self) -> bool {
        self.events
            .iter()
            .any(|event| matches!(event, EngineEvent::InstallStarted { .. }))
    }
}

#[test]
fn install_package_from_archive_honors_cancellation_before_placement() {
    let (temp, config) = test_config();
    let archive = temp.path().join("direct.zip");
    write_zip(&archive, &["DirectZip.vst3/Contents/Info.plist"]);
    let install_root = temp.path().join("install-root");
    let engine = ApmEngine::new(config);
    let mut events = CancelAfterInstallStart { events: Vec::new() };

    let error = engine
        .install_package_from_archive_with_dest_resolver(
            InstallPackageRequest {
                slug: "direct-zip".to_string(),
                format: Some(PluginFormat::Vst3),
                archive_path: Some(archive),
                ..InstallPackageRequest::default()
            },
            &mut events,
            |format, _scope| install_root.join(format.to_string().to_lowercase()),
        )
        .expect_err("install should stop on cancellation");

    assert_eq!(
        error.to_string(),
        crate::engine::OPERATION_CANCELED_BY_REQUEST
    );
    assert!(!install_root.join("vst3/DirectZip.vst3").exists());
}

struct CancelAfterQuarantineStart {
    events: Vec<EngineEvent>,
}

impl EventSink for CancelAfterQuarantineStart {
    fn emit(&mut self, event: EngineEvent) {
        self.events.push(event);
    }

    fn cancel_requested(&self) -> bool {
        self.events
            .iter()
            .any(|event| matches!(event, EngineEvent::InstallQuarantineRemovalStarted { .. }))
    }
}

#[test]
fn install_package_from_archive_rolls_back_when_canceled_after_placement() {
    let (temp, config) = test_config();
    let archive = temp.path().join("direct.zip");
    write_zip(&archive, &["DirectZip.vst3/Contents/Info.plist"]);
    let install_root = temp.path().join("install-root");
    let engine = ApmEngine::new(config);
    let mut events = CancelAfterQuarantineStart { events: Vec::new() };

    let error = engine
        .install_package_from_archive_with_dest_resolver(
            InstallPackageRequest {
                slug: "direct-zip".to_string(),
                format: Some(PluginFormat::Vst3),
                archive_path: Some(archive),
                ..InstallPackageRequest::default()
            },
            &mut events,
            |format, _scope| install_root.join(format.to_string().to_lowercase()),
        )
        .expect_err("install should stop on cancellation");

    let installed_path = install_root.join("vst3/DirectZip.vst3");
    assert_eq!(
        error.to_string(),
        crate::engine::OPERATION_CANCELED_BY_REQUEST
    );
    assert!(!installed_path.exists());
    assert!(events.events.iter().any(|event| {
        matches!(
            event,
            EngineEvent::InstallRolledBack {
                slug,
                format: PluginFormat::Vst3,
                path
            } if slug == "direct-zip" && path == &installed_path
        )
    }));
}

#[test]
fn install_package_from_archive_requires_format_for_multi_archive() {
    let (_temp, config) = test_config();
    let engine = ApmEngine::new(config);
    let mut noop = |_| {};

    let result = engine
        .install_package_from_archive(
            InstallPackageRequest {
                slug: "multi-format".to_string(),
                archive_path: Some(PathBuf::from("/tmp/unused.zip")),
                ..InstallPackageRequest::default()
            },
            &mut noop,
        )
        .expect("request should be classified");

    assert!(matches!(
        result,
        InstallPackageResult::FormatRequired {
            available_formats,
            ..
        } if available_formats == vec![PluginFormat::Au, PluginFormat::Vst3]
    ));
}

#[test]
fn install_package_from_archive_rolls_back_when_state_save_fails() {
    let (temp, mut config) = test_config();
    let archive = temp.path().join("direct.zip");
    write_zip(&archive, &["DirectZip.vst3/Contents/Info.plist"]);

    let blocked_data_dir = temp.path().join("blocked-data");
    std::fs::write(&blocked_data_dir, "not a directory").expect("write blocked data path");
    config.data_dir = Some(blocked_data_dir);

    let install_root = temp.path().join("install-root");
    let engine = ApmEngine::new(config);
    let mut events = Vec::new();

    engine
        .install_package_from_archive_with_dest_resolver(
            InstallPackageRequest {
                slug: "direct-zip".to_string(),
                format: Some(PluginFormat::Vst3),
                archive_path: Some(archive),
                ..InstallPackageRequest::default()
            },
            &mut |event| events.push(event),
            |format, _scope| install_root.join(format.to_string().to_lowercase()),
        )
        .expect_err("state save should fail");

    let installed_path = install_root.join("vst3/DirectZip.vst3");
    assert!(!installed_path.exists());
    assert!(events.iter().any(|event| {
        matches!(
            event,
            EngineEvent::InstallRolledBack {
                slug,
                format: PluginFormat::Vst3,
                path
            } if slug == "direct-zip" && path == &installed_path
        )
    }));
    assert!(events.iter().any(|event| {
        matches!(
            event,
            EngineEvent::InstallFailed { slug, .. } if slug == "direct-zip"
        )
    }));
}

#[test]
fn install_package_from_archive_checks_state_before_copying() {
    let (temp, config) = test_config();
    let archive = temp.path().join("direct.zip");
    write_zip(&archive, &["DirectZip.vst3/Contents/Info.plist"]);
    std::fs::create_dir_all(config.resolved_data_dir()).expect("create data dir");
    std::fs::write(config.state_file(), "not valid toml").expect("write corrupt state");

    let install_root = temp.path().join("install-root");
    let engine = ApmEngine::new(config);
    let mut events = Vec::new();

    engine
        .install_package_from_archive_with_dest_resolver(
            InstallPackageRequest {
                slug: "direct-zip".to_string(),
                format: Some(PluginFormat::Vst3),
                archive_path: Some(archive),
                ..InstallPackageRequest::default()
            },
            &mut |event| events.push(event),
            |format, _scope| install_root.join(format.to_string().to_lowercase()),
        )
        .expect_err("corrupt state should fail before copy");

    assert!(!install_root.join("vst3/DirectZip.vst3").exists());
    assert!(events.is_empty());
}

#[test]
fn install_package_from_archive_preserves_privileged_pkg_policy() {
    let (_temp, config) = test_config();
    let engine = ApmEngine::new(config);
    let mut noop = |_| {};

    let result = engine
        .install_package_from_archive(
            InstallPackageRequest {
                slug: "direct-pkg".to_string(),
                format: Some(PluginFormat::Vst3),
                ..InstallPackageRequest::default()
            },
            &mut noop,
        )
        .expect("request should be classified");

    assert!(matches!(
        result,
        InstallPackageResult::ExternalHandoffRequired {
            reason,
            ..
        } if reason.contains("privileged scripts")
    ));
}

#[test]
fn install_package_from_archive_preserves_manual_handoff() {
    let (_temp, config) = test_config();
    let engine = ApmEngine::new(config);
    let mut noop = |_| {};

    let result = engine
        .install_package_from_archive(
            InstallPackageRequest {
                slug: "manual-effect".to_string(),
                format: Some(PluginFormat::Vst3),
                ..InstallPackageRequest::default()
            },
            &mut noop,
        )
        .expect("request should be classified");

    assert!(matches!(
        result,
        InstallPackageResult::ExternalHandoffRequired { .. }
    ));
}

#[cfg(feature = "reqwest")]
#[test]
fn install_package_from_url_downloads_zip_and_records_state() {
    let (temp, config) = test_config();
    let archive = temp.path().join("download.zip");
    write_zip(&archive, &["Downloaded.vst3/Contents/Info.plist"]);
    let archive_bytes = std::fs::read(&archive).expect("read archive");
    let sha256 = sha256_hex(&archive_bytes);
    let server = serve_once(archive_bytes);
    let download_url = server.url.clone();
    write_download_registry(&config, &server.url, &sha256);

    let install_root = temp.path().join("install-root");
    let engine = ApmEngine::new(config.clone());
    let mut events = Vec::new();

    let result = engine
        .install_package_from_url_with_dest_resolver(
            InstallPackageRequest {
                slug: "download-zip".to_string(),
                format: Some(PluginFormat::Vst3),
                ..InstallPackageRequest::default()
            },
            &mut |event| events.push(event),
            |format, _scope| install_root.join(format.to_string().to_lowercase()),
        )
        .expect("url install should succeed");

    server.join();

    let InstallPackageResult::Installed { package } = result else {
        panic!("expected installed result");
    };
    assert_eq!(package.slug, "download-zip");
    assert_eq!(
        package.formats[0].path,
        install_root.join("vst3/Downloaded.vst3")
    );
    assert!(package.formats[0].path.join("Contents/Info.plist").exists());
    assert!(config
        .downloads_cache_dir()
        .join("download-zip-1.0.0-vst3.zip")
        .exists());
    assert!(InstallState::load(&config)
        .expect("state should load")
        .find("download-zip")
        .is_some());
    assert!(events.iter().any(|event| {
        matches!(
            event,
            EngineEvent::InstallDownloadStarted {
                slug,
                format: PluginFormat::Vst3,
                url
            } if slug == "download-zip" && url == &download_url
        )
    }));
    assert!(events.iter().any(|event| {
        matches!(
            event,
            EngineEvent::InstallDownloadProgress {
                slug,
                format: PluginFormat::Vst3,
                bytes,
                total_bytes
            } if slug == "download-zip" && *bytes > 0 && *total_bytes == Some(*bytes)
        )
    }));
    assert!(events.iter().any(|event| {
        matches!(
            event,
            EngineEvent::InstallDownloadFinished {
                slug,
                format: PluginFormat::Vst3,
                bytes,
                ..
            } if slug == "download-zip" && *bytes > 0
        )
    }));
    assert!(events.iter().any(|event| {
        matches!(
            event,
            EngineEvent::InstallArchiveInstallStarted {
                slug,
                format: PluginFormat::Vst3,
                install_type: InstallType::Zip,
                path
            } if slug == "download-zip"
                && path.ends_with("download-zip-1.0.0-vst3.zip")
        )
    }));
}

#[cfg(feature = "reqwest")]
#[test]
fn install_package_from_url_deletes_bad_checksum_download() {
    let (temp, config) = test_config();
    let archive = temp.path().join("bad-download.zip");
    write_zip(&archive, &["Downloaded.vst3/Contents/Info.plist"]);
    let archive_bytes = std::fs::read(&archive).expect("read archive");
    let server = serve_once(archive_bytes);
    write_download_registry(&config, &server.url, "bad-sha");

    let install_root = temp.path().join("install-root");
    let engine = ApmEngine::new(config.clone());
    let mut events = Vec::new();

    engine
        .install_package_from_url_with_dest_resolver(
            InstallPackageRequest {
                slug: "download-zip".to_string(),
                format: Some(PluginFormat::Vst3),
                ..InstallPackageRequest::default()
            },
            &mut |event| events.push(event),
            |format, _scope| install_root.join(format.to_string().to_lowercase()),
        )
        .expect_err("checksum should fail");

    server.join();

    assert!(!config
        .downloads_cache_dir()
        .join("download-zip-1.0.0-vst3.zip")
        .exists());
    assert!(!install_root.join("vst3/Downloaded.vst3").exists());
    assert!(events.iter().any(|event| {
        matches!(
            event,
            EngineEvent::InstallDownloadStarted {
                slug,
                format: PluginFormat::Vst3,
                ..
            } if slug == "download-zip"
        )
    }));
    assert!(events.iter().any(
        |event| matches!(event, EngineEvent::InstallFailed { slug, .. } if slug == "download-zip")
    ));
    assert!(!events
        .iter()
        .any(|event| matches!(event, EngineEvent::InstallFormatPlaced { .. })));
}

#[cfg(feature = "reqwest")]
struct CancelAfterDownloadProgress {
    events: Vec<EngineEvent>,
}

#[cfg(feature = "reqwest")]
impl EventSink for CancelAfterDownloadProgress {
    fn emit(&mut self, event: EngineEvent) {
        self.events.push(event);
    }

    fn cancel_requested(&self) -> bool {
        self.events
            .iter()
            .any(|event| matches!(event, EngineEvent::InstallDownloadProgress { .. }))
    }
}

#[cfg(feature = "reqwest")]
#[test]
fn install_package_from_url_deletes_partial_download_when_canceled() {
    let (temp, config) = test_config();
    let archive = temp.path().join("cancel-download.zip");
    write_zip(&archive, &["Downloaded.vst3/Contents/Info.plist"]);
    let archive_bytes = std::fs::read(&archive).expect("read archive");
    let sha256 = sha256_hex(&archive_bytes);
    let server = serve_once(archive_bytes);
    write_download_registry(&config, &server.url, &sha256);

    let install_root = temp.path().join("install-root");
    let engine = ApmEngine::new(config.clone());
    let mut events = CancelAfterDownloadProgress { events: Vec::new() };

    let error = engine
        .install_package_from_url_with_dest_resolver(
            InstallPackageRequest {
                slug: "download-zip".to_string(),
                format: Some(PluginFormat::Vst3),
                ..InstallPackageRequest::default()
            },
            &mut events,
            |format, _scope| install_root.join(format.to_string().to_lowercase()),
        )
        .expect_err("download should stop on cancellation");

    server.join();

    let archive_path = config
        .downloads_cache_dir()
        .join("download-zip-1.0.0-vst3.zip");
    assert_eq!(
        error.to_string(),
        crate::engine::OPERATION_CANCELED_BY_REQUEST
    );
    assert!(!archive_path.exists());
    assert!(!part_path(&archive_path).exists());
    assert!(!install_root.join("vst3/Downloaded.vst3").exists());
    assert!(events.events.iter().any(|event| {
        matches!(
            event,
            EngineEvent::InstallFailed { slug, .. } if slug == "download-zip"
        )
    }));
}

fn write_zip(path: &Path, entries: &[&str]) {
    let file = std::fs::File::create(path).expect("create zip");
    let mut writer = zip::ZipWriter::new(file);
    let options = SimpleFileOptions::default().unix_permissions(0o644);

    for entry in entries {
        writer.start_file(entry, options).expect("start zip file");
        writer.write_all(b"test").expect("write zip file");
    }

    writer.finish().expect("finish zip");
}

#[cfg(feature = "reqwest")]
fn write_download_registry(config: &Config, url: &str, sha256: &str) {
    let plugins_dir = config.registries_cache_dir().join("official/plugins");
    std::fs::write(
        plugins_dir.join("download-zip.toml"),
        format!(
            r#"
slug = "download-zip"
name = "Download ZIP"
vendor = "Test Vendor"
version = "1.0.0"
description = "A download zip install fixture"
category = "effects"
license = "MIT"

[formats.vst3]
url = "{url}"
sha256 = "{sha256}"
install_type = "zip"
bundle_path = "Downloaded.vst3"
"#
        ),
    )
    .expect("write download fixture");
}

#[cfg(feature = "reqwest")]
fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

#[cfg(feature = "reqwest")]
struct TestServer {
    url: String,
    handle: thread::JoinHandle<()>,
}

#[cfg(feature = "reqwest")]
impl TestServer {
    fn join(self) {
        self.handle.join().expect("server thread should finish");
    }
}

#[cfg(feature = "reqwest")]
fn serve_once(body: Vec<u8>) -> TestServer {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind test server");
    let url = format!(
        "http://{}/download.zip",
        listener.local_addr().expect("addr")
    );
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept request");
        let mut buffer = [0_u8; 1024];
        let _ = stream.read(&mut buffer);
        let header = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        stream.write_all(header.as_bytes()).expect("write header");
        stream.write_all(&body).expect("write body");
    });
    TestServer { url, handle }
}
